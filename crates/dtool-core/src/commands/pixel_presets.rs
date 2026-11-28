use crate::brush::{BrushImage, BrushLibrary, BrushTip, MAX_SUB_BRUSHES, TipKind, fit_stamp_size};
use crate::{Command, CommandError, CommandResult, Session};
use serde_json::{json, Value};
use std::{fs, io::Read, path::{Path, PathBuf}, sync::{Arc, Mutex, OnceLock}};

const MAX_FILE: u64 = 64 * 1024 * 1024;
const MAX_TIPS: usize = 512;
const MAX_CATEGORIES: usize = 128;
const BASIC: &str = "Basic";
const PREFIX: &str = "brush.presets.";
pub const APPLY: &str = "brush.presets.apply";
pub const EDITOR_OPEN: &str = "brush.presets.editor.open";
pub const EDITOR_QUERY: &str = "brush.presets.editor.query";
pub const EDITOR_SET: &str = "brush.presets.editor.set";
type ExternalImporter = fn(&str, &str, &[u8]) -> Result<ExternalBrushes, CommandError>;

pub struct ExternalBrushes {
    pub tips: Vec<BrushTip>,
    pub skipped: usize,
    pub limited: usize,
}

static EXTERNAL_IMPORTER: OnceLock<ExternalImporter> = OnceLock::new();

pub fn register_external_importer(importer: ExternalImporter) {
    let _ = EXTERNAL_IMPORTER.set(importer);
}

static LIBRARY: Mutex<Option<Arc<BrushLibrary>>> = Mutex::new(None);

fn path() -> PathBuf {
    crate::prefs::Prefs::default_path().with_file_name("pixel_brush_presets.json")
}

fn library_error() -> CommandError { CommandError::new("IO_ERROR") }

fn read_file(path: &Path) -> Result<Vec<u8>, CommandError> {
    let mut bytes = Vec::new();
    fs::File::open(path).map_err(|_| library_error())?.take(MAX_FILE + 1)
        .read_to_end(&mut bytes).map_err(|_| library_error())?;
    if bytes.len() as u64 > MAX_FILE { return Err(CommandError::invalid_params()); }
    Ok(bytes)
}

fn decode(path: &Path) -> Result<BrushLibrary, CommandError> {
    let library: BrushLibrary = serde_json::from_slice(&read_file(path)?).map_err(|_| CommandError::invalid_params())?;
    validate(&library)?;
    Ok(library)
}

fn validate(library: &BrushLibrary) -> Result<(), CommandError> {
    let mut categories = std::collections::HashSet::new();
    let mut ids = std::collections::HashSet::new();
    if library.categories.len() > MAX_CATEGORIES || library.tips.len() > MAX_TIPS
        || !library.categories.iter().all(|category| valid_label(category) && categories.insert(category))
        || !library.tips.iter().all(|tip| {
            valid_label(&tip.id) && ids.insert(&tip.id) && valid_label(&tip.name)
                && categories.contains(&tip.category)
                && valid_tip(tip)
        })
    {
        return Err(CommandError::invalid_params());
    }
    Ok(())
}

fn valid_tip(tip: &BrushTip) -> bool {
    let shape_valid = match tip.kind {
        TipKind::Round | TipKind::Square => tip.image.is_none() && tip.additional_images.is_empty(),
        TipKind::Image | TipKind::Intensity => tip.image.as_ref().is_some_and(BrushImage::valid),
    };
    shape_valid && tip.additional_images.len() <= 7 && tip.additional_images.iter().all(BrushImage::valid)
        && tip.texture.as_ref().is_none_or(|texture| texture.valid())
        && tip.wet_edges.curve.as_ref().is_none_or(|curve| curve.valid())
        && tip.sub_brushes.len() <= MAX_SUB_BRUSHES
        && tip.sub_brushes.iter().all(|child| supports_sub_brush(&child.tip) && valid_tip(&child.tip))
        && (1.0..=2048.0).contains(&tip.size_px) && (0.01..=9.99).contains(&tip.spacing)
        && (0.01..=1.0).contains(&tip.roundness)
        && (-180.0..=180.0).contains(&tip.rotation) && tip.dynamics.valid()
        && [tip.opacity, tip.flow, tip.hardness].into_iter().chain(tip.accumulation)
            .all(|value| (0.0..=1.0).contains(&value))
}

pub fn supports_sub_brush(tip: &BrushTip) -> bool {
    tip.sub_brushes.is_empty()
}

fn omit_legacy_imported_pressure(saved: &BrushTip, previous: &mut BrushTip, include_opacity: bool) {
    use crate::brush::{Controller, ControllerKind};

    let default = Controller::default();
    for (old, new) in [
        (&saved.dynamics.size, &mut previous.dynamics.size),
        (&saved.dynamics.flow, &mut previous.dynamics.flow),
    ].into_iter().chain(include_opacity.then_some((&saved.dynamics.opacity, &mut previous.dynamics.opacity))) {
        if old == &default && new.kind == ControllerKind::Pressure {
            *new = default.clone();
        }
    }
}

pub fn editor_tool(tool: &str) -> Option<&'static str> {
    crate::context_toolbar::canonical_tool(tool).filter(|tool| {
        *tool != crate::context_toolbar::CALLIGRAPHY_TOOL
            && super::pixel_brush::operation(tool).is_some_and(|op| op != crate::brush::BrushOp::Pixel)
    })
}

fn editor_tip(session: &Session, tool: &str) -> Result<BrushTip, CommandError> {
    let op = super::pixel_brush::operation(tool).ok_or_else(CommandError::invalid_params)?;
    let settings = super::pixel_brush::settings_for(session, tool, op);
    let current = session.tool_options.raster_tip(tool);
    Ok(BrushTip {
        id: current.map_or_else(|| "custom".into(), |tip| tip.id.clone()),
        name: current.map_or_else(|| dtool_strings::text("brush_editor.current").into(), |tip| tip.name.clone()),
        category: current.map_or_else(|| BASIC.into(), |tip| tip.category.clone()),
        kind: settings.tip_kind,
        spacing: settings.spacing,
        size_px: settings.size_px,
        opacity: settings.opacity,
        accumulation: current.and_then(|tip| tip.accumulation),
        flow: settings.flow,
        hardness: settings.hardness,
        roundness: settings.roundness,
        rotation: settings.rotation,
        dynamics: settings.dynamics,
        image: settings.stamp,
        additional_images: settings.additional_stamps,
        wet_edges: current.map(|tip| tip.wet_edges.clone()).unwrap_or_default(),
        texture: settings.texture,
        sub_brushes: settings.sub_brushes,
    })
}

/// Temporary adjustments to the current tool. The saved preset stays intact.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TipAdjustment {
    SizeHardness { size_px: f32, hardness: f32 },
    ShapeSpacing { roundness: f32, spacing: f32 },
    Rotation(f32),
}

pub fn adjust_tip(session: &mut Session, adjustment: TipAdjustment) {
    use crate::context_toolbar::{CALLIGRAPHY_ANGLE, CALLIGRAPHY_ROUNDNESS, CALLIGRAPHY_TOOL};
    let tool = session.active_tool.clone();
    let Ok(mut tip) = editor_tip(session, &tool) else { return; };
    match adjustment {
        TipAdjustment::SizeHardness { size_px, hardness } => {
            tip.size_px = size_px;
            tip.hardness = hardness;
            session.tool_options.set_number(&tool, "brush.width", size_px.into());
            session.tool_options.set_number(&tool, "brush.hardness", f64::from(hardness) * 100.0);
        }
        TipAdjustment::ShapeSpacing { roundness, spacing } => {
            tip.roundness = roundness;
            tip.spacing = spacing;
            if tool == CALLIGRAPHY_TOOL {
                session.tool_options.set_number(&tool, CALLIGRAPHY_ROUNDNESS, f64::from(roundness) * 100.0);
            }
        }
        TipAdjustment::Rotation(rotation) => {
            tip.rotation = rotation;
            if tool == CALLIGRAPHY_TOOL {
                session.tool_options.set_number(&tool, CALLIGRAPHY_ANGLE, rotation.into());
            }
        }
    }
    session.tool_options.set_raster_tip(&tool, tip);
}

fn apply_tip(session: &mut Session, tool: &str, tip: BrushTip) {
    for (name, value) in [
        ("brush.width", tip.size_px),
        ("brush.opacity", tip.opacity * 100.0),
        ("brush.flow", tip.flow * 100.0),
        ("brush.hardness", tip.hardness * 100.0),
    ] {
        session.tool_options.set(tool, name, &json!(value));
    }
    if let Some(enabled) = tip.wet_edges.enabled {
        session.tool_options.set(tool, "brush.wet_edges", &json!(enabled));
    }
    session.tool_options.set_raster_tip(tool, tip);
}

fn cached_library(cache: &mut Option<Arc<BrushLibrary>>) -> Result<Arc<BrushLibrary>, CommandError> {
    if let Some(library) = cache { return Ok(Arc::clone(library)); }
    let path = path();
    let library = Arc::new(if path.exists() { decode(&path)? } else { BrushLibrary::builtin() });
    *cache = Some(Arc::clone(&library));
    Ok(library)
}

pub fn library_snapshot() -> Result<Arc<BrushLibrary>, CommandError> {
    let mut cache = LIBRARY.lock().map_err(|_| library_error())?;
    cached_library(&mut cache)
}

pub fn library() -> Result<BrushLibrary, CommandError> {
    Ok(library_snapshot()?.as_ref().clone())
}

fn update(edit: impl FnOnce(&mut BrushLibrary) -> CommandResult) -> CommandResult {
    let mut cache = LIBRARY.lock().map_err(|_| library_error())?;
    let mut candidate = cached_library(&mut cache)?.as_ref().clone();
    let result = edit(&mut candidate)?;
    validate(&candidate)?;
    let path = path();
    if let Some(parent) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|_| library_error())?;
    }
    let data = serde_json::to_vec_pretty(&candidate).map_err(|_| library_error())?;
    if data.len() as u64 > MAX_FILE { return Err(CommandError::new("REQUEST_TOO_LARGE")); }
    crate::native_io::write_atomic(&path, &data).map_err(|_| library_error())?;
    *cache = Some(Arc::new(candidate));
    Ok(result)
}

pub fn builtin(id: &str) -> bool { matches!(id, "round_hard" | "round_soft" | "square_hard") }

fn valid_label(value: &str) -> bool {
    !value.is_empty() && value == value.trim() && value.chars().count() <= 64
        && !value.chars().any(char::is_control)
}

fn label<'a>(params: &'a Value, field: &str) -> Result<&'a str, CommandError> {
    params.get(field).and_then(Value::as_str).map(str::trim).filter(|name| valid_label(name))
        .ok_or_else(CommandError::invalid_params)
}

fn fresh_id(library: &mut BrushLibrary) -> Result<String, CommandError> {
    let id = (library.next_id.max(1)..u64::MAX).map(|i| (i, format!("user-{i}")))
        .find(|(_, id)| library.get(id).is_none()).ok_or_else(CommandError::invalid_params)?;
    library.next_id = id.0 + 1;
    Ok(id.1)
}

fn category_exists(library: &BrushLibrary, category: &str) -> Result<(), CommandError> {
    if library.categories.iter().any(|known| known == category) { Ok(()) } else { Err(CommandError::not_found()) }
}

pub(crate) fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    let verb = command.name.strip_prefix(PREFIX)?;
    Some(match verb {
        "editor.open" => session.has_document && editor_tool(&session.active_tool).is_some(),
        "apply" | "editor.query" | "editor.set" => session.has_document,
        "from_selection" => selection_can_stamp(session),
        _ => true,
    })
}

pub(crate) fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    let verb = command.name.strip_prefix(PREFIX)?;
    Some(execute(session, verb, &command.params))
}

fn execute(session: &mut Session, verb: &str, params: &Value) -> CommandResult {
    match verb {
        "editor.query" | "editor.set" => {
            if !session.has_document { return Err(CommandError::no_document()); }
            let tool = params.get("tool").and_then(Value::as_str).unwrap_or(&session.active_tool);
            let tool = editor_tool(tool).ok_or_else(CommandError::invalid_params)?;
            let id = params.get("id").map(|_| label(params, "id")).transpose()?;
            if verb == "editor.query" {
                let tip = if let Some(id) = id {
                    library_snapshot()?.get(id).ok_or_else(CommandError::not_found)?.clone()
                } else {
                    editor_tip(session, tool)?
                };
                return Ok(json!({"tool": tool, "tip": tip}));
            }
            let mut tip: BrushTip = serde_json::from_value(params["tip"].clone()).map_err(|_| CommandError::invalid_params())?;
            if !valid_tip(&tip) { return Err(CommandError::invalid_params()); }
            if params.get("save_as").is_some() {
                let name = label(params, "save_as")?;
                update(|library| {
                    category_exists(library, &tip.category)?;
                    tip.id = fresh_id(library)?;
                    tip.name = name.to_owned();
                    library.tips.push(tip.clone());
                    Ok(Value::Null)
                })?;
            } else if let Some(id) = id {
                if builtin(id) { return Err(CommandError::invalid_params()); }
                update(|library| {
                    let saved = library.tips.iter_mut().find(|saved| saved.id == id).ok_or_else(CommandError::not_found)?;
                    tip.id = saved.id.clone();
                    tip.name = saved.name.clone();
                    tip.category = saved.category.clone();
                    *saved = tip.clone();
                    Ok(Value::Null)
                })?;
            } else {
                let current = editor_tip(session, tool)?;
                tip.id = current.id;
                tip.name = current.name;
                tip.category = current.category;
            }
            apply_tip(session, tool, tip);
            Ok(json!({"tool": tool, "changes": []}))
        }
        "list" => {
            let library = library_snapshot()?;
            let tool = params.get("tool").and_then(Value::as_str).unwrap_or(&session.active_tool);
            Ok(json!({"categories": library.categories, "presets": library.tips,
                "active": session.tool_options.raster_tip(tool).map(|tip| &tip.id)}))
        }
        "apply" => {
            if !session.has_document { return Err(CommandError::no_document()); }
            let id = label(params, "id")?;
            let library = library_snapshot()?;
            let tip = library.get(id).ok_or_else(CommandError::not_found)?.clone();
            let tool = params.get("tool").and_then(Value::as_str).unwrap_or(&session.active_tool);
            let tool = editor_tool(tool).unwrap_or("tool.paint.brush");
            apply_tip(session, tool, tip);
            session.active_tool = tool.to_owned();
            Ok(json!({"id": id, "tool": tool, "changes": []}))
        }
        "new" | "from_selection" => {
            let name = if params.get("name").is_none() && params.get("path").is_some() {
                label_path(params)?.file_stem().and_then(|value| value.to_str()).map(str::trim)
                    .filter(|name| valid_label(name)).unwrap_or("Brush")
            } else {
                label(params, "name")?
            };
            let category = params.get("category").and_then(Value::as_str).unwrap_or(BASIC);
            let (kind, image) = if verb == "from_selection" {
                (TipKind::Image, Some(stamp_from_selection(session)?))
            } else {
                match params.get("kind").and_then(Value::as_str) {
                    Some("image" | "intensity") => {
                        let path = label_path(params)?;
                        let decoded = super::image_place::decode_path(path)?;
                        let stamp = BrushImage::from_rgba(decoded.resource.pixel_size, decoded.rgba)
                            .ok_or_else(CommandError::invalid_params)?;
                        let kind = if params["kind"] == "image" { TipKind::Image } else { TipKind::Intensity };
                        (kind, Some(stamp))
                    }
                    Some("square") => (TipKind::Square, None),
                    None | Some("round") => (TipKind::Round, None),
                    _ => return Err(CommandError::invalid_params()),
                }
            };
            let number = |key, fallback: f32| -> Result<f32, CommandError> {
                match params.get(key) {
                    Some(value) => value.as_f64().map(|value| value as f32).ok_or_else(CommandError::invalid_params),
                    None => Ok(fallback),
                }
            };
            let op = super::pixel_brush::operation(&session.active_tool).unwrap_or(crate::brush::BrushOp::Paint);
            let settings = super::pixel_brush::settings(session, op);
            let size = image.as_ref().map_or(settings.size_px, |stamp| stamp.size[0].max(stamp.size[1]) as f32);
            update(|library| {
                category_exists(library, category)?;
                let id = fresh_id(library)?;
                library.tips.push(BrushTip {
                    id: id.clone(), name: name.to_owned(), category: category.to_owned(), kind, image,
                    additional_images: Vec::new(),
                    texture: None,
                    sub_brushes: Vec::new(),
                    wet_edges: Default::default(),
                    size_px: number("size_px", size)?, spacing: number("spacing", settings.spacing)?,
                    opacity: number("opacity", settings.opacity)?, flow: number("flow", settings.flow)?,
                    accumulation: params.get("accumulation").map(|_| number("accumulation", 1.0))
                        .transpose()?.or(settings.accumulation),
                    hardness: number("hardness", settings.hardness)?,
                    roundness: number("roundness", settings.roundness)?,
                    rotation: if matches!(kind, TipKind::Image | TipKind::Intensity) { 0.0 } else { settings.rotation },
                    dynamics: settings.dynamics,
                });
                Ok(json!({"id": id, "changes": []}))
            })
        }
        "rename" | "delete" | "duplicate" => {
            let id = label(params, "id")?;
            update(|library| {
                let index = library.tips.iter().position(|tip| tip.id == id).ok_or_else(CommandError::not_found)?;
                if builtin(id) && verb != "duplicate" { return Err(CommandError::invalid_params()); }
                match verb {
                    "rename" => library.tips[index].name = label(params, "name")?.to_owned(),
                    "delete" => { library.tips.remove(index); }
                    _ => {
                        let mut tip = library.tips[index].clone();
                        tip.id = fresh_id(library)?;
                        tip.name = label(params, "name")?.to_owned();
                        library.tips.push(tip);
                    }
                }
                Ok(json!({"changes": []}))
            })
        }
        "category.add" | "category.rename" | "category.delete" => update(|library| {
            let name = label(params, "name")?;
            match verb {
                "category.add" => {
                    if category_exists(library, name).is_ok() { return Err(CommandError::invalid_params()); }
                    library.categories.push(name.to_owned());
                }
                "category.rename" => {
                    let next = label(params, "new_name")?;
                    let index = library.categories.iter().position(|value| value == name).ok_or_else(CommandError::not_found)?;
                    if name == BASIC || category_exists(library, next).is_ok() { return Err(CommandError::invalid_params()); }
                    library.categories[index] = next.to_owned();
                    for tip in library.tips.iter_mut().filter(|tip| tip.category == name) { tip.category = next.to_owned(); }
                }
                _ => {
                    category_exists(library, name)?;
                    if name == BASIC || library.tips.iter().any(|tip| tip.category == name) {
                        return Err(CommandError::invalid_params());
                    }
                    library.categories.retain(|category| category != name);
                }
            }
            Ok(json!({"changes": []}))
        }),
        "import" => {
            let path = label_path(params)?;
            let affinity = path.extension().and_then(|value| value.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("afbrushes"));
            let photoshop = path.extension().and_then(|value| value.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("abr"));
            let (imported, skipped, limited) = import_library(path)?;
            let category = imported.categories.first().cloned();
            update(|library| {
                for category in imported.categories {
                    if category_exists(library, &category).is_err() { library.categories.push(category); }
                }
                let mut added = 0;
                let mut updated = 0;
                for mut tip in imported.tips {
                    if let Some(saved) = library.tips.iter_mut().find(|saved| saved.id == tip.id) {
                        if saved == &tip { continue; }
                        let mut previous = tip.clone();
                        if affinity || photoshop { omit_legacy_imported_pressure(saved, &mut previous, photoshop); }
                        if photoshop {
                            if saved.dynamics.rotation_jitter == 0.0 { previous.dynamics.rotation_jitter = 0.0; }
                            if saved.dynamics.direction_rotation == 0.0 { previous.dynamics.direction_rotation = 0.0; }
                            if saved.dynamics.size_jitter.is_default() { previous.dynamics.size_jitter = Default::default(); }
                            if saved.dynamics.roundness_jitter.is_default() { previous.dynamics.roundness_jitter = Default::default(); }
                            if saved.dynamics.path_scatter.is_default() { previous.dynamics.path_scatter = Default::default(); }
                            if saved.dynamics.size == Default::default() && saved.dynamics.tilt_scale.is_none()
                                && previous.dynamics.size.kind == crate::brush::ControllerKind::Tilt
                            {
                                previous.dynamics.size = Default::default();
                                previous.dynamics.tilt_scale = None;
                            }
                        }
                        if saved.texture.is_none() { previous.texture = None; }
                        if saved.wet_edges.is_default() { previous.wet_edges = Default::default(); }
                        if saved.sub_brushes.is_empty() {
                            previous.sub_brushes.clear();
                        } else {
                            for (old, new) in saved.sub_brushes.iter().zip(&mut previous.sub_brushes) {
                                if old.tip.wet_edges.is_default() { new.tip.wet_edges = Default::default(); }
                                if affinity { omit_legacy_imported_pressure(&old.tip, &mut new.tip, false); }
                            }
                        }
                        if saved == &previous {
                            *saved = tip;
                            updated += 1;
                            continue;
                        }
                    }
                    if library.tips.len() >= MAX_TIPS { return Err(CommandError::invalid_params()); }
                    if library.get(&tip.id).is_some() || builtin(&tip.id) { tip.id = fresh_id(library)?; }
                    library.tips.push(tip);
                    added += 1;
                }
                Ok(json!({"imported": added, "updated": updated, "skipped": skipped, "limited": limited.min(added + updated), "category": category, "changes": []}))
            })
        }
        "export" => {
            use std::io::Write;
            let path = interchange_path(params)?;
            let mut library = library()?;
            if let Some(category) = params.get("category").and_then(Value::as_str) {
                category_exists(&library, category)?;
                library.tips.retain(|tip| tip.category == category);
                library.categories.retain(|name| name == category);
            }
            let data = serde_json::to_vec_pretty(&library).map_err(|_| library_error())?;
            if params["replace_existing"].as_bool() == Some(true) {
                crate::native_io::write_atomic(path, &data).map_err(|_| library_error())?;
                return Ok(json!({"exported": library.tips.len(), "changes": []}));
            }
            let mut output = fs::OpenOptions::new().create_new(true).write(true).open(path).map_err(|_| library_error())?;
            if output.write_all(&data).and_then(|()| output.sync_all()).is_err() {
                drop(output);
                let _ = fs::remove_file(path);
                return Err(library_error());
            }
            Ok(json!({"exported": library.tips.len(), "changes": []}))
        }
        _ => Err(CommandError::invalid_params()),
    }
}

fn selection_can_stamp(session: &Session) -> bool {
    session.has_document && (session.document.pixel_selection.as_ref().is_some_and(|mask| !mask.is_empty())
        || session.selection.ids.iter().any(|id| crate::scene::effective_pickable(&session.document, *id)))
}

fn stamp_from_selection(session: &Session) -> Result<BrushImage, CommandError> {
    if !session.has_document { return Err(CommandError::no_document()); }
    let doc = &session.document;
    let flags = crate::CanvasFlags::default();
    let nothing = || CommandError::new("NOTHING_SELECTED");

    let render = |doc: &crate::Document, bounds: [f32; 4], size: [u32; 2]| {
        let [w, h] = fit_stamp_size(size);
        let rgba = crate::layer_style::raster::render_region_scaled(doc, &flags, &[], bounds, w, h, w as f32 / bounds[2]);
        ([w, h], rgba)
    };
    let (size, rgba) = if let Some(mask) = doc.pixel_selection.as_ref().filter(|mask| !mask.is_empty()) {
        let surface = crate::selection::PixelSurface::for_page(doc, mask.page_id).filter(|surface| surface.matches(doc, mask))
            .ok_or_else(CommandError::invalid_params)?;
        let [x, y, w, h] = mask.bounds().ok_or_else(nothing)?;
        let [left, top] = surface.pixel_to_doc([x as f32, y as f32]);
        let [right, bottom] = surface.pixel_to_doc([(x + w) as f32, (y + h) as f32]);
        let ([out_w, out_h], mut rgba) = render(doc, [left, top, right - left, bottom - top], [w, h]);
        for (index, pixel) in rgba.chunks_exact_mut(4).enumerate() {
            let px = index as u32 % out_w;
            let py = index as u32 / out_w;
            let coverage = if [w, h] == [out_w, out_h] {
                u16::from(mask.get(x + px, y + py))
            } else {
                let sample = |at: u32, source: u32, target: u32, offset: f64| {
                    ((f64::from(at) + offset) * f64::from(source) / f64::from(target)) as u32
                };
                let mut total = 0;
                for oy in [0.25, 0.75] {
                    for ox in [0.25, 0.75] {
                        total += u16::from(mask.get(x + sample(px, w, out_w, ox), y + sample(py, h, out_h, oy)));
                    }
                }
                (total + 2) / 4
            };
            pixel[3] = ((u16::from(pixel[3]) * coverage + 127) / 255) as u8;
        }
        ([out_w, out_h], rgba)
    } else {
        let ids: Vec<_> = session.selection.ids.iter().copied().filter(|id| crate::scene::effective_pickable(doc, *id)).collect();
        if ids.is_empty() { return Err(nothing()); }
        let mut kept = std::collections::HashSet::new();
        for id in &ids {
            kept.extend(doc.subtree_ids(*id));
            let mut parent = doc.get_object(*id).and_then(|object| object.parent_id);
            while let Some(id) = parent.filter(|id| kept.insert(*id)) {
                parent = doc.get_object(id).and_then(|object| object.parent_id);
            }
        }
        let mut source = doc.clone();
        source.objects.retain(|object| kept.contains(&object.id));
        let scene = crate::scene::estimate_scene(&source, &flags);
        let bounds = crate::scene_hit::selection_box(&scene, &source, &ids)
            .filter(|bounds| bounds.iter().all(|value| value.is_finite()) && bounds[2] > 0.0 && bounds[3] > 0.0)
            .ok_or_else(nothing)?;
        let scale = crate::units::to_px(1.0, doc.units, doc.dpi).ok_or_else(CommandError::invalid_params)?;
        let size = [bounds[2], bounds[3]].map(|side| (side * scale).ceil().max(1.0) as u32);
        render(&source, bounds, size)
    };
    BrushImage::from_rgba(size, rgba).filter(BrushImage::valid).ok_or_else(nothing)
}

fn label_path(params: &Value) -> Result<&Path, CommandError> {
    params.get("path").and_then(Value::as_str).filter(|path| !path.is_empty())
        .map(Path::new).ok_or_else(CommandError::invalid_params)
}

fn import_library(path: &Path) -> Result<(BrushLibrary, usize, usize), CommandError> {
    use sha2::{Digest, Sha256};
    let extension = path.extension().and_then(|part| part.to_str()).unwrap_or("").to_ascii_lowercase();
    if extension == "dtoolbrushes" {
        return Ok((decode(path)?, 0, 0));
    }
    if !matches!(extension.as_str(), "abr" | "afbrushes") {
        return Err(CommandError::invalid_params());
    }
    let importer = EXTERNAL_IMPORTER.get().ok_or_else(|| CommandError::new("NOT_AVAILABLE"))?;
    let stem = path.file_stem().and_then(|name| name.to_str()).unwrap_or("");
    let bytes = read_file(path)?;
    let ExternalBrushes { mut tips, skipped, limited } = importer(&extension, stem, &bytes)?;
    let hash: String = Sha256::digest(&bytes)[..16].iter().map(|byte| format!("{byte:02x}")).collect();
    let mut categories = Vec::new();
    for (index, tip) in tips.iter_mut().enumerate() {
        tip.id = format!("import-{}-{}", hash, index + 1);
        if !categories.contains(&tip.category) {
            categories.push(tip.category.clone());
        }
    }
    let library = BrushLibrary { categories, tips, next_id: 0 };
    validate(&library)?;
    Ok((library, skipped, limited))
}

fn interchange_path(params: &Value) -> Result<&Path, CommandError> {
    let path = label_path(params)?;
    if path.extension().and_then(|value| value.to_str()).is_some_and(|value| value.eq_ignore_ascii_case("dtoolbrushes")) {
        Ok(path)
    } else {
        Err(CommandError::invalid_params())
    }
}
