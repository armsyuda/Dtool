use super::fonts::FontLibrary;
use super::story::{CharPatch, Story, TextRange};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
pub const FONT_LIST: &str = "font.list";
pub const FONT_FILES: &str = "font.files";
pub const FONT_PREVIEW: &str = "text.font.preview";
pub const FONT_PREVIEW_CANCEL: &str = "text.font.preview.cancel";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontPreview {
    pub story: u64,
    pub start: usize,
    pub end: usize,
    pub family: String,
    pub style: Option<String>,
    /// DocInstance of the previewed document; previews never show in another document.
    #[serde(default, skip_serializing)]
    pub doc: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum FontReply {
    List { families: Vec<String> },
    Preview { preview: Option<FontPreview> },
}

static PREVIEW: Mutex<Vec<FontPreview>> = Mutex::new(Vec::new());

struct StylePreview {
    document: u64,
    version: u64,
    stories: Vec<dtool_core::Story>,
}

static STYLE_PREVIEW: Mutex<Option<StylePreview>> = Mutex::new(None);

pub fn set_style_preview(doc: &dtool_core::Document, command: &dtool_core::Command) -> Result<(), dtool_core::CommandError> {
    let mut view = doc.clone();
    let commands = if command.name == "edit.batch" {
        dtool_core::serde_json::from_value(command.params["commands"].clone())
            .map_err(|_| dtool_core::CommandError::invalid_params())?
    } else {
        vec![command.clone()]
    };
    for edit in &commands {
        dtool_core::text_cmd::apply(edit, &mut view)?;
    }
    let stories = view.stories.into_iter().filter(|story| {
        commands.iter().any(|edit| edit.params["story"].as_u64() == Some(story.id.0))
    }).collect();
    *STYLE_PREVIEW.lock().unwrap_or_else(|error| error.into_inner()) = Some(StylePreview {
        document: doc.instance.0,
        version: doc.version,
        stories,
    });
    bump_view();
    Ok(())
}

pub fn cancel_style_preview() -> bool {
    let cancelled = STYLE_PREVIEW.lock().unwrap_or_else(|error| error.into_inner()).take().is_some();
    if cancelled {
        bump_view();
    }
    cancelled
}

fn preview_slot() -> std::sync::MutexGuard<'static, Vec<FontPreview>> {
    PREVIEW.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn font_list(lib: &FontLibrary) -> Vec<String> {
    lib.families().into_iter().map(str::to_owned).collect()
}

pub fn appearance_preview_active(doc: &dtool_core::Document) -> bool {
    let style = STYLE_PREVIEW.lock().unwrap_or_else(|error| error.into_inner()).as_ref()
        .is_some_and(|preview| preview.document == doc.instance.0 && preview.version == doc.version);
    style || preview_slot().iter().any(|preview| preview.doc == doc.instance.0)
}

pub fn font_preview() -> Option<FontPreview> {
    preview_slot().first().cloned()
}

pub fn cancel_font_preview() -> bool {
    let had = {
        let mut slot = preview_slot();
        let had = !slot.is_empty();
        slot.clear();
        had
    };
    if had {
        bump_view();
    }
    had
}

fn bump_view() {
    dtool_core::text_lines::publish_font_generation(dtool_core::text_lines::font_generation() + 1);
}

pub fn set_font_preview(lib: &FontLibrary, doc: &dtool_core::Document, preview: FontPreview) -> Result<FontPreview, dtool_core::CommandError> {
    set_font_previews(lib, doc, vec![preview])
}

fn set_font_previews(lib: &FontLibrary, doc: &dtool_core::Document, previews: Vec<FontPreview>) -> Result<FontPreview, dtool_core::CommandError> {
    let mut stored = Vec::with_capacity(previews.len());
    let mut seen = std::collections::HashSet::new();
    for preview in previews {
        let story = doc.stories.iter().find(|s| s.id.0 == preview.story).ok_or_else(dtool_core::CommandError::not_found)?;
        let len = story.text.chars().count();
        if preview.start > preview.end || preview.end > len {
            return Err(dtool_core::CommandError::invalid_params());
        }
        let family = lib.families().into_iter().find(|f| f.eq_ignore_ascii_case(preview.family.trim()))
            .ok_or_else(dtool_core::CommandError::not_found)?.to_owned();
        if let Some(style) = preview.style.as_deref() {
            lib.resolve(&family, style).ok_or_else(dtool_core::CommandError::not_found)?;
        }
        if !seen.insert(preview.story) {
            return Err(dtool_core::CommandError::invalid_params());
        }
        stored.push(FontPreview { family, doc: doc.instance.0, ..preview });
    }
    let first = stored.first().cloned().ok_or_else(dtool_core::CommandError::invalid_params)?;
    let changed = {
        let mut slot = preview_slot();
        let changed = *slot != stored;
        *slot = stored;
        changed
    };
    if changed {
        bump_view();
    }
    Ok(first)
}


pub fn run_font_command(lib: &FontLibrary, doc: &dtool_core::Document, command: &dtool_core::Command) -> Option<Result<FontReply, dtool_core::CommandError>> {
    let params = &command.params;
    Some(match command.name.as_str() {
        FONT_LIST => Ok(FontReply::List { families: font_list(lib) }),
        FONT_PREVIEW => (|| {
            let invalid = dtool_core::CommandError::invalid_params;
            let family = params.get("family").and_then(|v| v.as_str()).ok_or_else(invalid)?;
            let style = params.get("style").and_then(|v| v.as_str()).map(str::to_owned);
            let targets = match params.get("targets") {
                Some(value) => value.as_array().ok_or_else(invalid)?.as_slice(),
                None => std::slice::from_ref(params),
            };
            let previews = targets.iter().map(|target| -> Result<_, dtool_core::CommandError> {
                let story = target.get("story").and_then(|v| v.as_u64()).ok_or_else(invalid)?;
                let range = target.get("range").ok_or_else(invalid)?;
                let pick = |key: &str, index: usize| range.get(key).or_else(|| range.get(index))
                    .and_then(|v| v.as_u64()).map(|v| v as usize).ok_or_else(invalid);
                Ok(FontPreview {
                    story, start: pick("start", 0)?, end: pick("end", 1)?,
                    family: family.to_owned(), style: style.clone(), doc: 0,
                })
            }).collect::<Result<Vec<_>, _>>()?;
            set_font_previews(lib, doc, previews).map(|preview| FontReply::Preview { preview: Some(preview) })
        })(),
        FONT_PREVIEW_CANCEL => {
            cancel_font_preview();
            Ok(FontReply::Preview { preview: None })
        }
        _ => return None,
    })
}

pub fn previewed_story(story: &Story) -> Story {
    let mut out = story.clone();
    let preview = preview_slot().iter().find(|p| p.story == story.id.0 && p.start < p.end).cloned();
    if let Some(preview) = preview {
        let patch = CharPatch { family: Some(preview.family), style: preview.style, ..Default::default() };
        out.patch_char(TextRange::new(preview.start, preview.end), &patch);
    }
    out
}
pub fn font_files(lib: &FontLibrary, params: &dtool_core::serde_json::Value) -> dtool_core::serde_json::Value {
    use dtool_core::serde_json::{json, Value};
    let faces = params.get("faces").and_then(Value::as_array).cloned().unwrap_or_default();
    let files: Vec<Value> = faces
        .iter()
        .filter_map(|face| {
            let family = face.get("family").and_then(Value::as_str)?;
            let style = face.get("style").and_then(Value::as_str).unwrap_or("");
            let ids: Vec<_> = match lib.resolve(family, style) {
                Some(id) if !style.is_empty() => vec![id],
                _ => lib.faces(family).into_iter().map(|face| face.id).collect(),
            };
            let mut paths: Vec<String> = Vec::new();
            for id in ids {
                if let Some(face) = lib.face(id) {
                    let path = face.path.to_string_lossy().into_owned();
                    if !path.is_empty() && !paths.contains(&path) {
                        paths.push(path);
                    }
                }
            }
            Some(json!({ "family": family, "style": style, "paths": paths }))
        })
        .collect();
    json!({ "files": files })
}
fn range_at(range: &dtool_core::serde_json::Value, key: &str) -> usize {
    range.get(key).and_then(|v| v.as_u64()).unwrap_or(0) as usize
}

fn preview_range(range: &dtool_core::serde_json::Value, start: usize, end: usize, preview: Option<&FontPreview>) -> dtool_core::serde_json::Value {
    let mut out = range.clone();
    if !out.is_object() {
        out = dtool_core::serde_json::Value::Object(Default::default());
    }
    out["start"] = (start as u64).into();
    out["end"] = (end as u64).into();
    if let Some(preview) = preview {
        out["family"] = preview.family.clone().into();
        if let Some(style) = &preview.style {
            out["style"] = style.clone().into();
        }
    }
    out
}

/// Core story with the active preview family applied; None when no preview targets it.
pub fn previewed_core_story(doc: &dtool_core::Document, core: &dtool_core::Story) -> Option<dtool_core::Story> {
    if let Some(story) = STYLE_PREVIEW.lock().unwrap_or_else(|error| error.into_inner()).as_ref()
        .filter(|preview| preview.document == doc.instance.0 && preview.version == doc.version)
        .and_then(|preview| preview.stories.iter().find(|story| story.id == core.id)).cloned()
    {
        return Some(story);
    }
    let preview = preview_slot().iter()
        .find(|p| p.doc == doc.instance.0 && p.story == core.id.0 && p.start < p.end).cloned()?;
    let (start, end) = (preview.start, preview.end);
    let mut ranges = Vec::new();
    let mut covered = Vec::new();
    for range in &core.ranges {
        let (rs, re) = (range_at(range, "start"), range_at(range, "end"));
        if range.get("frame").is_some() || rs >= re || re <= start || rs >= end {
            ranges.push(range.clone());
            continue;
        }
        if rs < start {
            ranges.push(preview_range(range, rs, start, None));
        }
        let (os, oe) = (rs.max(start), re.min(end));
        ranges.push(preview_range(range, os, oe, Some(&preview)));
        covered.push((os, oe));
        if re > end {
            ranges.push(preview_range(range, end, re, None));
        }
    }
    covered.sort_unstable();
    let blank = dtool_core::serde_json::Value::Object(Default::default());
    let mut at = start;
    for (a, b) in covered {
        if a > at {
            ranges.push(preview_range(&blank, at, a, Some(&preview)));
        }
        at = at.max(b);
    }
    if at < end {
        ranges.push(preview_range(&blank, at, end, Some(&preview)));
    }
    let mut out = core.clone();
    out.ranges = ranges;
    Some(out)
}

/// Display-only copy of `doc` with the preview applied to `story`; the live document is never touched.
pub fn preview_document(doc: &dtool_core::Document, story: dtool_core::StoryId) -> Option<dtool_core::Document> {
    let index = doc.stories.iter().position(|s| s.id == story)?;
    let patched = previewed_core_story(doc, &doc.stories[index])?;
    let mut view = doc.clone();
    view.stories[index] = patched;
    Some(view)
}
