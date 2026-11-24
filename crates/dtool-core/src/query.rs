use dtool_protocol::{CommandError, ChangeName};
use serde_json::json;
use crate::object::ObjectKind;
use crate::document::Document;
use crate::table::query::table_query;

pub type TableOverflowProvider = fn(&Document, crate::object::ObjectId) -> Option<std::collections::HashMap<crate::table::model::CellId, bool>>;

static TABLE_OVERFLOW: std::sync::OnceLock<TableOverflowProvider> = std::sync::OnceLock::new();

pub fn register_table_overflow(provider: TableOverflowProvider) -> bool {
    TABLE_OVERFLOW.set(provider).is_ok() || TABLE_OVERFLOW.get().is_some_and(|current| *current as usize == provider as usize)
}

pub fn table_query_live(doc: &Document, id: crate::object::ObjectId, cell: Option<crate::table::model::CellId>) -> serde_json::Value {
    let mut value = table_query(doc, id, cell);
    if let Some(map) = TABLE_OVERFLOW.get().and_then(|provider| provider(doc, id)) {
        crate::table::query::attach_layout_overflow(&mut value, &map);
    }
    if let Some(lines) = crate::table::query::table_lines(doc, id) {
        crate::table::query::attach_layout_lines(&mut value, &lines);
    }
    value
}

pub fn embedded_tables(document: &Document) -> serde_json::Value {
    serde_json::Value::Array(
        document
            .objects
            .iter()
            .filter(|object| matches!(object.kind, ObjectKind::Table(_)))
            .map(|object| json!({ "id": object.id.0, "table": table_query_live(document, object.id, None) }))
            .collect(),
    )
}

pub fn query_snapshot(version: u64, document: &Document) -> Result<serde_json::Value, CommandError> {
    Ok(json!({
        "doc_version": version,
        "snapshot": crate::commands::doc_setup::snapshot(document)?,
        "tables": embedded_tables(document)
    }))
}

#[derive(Debug, Clone, Default)]
pub struct ChangeLog {
    base: Option<u64>,
    entries: std::collections::VecDeque<(u64, String)>,
}

impl ChangeLog {
    pub const CAPACITY: usize = 512;

    pub fn start(&mut self, generation: u64) {
        if self.base.is_none() {
            self.base = Some(generation);
        }
    }

    pub fn record(&mut self, generation: u64, name: &str) {
        self.start(generation.saturating_sub(1));
        self.entries.push_back((generation, name.to_string()));
        while self.entries.len() > Self::CAPACITY {
            if let Some((dropped, _)) = self.entries.pop_front() {
                self.base = Some(dropped);
            }
        }
    }

    pub fn is_kept(&self, since: u64, current: u64) -> bool {
        if since > current {
            return false;
        }
        if since == current {
            return true;
        }
        match self.base {
            Some(base) => since >= base,
            None => false,
        }
    }

    pub fn changes_since(&self, since: u64) -> Vec<ChangeName> {
        self.entries.iter().filter(|(generation, _)| *generation > since).map(|(_, name)| ChangeName { name: name.clone() }).collect()
    }
}

pub fn query_delta(version: u64, changes: Vec<ChangeName>) -> serde_json::Value {
    json!({
        "doc_version": version,
        "changes": changes
    })
}

pub fn invalid_params_error() -> CommandError {
    CommandError {
        code: "INVALID_PARAMS".to_string(),
        message_key: "error.invalid_params".to_string(),
    }
}

pub fn describe_object_kind(kind: &ObjectKind) -> serde_json::Value {
    match kind {
        ObjectKind::Shape => json!({"type": "shape"}),
        ObjectKind::Table(_) => json!({"type": "table"}),
        ObjectKind::Group => json!({"type": "group"}),
        ObjectKind::Artboard => json!({"type": "artboard"}),
        ObjectKind::Image => json!({"type": "image"}),
        ObjectKind::PictureFrame => json!({"type": "picture_frame"}),
        ObjectKind::Compound(spec) => json!({"type": "compound", "op": spec.op, "operands": spec.operands.len()}),
        ObjectKind::Text(text) => json!({"type": "text", "text_length": text.chars().count()}),
        ObjectKind::ArtText { story } => json!({"type": "art_text", "story_id": story.0}),
        ObjectKind::FrameText { story } => json!({
            "type": "frame_text",
            "story_id": story.0,
            "frame_props": serde_json::Value::Null
        }),
        ObjectKind::PathText(data) => json!({
            "type": "path_text",
            "path_id": data.path.0,
            "story_id": data.story.0,
            "flows_count": data.flows.len(),
            "source_paint": data.source_paint
        }),
        ObjectKind::Curve(curve) => json!({
            "type": "curve",
            "points": curve.contour.points.len(),
            "holes": curve.holes.len(),
            "closed": curve.contour.closed,
            "source_object": curve.source_object
        }),
        ObjectKind::Dimension(d) => json!({
            "type": "dimension",
            "length": d.length,
            "units": d.units,
            "offset": d.offset
        }),
    }
}

pub fn hit_test_object(kind: &ObjectKind) -> bool {
    match kind {
        ObjectKind::Shape | ObjectKind::Table(_) | ObjectKind::Group | ObjectKind::Artboard | ObjectKind::Compound(_) | ObjectKind::Image | ObjectKind::PictureFrame | ObjectKind::Text(_) => true,
        ObjectKind::ArtText { .. } => true,
        ObjectKind::FrameText { .. } => true,
        ObjectKind::PathText(data) => !data.flows.is_empty(),
        ObjectKind::Curve(_) => true,
        ObjectKind::Dimension(_) => true,
    }
}

pub fn list_layers_object(kind: &ObjectKind) -> serde_json::Value {
    match kind {
        ObjectKind::Shape => json!({"layer_type": "shape", "editable": true}),
        ObjectKind::Table(_) => json!({"layer_type": "table", "editable": true}),
        ObjectKind::Group => json!({"layer_type": "group", "editable": true}),
        ObjectKind::Artboard => json!({"layer_type": "artboard", "editable": true}),
        ObjectKind::Image => json!({"layer_type": "image", "editable": true}),
        ObjectKind::PictureFrame => json!({"layer_type": "picture_frame", "editable": true}),
        ObjectKind::Compound(spec) => json!({"layer_type": "compound", "op": spec.op, "operands": spec.operands.iter().map(|o| o.id.0).collect::<Vec<_>>(), "editable": true}),
        ObjectKind::Text(text) => json!({"layer_type": "text", "text_length": text.chars().count(), "editable": true}),
        ObjectKind::ArtText { story } => json!({"layer_type": "art_text", "story_id": story.0, "editable": true}),
        ObjectKind::FrameText { story } => json!({
            "layer_type": "frame_text",
            "story_id": story.0,
            "frame_props": serde_json::Value::Null,
            "editable": true
        }),
        ObjectKind::PathText(data) => json!({
            "layer_type": "path_text",
            "path_id": data.path.0,
            "story_id": data.story.0,
            "flows_count": data.flows.len(),
            "editable": true
        }),
        ObjectKind::Curve(curve) => json!({
            "layer_type": "curve",
            "points": curve.contour.points.len(),
            "holes": curve.holes.len(),
            "editable": true,
            "source_object": curve.source_object
        }),
        ObjectKind::Dimension(d) => json!({
            "layer_type": "dimension",
            "length": d.length,
            "units": d.units,
            "editable": true
        }),
    }
}

pub fn preflight_object(kind: &ObjectKind) -> Vec<String> {
    let mut warnings = Vec::new();
    match kind {
        ObjectKind::Shape | ObjectKind::Table(_) | ObjectKind::Group | ObjectKind::Artboard | ObjectKind::Compound(_) | ObjectKind::Image | ObjectKind::PictureFrame | ObjectKind::Text(_) => {}
        ObjectKind::ArtText { story } => {
            if story.0 == 0 {
                warnings.push("empty_story".to_string());
            }
        }
        ObjectKind::FrameText { story } => {
            if story.0 == 0 {
                warnings.push("empty_story".to_string());
            }
        }
        ObjectKind::PathText(data) => {
            if data.flows.is_empty() {
                warnings.push("no_flows".to_string());
            }
            if data.story.0 == 0 {
                warnings.push("empty_story".to_string());
            }
            if data.path.0 == 0 {
                warnings.push("invalid_path".to_string());
            }
        }
        ObjectKind::Curve(curve) => {
            if curve.is_empty() {
                warnings.push("empty_curve".to_string());
            }
        }
        ObjectKind::Dimension(_) => {}
    }
    warnings
}

pub fn query_doc_object(doc: &Document, id_str: &str) -> Result<serde_json::Value, CommandError> {
    let id = crate::object::ObjectId(id_str.strip_prefix("node:").unwrap_or(id_str).parse::<u64>().map_err(|_| invalid_params_error())?);

    let obj = doc.objects.iter().find(|o| o.id == id)
        .ok_or_else(|| CommandError::not_found())?;

    match &obj.kind {
        ObjectKind::PathText(data) => {
            let story = doc.stories.iter().find(|s| s.id == data.story);
            let has_story = story.is_some();
            let text_len = story.map(|s| s.text.chars().count()).unwrap_or(0);

            let (overset, overset_known) = overset_json(doc, id);

            Ok(json!({
                "type": "path_text",
                "path_id": data.path.0,
                "story_id": data.story.0,
                "flows_count": data.flows.len(),
                "source_paint": data.source_paint,
                "has_story": has_story,
                "text_length": text_len,
                "overset_possible": !overset.is_null(),
                "overset": overset,
                "overset_known": overset_known
            }))
        }
        ObjectKind::ArtText { story: story_id } => {
            let story = doc.stories.iter().find(|s| s.id == *story_id);
            let text_len = story.map(|s| s.text.chars().count()).unwrap_or(0);

            Ok(json!({
                "type": "art_text",
                "vertical": obj.is_vertical_art_text(),
                "story_id": story_id.0,
                "text_length": text_len,
                "overset_possible": false
            }))
        }
        ObjectKind::FrameText { story: story_id } => {
            let story = doc.stories.iter().find(|s| s.id == *story_id);
            let text_len = story.map(|s| s.text.chars().count()).unwrap_or(0);
            let (overset, overset_known) = overset_json(doc, id);

            Ok(json!({
                "type": "frame_text",
                "story_id": story_id.0,
                "text_length": text_len,
                "overset_possible": !overset.is_null(),
                "overset": overset,
                "overset_known": overset_known,
                "frame_props": serde_json::to_value(obj.frame_props()).unwrap_or(serde_json::Value::Null)
            }))
        }
        ObjectKind::Table(_) => {
            let table_data = table_query_live(doc, id, None);
            Ok(json!({
                "type": "table",
                "table": table_data
            }))
        }
        ObjectKind::Shape | ObjectKind::Group | ObjectKind::Artboard | ObjectKind::Compound(_) | ObjectKind::Image | ObjectKind::PictureFrame | ObjectKind::Text(_) | ObjectKind::Curve(_) | ObjectKind::Dimension(_) => Ok(describe_object_kind(&obj.kind)),
    }
}

pub(crate) fn overset_json(doc: &Document, id: crate::object::ObjectId) -> (serde_json::Value, bool) {
    match crate::text_lines::overset(doc, id) {
        Some(Some((start, end))) => (json!({"start": start, "end": end}), true),
        Some(None) => (serde_json::Value::Null, true),
        None => (serde_json::Value::Null, false),
    }
}

pub fn query_overset(doc: &Document, id_str: &str) -> Result<Option<serde_json::Value>, CommandError> {
    let id = crate::object::ObjectId(id_str.strip_prefix("node:").unwrap_or(id_str).parse::<u64>().map_err(|_| invalid_params_error())?);

    let obj = doc.objects.iter().find(|o| o.id == id)
        .ok_or_else(|| CommandError::not_found())?;

    let Some(story_id) = obj.story_id() else {
        return Ok(None);
    };
    let text_len = doc.stories.iter().find(|s| s.id == story_id)
        .map(|s| s.text.chars().count())
        .ok_or_else(|| CommandError::not_found())?;
    let (overset, known) = overset_json(doc, id);

    Ok(Some(json!({
        "overset": overset,
        "overset_known": known,
        "story_id": story_id.0,
        "text_length": text_len
    })))
}
