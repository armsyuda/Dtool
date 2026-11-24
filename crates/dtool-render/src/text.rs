use dtool_core::object::ObjectKind;
use dtool_core::document::{Document, StoryId};
use dtool_text::layout_story;
use dtool_text::path_text::layout_path_text;
use dtool_text::Story;

pub fn render_object_text(doc: &Document, kind: &ObjectKind) -> Vec<serde_json::Value> {
    let mut output = Vec::new();
    match kind {
        ObjectKind::Shape => {}
        ObjectKind::ArtText { story } => {
            if let Some(s) = find_story(doc, *story) {
                let lines = layout_story(None, s, None);
                for line in &lines {
                    output.push(serde_json::json!({
                        "type": "line",
                        "range_start": line.range.start,
                        "range_end": line.range.end,
                        "baseline": line.baseline,
                        "width": line.width,
                        "glyph_count": line.glyphs.len(),
                        "underline": build_underline_info(s, &line),
                        "strike": build_strike_info(s, &line),
                        "background": build_background_info(s, &line)
                    }));
                }
            }
        }
        ObjectKind::FrameText { story, frame } => {
            if let Some(s) = find_story(doc, *story) {
                let width = frame.width;
                let lines = layout_story(None, s, Some(width));
                for line in &lines {
                    output.push(serde_json::json!({
                        "type": "line",
                        "range_start": line.range.start,
                        "range_end": line.range.end,
                        "baseline": line.baseline,
                        "width": line.width,
                        "glyph_count": line.glyphs.len(),
                        "underline": build_underline_info(s, &line),
                        "strike": build_strike_info(s, &line),
                        "background": build_background_info(s, &line)
                    }));
                }
            }
        }
        ObjectKind::PathText(data) => {
            if let Some(s) = find_story(doc, data.story) {
                if let Some(path_obj) = doc.objects.iter().find(|o| o.id == data.path) {
                    let layout = layout_path_text(None, &data, s, path_obj);
                    output.push(serde_json::json!({
                        "type": "path_text_layout",
                        "bounds": [layout.bounds[0], layout.bounds[1], layout.bounds[2], layout.bounds[3]],
                        "glyph_count": layout.glyphs.len(),
                        "source_paint": data.source_paint,
                        "glyphs": layout.glyphs.iter().map(|g| {
                            serde_json::json!({
                                "glyph_id": g.glyph.glyph_id,
                                "position_x": g.position[0],
                                "position_y": g.position[1],
                                "angle": g.angle,
                                "flipped": g.flipped,
                                "cluster_start": g.cluster.start,
                                "cluster_end": g.cluster.end
                            })
                        }).collect::<Vec<_>>()
                    }));
                }
            }
        }
    }
    output
}

fn find_story(doc: &Document, id: StoryId) -> Option<&Story> {
    doc.stories.iter().find(|s| s.id == id)
}

fn build_underline_info(story: &Story, line: &dtool_text::layout::LayoutLine) -> serde_json::Value {
    let start = line.range.start;
    let end = line.range.end;
    let mut segments: Vec<serde_json::Value> = Vec::new();

    for run in &story.runs {
        if run.range.end <= start || run.range.start >= end {
            continue;
        }
        if !run.style.underline.on {
            continue;
        }
        let seg_start = run.range.start.max(start);
        let seg_end = run.range.end.min(end);
        segments.push(serde_json::json!({
            "range_start": seg_start,
            "range_end": seg_end,
            "colour": run.style.underline.colour.clone(),
            "weight": run.style.underline.weight,
            "offset": run.style.underline.offset
        }));
    }

    serde_json::json!({
        "segments": segments
    })
}

fn build_strike_info(story: &Story, line: &dtool_text::layout::LayoutLine) -> serde_json::Value {
    let start = line.range.start;
    let end = line.range.end;
    let mut segments: Vec<serde_json::Value> = Vec::new();

    for run in &story.runs {
        if run.range.end <= start || run.range.start >= end {
            continue;
        }
        if !run.style.strike.on {
            continue;
        }
        let seg_start = run.range.start.max(start);
        let seg_end = run.range.end.min(end);
        segments.push(serde_json::json!({
            "range_start": seg_start,
            "range_end": seg_end,
            "colour": run.style.strike.colour.clone(),
            "weight": run.style.strike.weight,
            "offset": run.style.strike.offset
        }));
    }

    serde_json::json!({
        "segments": segments
    })
}

fn build_background_info(story: &Story, line: &dtool_text::layout::LayoutLine) -> serde_json::Value {
    let start = line.range.start;
    let end = line.range.end;
    let mut segments: Vec<serde_json::Value> = Vec::new();

    for run in &story.runs {
        if run.range.end <= start || run.range.start >= end {
            continue;
        }
        if let Some(ref colour) = run.style.background {
            let seg_start = run.range.start.max(start);
            let seg_end = run.range.end.min(end);
            segments.push(serde_json::json!({
                "range_start": seg_start,
                "range_end": seg_end,
                "colour": colour.clone()
            }));
        }
    }

    serde_json::json!({
        "segments": segments
    })
}
