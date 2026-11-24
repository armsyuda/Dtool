use crate::para_style::{BreakKind, Special};
use crate::story::{CharStyle, ParaStyle, Run, Story, StoryId, StyleLevel, StyleRef, TextRange};
use serde::Deserialize;
use dtool_core::serde_json;

pub const CHAR_STYLE_KEY: &str = "char_style";
pub const PARA_STYLE_KEY: &str = "para_style";
pub const STYLE_OVERRIDES_KEY: &str = "style_overrides";

fn applied_styles_from_core(core: &dtool_core::Story) -> Vec<Run<StyleRef>> {
    let mut styles = Vec::new();
    for (list, key, level) in [(&core.ranges, CHAR_STYLE_KEY, StyleLevel::Character), (&core.paras, PARA_STYLE_KEY, StyleLevel::Paragraph)] {
        for range in list {
            let start = range.get("start").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let end = range.get("end").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let Some(name) = range.get(key).and_then(|v| v.as_str()) else {
                continue;
            };
            if start >= end || range.get("frame").is_some() {
                continue;
            }
            let overrides = range
                .get(STYLE_OVERRIDES_KEY)
                .and_then(|v| v.as_object())
                .map(|map| map.iter().map(|(k, v)| (k.clone(), v.to_string())).collect())
                .unwrap_or_default();
            styles.push(Run { range: TextRange::new(start, end), style: StyleRef { level, name: Some(name.to_string()), overrides } });
        }
    }
    styles.sort_by_key(|run| (run.range.start, run.style.level == StyleLevel::Paragraph));
    styles
}

pub fn story_from_core(core: &dtool_core::Story) -> Story {
    let mut chars = Vec::new();
    for range in &core.ranges {
        if range.get("frame").is_some() {
            continue;
        }
        let start = range.get("start").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let end = range.get("end").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        if start >= end {
            continue;
        }
        let mut style = CharStyle::default();
        if let Some(size) = range.get("size_pt").and_then(|v| v.as_f64()) {
            if size.is_finite() && size > 0.0 {
                style.size_pt = size as f32;
            }
        }
        if let Some(family) = range.get("family").and_then(|v| v.as_str()) {
            if !family.is_empty() {
                style.family = family.to_string();
            }
        }
        if let Some(face) = range.get("style").and_then(|v| v.as_str()) {
            style.style = face.to_string();
        }
        let mut value=range.clone();
        if let Some(fields)=value.as_object_mut() {
            fields.entry("family".to_owned()).or_insert_with(||style.family.clone().into());
            fields.entry("style".to_owned()).or_insert_with(||style.style.clone().into());
            fields.entry("size_pt".to_owned()).or_insert_with(||f64::from(style.size_pt).into());
        }
        if let Ok(typed)=CharStyle::deserialize(value) {style=typed;}
        if style.family.trim().is_empty() {
            style.family = "sans-serif".to_owned();
        }
        chars.push(Run {
            range: TextRange::new(start, end),
            style,
        });
    }
    let mut paras = Vec::new();
    let empty_paragraph = (core.text.is_empty() || core.text.ends_with(['\n', '\u{2029}']))
        .then(|| core.text.chars().count());
    for range in &core.paras {
        let start = range.get("start").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let end = range.get("end").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        if start >= end && !(start == end && empty_paragraph == Some(start)) {
            continue;
        }
        paras.push(Run {
            range: TextRange::new(start, end),
            style: para_style_from_value(range),
        });
    }
    let specials = core
        .specials
        .iter()
        .filter_map(|(pos, mark)| Some((*pos, map_special(mark))))
        .collect();
    Story {
        id: StoryId(core.id.0),
        text: core.text.clone(),
        chars,
        paras,
        specials,
        styles: applied_styles_from_core(core),
        layout_units_per_point: None,
    }
}

pub(crate) fn story_with_substitutions(core: &dtool_core::Story, doc: &dtool_core::Document) -> Story {
    let mut story = story_from_core(core);
    apply_initial_words(&mut story, &doc.text_styles);
    if doc.font_substitutions.is_empty() {
        return story;
    }

    let library = crate::FontLibrary::shared();
    for run in &mut story.chars {
        let style = &mut run.style;
        let Some(rule) = doc.font_substitutions.iter().find(|entry| entry.source.matches(&style.family, &style.style)) else {
            continue;
        };
        if library.resolve(&style.family, &style.style).is_some() {
            continue;
        }
        if let Some(face) = library.resolve(&rule.substitute.family, &rule.substitute.style).and_then(|id| library.face(id)) {
            style.family.clone_from(&face.family);
            style.style.clone_from(&face.style);
            style.weight = face.weight;
            style.italic = face.italic;
        }
    }
    story
}

fn apply_initial_words(story: &mut Story, store: &dtool_core::text_styles::TextStyleStore) {
    use dtool_core::text_styles::StyleKind;

    if !story.paras.iter().any(|run| run.style.initial_words.on && run.style.initial_words.style.is_some()) {
        return;
    }
    let mut overlays = Vec::new();
    let mut paragraph_start = 0;
    for paragraph in story.text.split(['\n', '\u{2029}']) {
        let start = paragraph_start;
        paragraph_start += paragraph.chars().count() + 1;
        let Some(options) = crate::layout::para_run_at(story, start).map(|style| &style.initial_words)
            .filter(|options| options.on) else { continue };
        let Some(name) = options.style.as_deref() else { continue };
        let Ok(properties) = store.resolve(StyleKind::Character, name) else { continue };
        let mut words = 0;
        let mut in_word = false;
        let mut first = None;
        let mut end = start;
        for (index, ch) in paragraph.chars().enumerate() {
            if ch.is_whitespace() {
                if words >= options.max_words {
                    break;
                }
                in_word = false;
                continue;
            }
            if !in_word {
                words += 1;
                if words > options.max_words {
                    break;
                }
                first.get_or_insert(start + index);
                in_word = true;
            }
            end = start + index + 1;
            if options.end_chars.contains(ch) {
                break;
            }
        }
        let Some(start) = first else { continue };
        let mut points = vec![start, end];
        points.extend(story.chars.iter().flat_map(|run| [run.range.start, run.range.end])
            .filter(|&point| point > start && point < end));
        points.sort_unstable();
        points.dedup();
        for pair in points.windows(2) {
            let style = story.chars.iter().rev().find(|run| run.range.start <= pair[0] && pair[0] < run.range.end)
                .map(|run| run.style.clone()).unwrap_or_default();
            let Ok(serde_json::Value::Object(mut merged)) = serde_json::to_value(style) else { continue };
            merged.extend(properties.clone());
            let Ok(mut style) = CharStyle::deserialize(serde_json::Value::Object(merged)) else { continue };
            if style.family.trim().is_empty() {
                style.family = "sans-serif".to_owned();
            }
            overlays.push(Run { range: TextRange::new(pair[0], pair[1]), style });
        }
    }
    story.chars.extend(overlays);
}

/// Project point-based text metrics into the coordinate space used by document layout.
pub fn story_in_document(core: &dtool_core::Story, doc: &dtool_core::Document) -> Story {
    let mut story = story_with_substitutions(core, doc);
    let scale = dtool_core::units::units_per_point(doc.units, doc.dpi);
    if scale == 1.0 { return story; }
    story.layout_units_per_point = Some(scale);
    let len = story.text.chars().count();
    if len > 0 && !story.chars.iter().any(|run| run.range.start == 0 && run.range.end >= len) {
        story.chars.insert(0, Run {
            range: TextRange::new(0, len),
            style: CharStyle { family: "sans-serif".to_owned(), ..Default::default() },
        });
    }
    for run in &mut story.chars {
        let style = &mut run.style;
        style.size_pt *= scale;
        style.baseline_pt *= scale;
        style.leading = style.leading.map(|value| value * scale);
        for decoration in [&mut style.underline, &mut style.strike] {
            decoration.weight = decoration.weight.map(|value| value * scale);
            decoration.offset = decoration.offset.map(|value| value * scale);
        }
        if let Some((_, width)) = &mut style.outline { *width *= scale; }
    }
    story
}

fn para_style_from_value(range: &dtool_core::serde_json::Value) -> ParaStyle {
    if let Ok(style) = ParaStyle::deserialize(range.clone()) {
        return style;
    }
    let mut kept = range.clone();
    if let Some(map) = kept.as_object_mut() {
        let keys: Vec<String> = map.keys().cloned().collect();
        for key in keys {
            let Some(value) = map.get(&key).cloned() else { continue };
            let value = match (key.as_str(), value) {
                ("tabs", dtool_core::serde_json::Value::Array(items)) => dtool_core::serde_json::Value::Array(
                    items.into_iter().filter(|t| crate::para_style::TabStop::deserialize(t.clone()).is_ok()).collect(),
                ),
                (_, other) => other,
            };
            let mut single = dtool_core::serde_json::Map::new();
            single.insert(key.clone(), value.clone());
            if ParaStyle::deserialize(dtool_core::serde_json::Value::Object(single)).is_ok() {
                map.insert(key, value);
            } else {
                map.remove(&key);
            }
        }
    }
    ParaStyle::deserialize(kept).unwrap_or_default()
}

fn map_special(mark: &dtool_core::document::Special) -> Special {
    use dtool_core::document::{BreakKind as CoreBreak, Special as CoreSpecial};
    match mark {
        CoreSpecial::RightIndentTab => Special::RightIndentTab,
        CoreSpecial::IndentToHere => Special::IndentToHere,
        CoreSpecial::Break(CoreBreak::Line) => Special::Break(BreakKind::Line),
        CoreSpecial::Break(CoreBreak::Column) => Special::Break(BreakKind::Column),
        CoreSpecial::Break(CoreBreak::Frame) => Special::Break(BreakKind::Frame),
        CoreSpecial::Break(CoreBreak::Page) => Special::Break(BreakKind::Page),
        CoreSpecial::Break(CoreBreak::OddPage) => Special::Break(BreakKind::OddPage),
        CoreSpecial::Break(CoreBreak::EvenPage) => Special::Break(BreakKind::EvenPage),
    }
}

pub fn resolved_frame_props(object: &dtool_core::Object) -> Option<dtool_core::FrameProps> {
    object.frame_props()
}
