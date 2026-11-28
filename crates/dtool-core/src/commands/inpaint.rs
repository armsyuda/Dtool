use crate::document::DocInstance;
use crate::geometry::{apply_matrix, invert_matrix, object_matrix};
use crate::inpaint::{self, CancelToken, InpaintRequest, RasterView, SampleFrom, StagedRegion};
use crate::selection::{PixelSelection, PixelSurface};
use crate::{Command, CommandError, CommandResult, Document, Object, ObjectId, Session};
use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, Mutex, atomic::{AtomicU8, AtomicU64, Ordering}, mpsc};
use super::pixel_brush::{clone_layer, edit_layer, install_pixels};

pub const TOOL: &str = "tool.inpaint";
pub const ACTIVATE: &str = "pixel.inpaint";
pub const FILL_SELECTION: &str = "pixel.content_aware.fill";
pub const STROKE: &str = "pixel.inpaint.stroke";
pub const PREVIEW: &str = "pixel.inpaint.preview";
pub const QUERY: &str = "pixel.inpaint.query";
pub const APPLY: &str = "pixel.inpaint.apply";
pub const CANCEL: &str = "pixel.inpaint.cancel";
pub const WIDTH: &str = "inpaint.width";
pub const OPACITY: &str = "inpaint.opacity";
pub const SOURCE: &str = "inpaint.source";
const MAX_MASK_PIXELS: u64 = 32 * 1024 * 1024;
const MAX_PREVIEW_PIXELS: u64 = 8 * 1024 * 1024;
static NEXT_REVISION: AtomicU64 = AtomicU64::new(1 << 63);

#[derive(Debug, Clone, Copy, PartialEq)]
struct Settings { opacity: f32, source: SampleFrom }

impl Settings {
    fn current(session: &Session) -> Self {
        let source = match session.tool_options.choice(TOOL, SOURCE).as_deref() {
            Some("layer_below") => SampleFrom::LayerBelow,
            Some("all_layers") => SampleFrom::AllLayers,
            _ => SampleFrom::CurrentLayer,
        };
        Self { opacity: session.tool_options.number(TOOL, OPACITY).unwrap_or(100.0) as f32 / 100.0, source }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pin { instance: DocInstance, generation: u64, target: ObjectId }

impl Pin {
    fn current(session: &Session) -> Option<Self> {
        (session.active_tool == TOOL).then(|| Self::selected(session)).flatten()
    }

    fn selected(session: &Session) -> Option<Self> {
        if !session.has_document { return None; }
        clone_layer(session).map(|target| Self { instance: session.document.instance, generation: session.generation, target })
    }
}

#[derive(Debug)]
struct Job {
    token: Arc<CancelToken>,
    progress: Arc<AtomicU8>,
    result: Mutex<mpsc::Receiver<Result<StagedRegion, CommandError>>>,
}

impl Drop for Job {
    fn drop(&mut self) { self.token.cancel(); }
}

#[derive(Debug)]
struct Draft {
    pin: Pin,
    action: &'static str,
    mask: PixelSelection,
    bounds: [u32; 4],
    revision: u64,
    settings: Option<Settings>,
    job: Option<Job>,
    preview: Option<Arc<Document>>,
    error: Option<CommandError>,
}

#[derive(Debug, Default)]
pub struct State { draft: Option<Draft> }

impl State {
    pub fn pending(&self) -> bool { self.draft.as_ref().is_some_and(|draft| draft.job.is_some()) }
}

#[derive(Deserialize)]
struct Stroke { points: Vec<[f32; 2]> }

#[derive(Clone, Copy)]
struct RasterMap { position: [f32; 2], scale: [f32; 2], matrix: [f32; 6], inverse: [f32; 6] }

impl RasterMap {
    fn new(object: &Object, size: [u32; 2]) -> Result<Self, CommandError> {
        let matrix = object_matrix(object);
        let inverse = invert_matrix(&matrix).ok_or_else(CommandError::invalid_params)?;
        let scale = [size[0] as f32 / object.size[0].abs(), size[1] as f32 / object.size[1].abs()];
        if scale.iter().any(|value| *value <= 0.0 || !value.is_finite()) { return Err(CommandError::invalid_params()); }
        Ok(Self { position: object.pos, scale, matrix, inverse })
    }

    fn raster(self, point: [f32; 2]) -> [f32; 2] {
        let [x, y] = apply_matrix(&self.inverse, point);
        [(x - self.position[0]) * self.scale[0], (y - self.position[1]) * self.scale[1]]
    }

    fn world(self, [x, y]: [f32; 2]) -> [f32; 2] {
        apply_matrix(&self.matrix, [self.position[0] + x / self.scale[0], self.position[1] + y / self.scale[1]])
    }
}

fn dimensions(session: &Session, pin: Pin) -> Result<(&Object, [u32; 2], RasterMap), CommandError> {
    let object = session.document.get_object(pin.target).ok_or_else(CommandError::not_found)?;
    let size = crate::raster_source::dimensions(&session.document, object)
        .ok_or_else(CommandError::invalid_params)?.map(|side| side as u32);
    if u64::from(size[0]) * u64::from(size[1]) > MAX_MASK_PIXELS {
        return Err(CommandError::new("REQUEST_TOO_LARGE"));
    }
    Ok((object, size, RasterMap::new(object, size)?))
}

pub fn can_paint(session: &Session) -> bool { Pin::current(session).is_some() }

pub fn brush_diameter(session: &Session) -> Option<f32> {
    let (_, _, map) = dimensions(session, Pin::current(session)?).ok()?;
    Some(session.tool_options.number(TOOL, WIDTH).unwrap_or(40.0) as f32 / map.scale[0])
}

pub fn feedback(session: &Session) -> Option<(Option<u8>, Option<&str>)> {
    let draft = session.inpaint.draft.as_ref()?;
    if Some(draft.pin) != Pin::current(session) { return None; }
    Some((draft.job.as_ref().map(|job| job.progress.load(Ordering::Relaxed)),
        draft.error.as_ref().map(|error| error.message_key.as_str())))
}

pub fn mask_overlay(session: &Session) -> Option<(u64, &PixelSelection, [u32; 4], [[f32; 2]; 4])> {
    let draft = session.inpaint.draft.as_ref()?;
    if Some(draft.pin) != Pin::current(session) || draft.preview.is_some() { return None; }
    let [x, y, w, h] = draft.bounds;
    let (_, _, map) = dimensions(session, draft.pin).ok()?;
    let corners = [[x,y], [x+w,y], [x+w,y+h], [x,y+h]].map(|p| map.world(p.map(|v| v as f32)));
    Some((draft.revision, &draft.mask, draft.bounds, corners))
}

pub fn staged_document(session: &Session) -> Option<(u64, Arc<Document>)> {
    let draft = session.inpaint.draft.as_ref()?;
    if Some(draft.pin) == Pin::current(session) && draft.settings == Some(Settings::current(session)) {
        Some((draft.revision, draft.preview.as_ref()?.clone()))
    } else { None }
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    let draft = session.inpaint.draft.as_ref();
    Some(match command.name.as_str() {
        ACTIVATE => session.has_document,
        FILL_SELECTION => Pin::selected(session).is_some() && session.document.pixel_selection.is_some()
            && !session.inpaint.pending(),
        STROKE => can_paint(session),
        PREVIEW => can_paint(session) && !session.inpaint.pending()
            && (draft.is_some() || session.document.pixel_selection.is_some()),
        APPLY => staged_document(session).is_some(),
        CANCEL => draft.is_some(),
        QUERY => true,
        _ => return None,
    })
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    can_execute(session, command)?;
    Some(execute(session, command))
}

pub fn poll(session: &mut Session) {
    let pin = Pin::current(session);
    let settings = Settings::current(session);
    let Some(draft) = session.inpaint.draft.as_mut() else { return };
    if Some(draft.pin) != pin {
        session.inpaint.draft = None;
        return;
    }
    if draft.settings.is_some_and(|old| old != settings) {
        draft.job = None;
        draft.preview = None;
        draft.settings = None;
        draft.error = None;
        return;
    }
    let Some(job) = draft.job.as_mut() else { return };
    let result = match job.result.get_mut().map_or(Err(mpsc::TryRecvError::Disconnected), |receiver| receiver.try_recv()) {
        Ok(result) => result,
        Err(mpsc::TryRecvError::Empty) => return,
        Err(mpsc::TryRecvError::Disconnected) => Err(CommandError::new("INPAINT_FAILED")),
    };
    let pin = draft.pin;
    draft.job = None;
    let result = result.and_then(|region| preview_document(session, pin, region));
    let Some(draft) = session.inpaint.draft.as_mut() else { return };
    match result {
        Ok(document) => {
            draft.preview = Some(Arc::new(document));
            draft.revision = NEXT_REVISION.fetch_add(1, Ordering::Relaxed);
        }
        Err(error) => draft.error = Some(error),
    }
}

fn preview_document(session: &Session, pin: Pin, region: StagedRegion) -> Result<Document, CommandError> {
    let Some((object, mut pixels, _)) = edit_layer(session, Some(pin.target), None, false)? else {
        return Err(CommandError::invalid_params());
    };
    if region.rgba.is_empty() || pixels.read_region(region.rect) == region.rgba {
        return Err(CommandError::new("INPAINT_NO_CHANGE"));
    }
    pixels.set_region(region.rect, &region.rgba);
    let mut document = session.document.clone();
    install_pixels(&mut document, &session.selection.ids, object, pixels, false);
    Ok(document)
}

fn intersect_selection(session: &Session, mask: &mut PixelSelection, map: RasterMap) {
    let Some(selection) = session.document.pixel_selection.as_ref() else { return };
    let surface = PixelSurface::for_page(&session.document, selection.page_id);
    let Some([x0, y0, w, h]) = mask.bounds() else { return };
    for y in y0..y0+h {
        for x in x0..x0+w {
            let at = (y * mask.width + x) as usize;
            let old = mask.coverage[at];
            if old == 0 { continue; }
            let world = map.world([x as f32 + 0.5, y as f32 + 0.5]);
            let selected = surface.as_ref().map_or(0, |surface| surface.coverage_at(selection, world));
            mask.coverage[at] = ((u16::from(old) * u16::from(selected) + 127) / 255) as u8;
        }
    }
}

fn new_draft(pin: Pin, mask: PixelSelection, action: &'static str) -> Draft {
    let bounds = mask.bounds().unwrap_or_default();
    Draft { pin, action, mask, bounds, settings: None, job: None, preview: None, error: None,
        revision: NEXT_REVISION.fetch_add(1, Ordering::Relaxed) }
}

fn execute(session: &mut Session, command: &Command) -> CommandResult {
    poll(session);
    match command.name.as_str() {
        ACTIVATE => session.run_named("ui.tool.set", json!({ "id": TOOL })),
        FILL_SELECTION => {
            if session.inpaint.pending() { return Err(CommandError::new("INPAINT_RUNNING")); }
            let pin = Pin::selected(session).ok_or_else(CommandError::invalid_params)?;
            let (_, size, map) = dimensions(session, pin)?;
            let mask = selection_mask(session, size, map)?;
            let draft = new_draft(pin, mask, FILL_SELECTION);
            preview_roi(draft.bounds, size)?;
            session.run_named("ui.tool.set", json!({ "id": TOOL }))?;
            session.inpaint.draft = Some(draft);
            start_preview(session)
        }
        CANCEL => {
            session.inpaint.draft = None;
            Ok(json!({ "phase": "empty" }))
        }
        QUERY => {
            let Some(draft) = session.inpaint.draft.as_ref() else { return Ok(json!({ "phase": "empty" })) };
            let phase = if draft.job.is_some() { "running" } else if draft.preview.is_some() { "ready" } else if draft.error.is_some() { "failed" } else { "masked" };
            Ok(json!({ "phase": phase, "target": draft.pin.target.0,
                "progress": draft.job.as_ref().map(|job| job.progress.load(Ordering::Relaxed)),
                "error": draft.error.as_ref().map(|error| &error.code) }))
        }
        STROKE => {
            let pin = Pin::current(session).ok_or_else(CommandError::invalid_params)?;
            let (_, [w, h], map) = dimensions(session, pin)?;
            let stroke: Stroke = serde_json::from_value(command.params.clone()).map_err(|_| CommandError::invalid_params())?;
            if stroke.points.is_empty() || stroke.points.len() > 8192 || stroke.points.iter().flatten().any(|v| !v.is_finite()) {
                return Err(CommandError::invalid_params());
            }
            let width = session.tool_options.number(TOOL, WIDTH).unwrap_or(40.0) as f32;
            let local: Vec<_> = stroke.points.into_iter().map(|point| map.raster(point)).collect();
            let mut mask = inpaint::mask_from_stroke(w, h, &local, width);
            intersect_selection(session, &mut mask, map);
            if mask.is_empty() { return Ok(json!({ "changes": [] })); }
            if let Some(old) = session.inpaint.draft.take() {
                for (next, old_value) in mask.coverage.iter_mut().zip(old.mask.coverage.iter()) {
                    *next = (*next).max(*old_value);
                }
            }
            session.inpaint.draft = Some(new_draft(pin, mask, APPLY));
            Ok(json!({ "phase": "masked" }))
        }
        PREVIEW => start_preview(session),
        APPLY => {
            let (_, document) = staged_document(session).ok_or_else(CommandError::invalid_params)?;
            let selection = session.selection.ids.clone();
            let action = session.inpaint.draft.take().ok_or_else(CommandError::invalid_params)?.action;
            super::document_change::commit(session, action, (*document).clone(), selection,
                json!({ "engine_version": inpaint::ENGINE_VERSION }))
        }
        _ => Err(CommandError::invalid_params()),
    }
}

fn selection_mask(session: &Session, [width, height]: [u32; 2], map: RasterMap) -> Result<PixelSelection, CommandError> {
    if session.document.pixel_selection.is_none() { return Err(CommandError::invalid_params()); }
    let mut mask = PixelSelection::all(width, height);
    intersect_selection(session, &mut mask, map);
    if mask.is_empty() { return Err(CommandError::invalid_params()); }
    Ok(mask)
}

fn preview_roi([x, y, width, height]: [u32; 4], size: [u32; 2]) -> Result<[u32; 4], CommandError> {
    let [left, top] = [x.saturating_sub(64), y.saturating_sub(64)];
    let right = (x + width + 64).min(size[0]);
    let bottom = (y + height + 64).min(size[1]);
    let [width, height] = [right - left, bottom - top];
    if u64::from(width) * u64::from(height) > MAX_PREVIEW_PIXELS {
        return Err(CommandError::new("REQUEST_TOO_LARGE"));
    }
    Ok([left, top, width, height])
}

fn start_preview(session: &mut Session) -> CommandResult {
    let pin = Pin::current(session).ok_or_else(CommandError::invalid_params)?;
    let (_, size, map) = dimensions(session, pin)?;
    let settings = Settings::current(session);
    if session.inpaint.pending() { return Err(CommandError::new("INPAINT_RUNNING")); }
    if session.inpaint.draft.is_none() {
        let mask = selection_mask(session, size, map)?;
        session.inpaint.draft = Some(new_draft(pin, mask, APPLY));
    }
    let Some((object, pixels, _)) = edit_layer(session, Some(pin.target), None, false)? else {
        return Err(CommandError::invalid_params());
    };
    let draft = session.inpaint.draft.as_mut().ok_or_else(CommandError::invalid_params)?;
    let roi = preview_roi(draft.bounds, size)?;
    let [left, top, width, height] = roi;
    let coverage = (top..top + height).flat_map(|row| {
        let offset = (row * size[0] + left) as usize;
        draft.mask.coverage[offset..offset + width as usize].iter().copied()
    }).collect();
    let mask = PixelSelection::from_coverage(roi[2], roi[3], coverage);
    let document = (settings.source != SampleFrom::CurrentLayer).then(|| session.document.clone());
    let token = Arc::new(CancelToken::new());
    let progress = Arc::new(AtomicU8::new(0));
    let (send, receive) = mpsc::channel();
    let (worker_token, worker_progress) = (token.clone(), progress.clone());
    std::thread::Builder::new().name("dtool-inpaint".into()).spawn(move || {
        let base = RasterView { width: roi[2], height: roi[3], rgba: pixels.read_region(roi) };
        let request = InpaintRequest { node: pin.target, mask, opacity: settings.opacity, sample_from: settings.source, seed: pin.target.0.max(1) };
        let result = (|| {
            if worker_token.is_cancelled() { return Err(CommandError::new("CANCELLED")); }
            let sampled = document.as_ref().map(|doc| sample_layers(doc, &object, settings.source, map, roi)).transpose()?;
            let source = sampled.as_ref().unwrap_or(&base);
            let mut region = inpaint::run_over(&request, source, &base, &worker_token, &mut |p| {
                worker_progress.store((100 * p.done / p.total.max(1)) as u8, Ordering::Relaxed);
            })?;
            region.rect[0] += roi[0];
            region.rect[1] += roi[1];
            Ok(region)
        })();
        let _ = send.send(result);
    }).map_err(|_| CommandError::new("INPAINT_FAILED"))?;
    draft.settings = Some(settings);
    draft.preview = None;
    draft.error = None;
    draft.job = Some(Job { token, progress, result: Mutex::new(receive) });
    Ok(json!({ "phase": "running" }))
}

fn sample_layers(
    doc: &Document, object: &Object, source: SampleFrom, map: RasterMap, [x, y, w, h]: [u32; 4],
) -> Result<RasterView, CommandError> {
    let world = [[x,y], [x+w,y], [x+w,y+h], [x,y+h]].map(|p| map.world(p.map(|v| v as f32)));
    let left = world.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
    let top = world.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
    let width = world.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max) - left;
    let height = world.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max) - top;
    let bounds = [left, top, width, height];
    if bounds.iter().any(|v| !v.is_finite()) || width <= 0.0 || height <= 0.0 {
        return Err(CommandError::invalid_params());
    }
    let flags = crate::CanvasFlags::default();
    let raster = match source {
        SampleFrom::LayerBelow => crate::layer_style::raster::render_below_object_region(doc, &flags, object.id, bounds, [w, h])
            .ok_or_else(|| CommandError::new("INPAINT_NO_SOURCE"))?,
        _ => crate::layer_style::raster::render_region(doc, &flags, &[], bounds, w, h),
    };
    if raster.len() != w as usize * h as usize * 4 { return Err(CommandError::new("INPAINT_NO_SOURCE")); }
    let mut rgba = Vec::with_capacity(raster.len());
    for j in 0..h {
        for i in 0..w {
            let [wx, wy] = map.world([(x+i) as f32 + 0.5, (y+j) as f32 + 0.5]);
            let rx = ((wx - left) / width * w as f32).floor().clamp(0.0, w as f32 - 1.0) as u32;
            let ry = ((wy - top) / height * h as f32).floor().clamp(0.0, h as f32 - 1.0) as u32;
            let at = ((ry * w + rx) * 4) as usize;
            rgba.extend_from_slice(&raster[at..at+4]);
        }
    }
    Ok(RasterView { width: w, height: h, rgba })
}
