use serde::{Deserialize, Serialize};
use crate::document::Document;
use crate::object::ObjectId;
use crate::CommandError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SliceId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OutputId(pub u64);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SliceSource {
    Manual,
    Linked(ObjectId),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OutputSize {
    Scale(f32),
    Width(u32),
    Height(u32),
    Square(u32),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SliceOutput {
    pub id: OutputId,
    pub enabled: bool,
    pub format: String,
    pub size: OutputSize,
    pub suffix: String,
    pub path: Option<String>,
    pub options: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Slice {
    pub id: SliceId,
    pub name: String,
    pub source: SliceSource,
    pub bounds: [f32; 4],
    pub auto_bounds: bool,
    pub enabled: bool,
    pub export_selected: bool,
    pub path: String,
    pub outputs: Vec<SliceOutput>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PathProblem {
    Duplicate,
    Prohibited,
    Reserved,
    DotsOrSpaces,
    EmptySegment,
    BadPath,
    BadRoot,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SliceTarget {
    SelectedOutput(SliceId, OutputId),
    SelectedSlices,
    AllEnabled,
}

pub const SLICE_TOKENS: &[&str] = &[
    "document_name",
    "document_file_name",
    "slice_name",
    "scale",
    "scale_suffix",
    "width",
    "height",
    "format",
];

fn snap_to_pixel(v: f32) -> i32 {
    v.round() as i32
}

pub fn create_manual_slice(bounds: [f32; 4], name: String, id: SliceId) -> Slice {
    let x0 = snap_to_pixel(bounds[0]);
    let y0 = snap_to_pixel(bounds[1]);
    let x1 = snap_to_pixel(bounds[2]);
    let y1 = snap_to_pixel(bounds[3]);

    let min_x = x0.min(x1);
    let max_x = x0.max(x1);
    let min_y = y0.min(y1);
    let max_y = y0.max(y1);

    Slice {
        id,
        name,
        source: SliceSource::Manual,
        bounds: [min_x as f32, min_y as f32, max_x as f32, max_y as f32],
        auto_bounds: false,
        enabled: true,
        export_selected: false,
        path: String::new(),
        outputs: Vec::new(),
    }
}

pub fn create_linked_slice(object_id: ObjectId, name: String, id: SliceId) -> Slice {
    Slice {
        id,
        name,
        source: SliceSource::Linked(object_id),
        bounds: [0.0, 0.0, 0.0, 0.0],
        auto_bounds: true,
        enabled: true,
        export_selected: false,
        path: String::new(),
        outputs: Vec::new(),
    }
}

pub fn expand_path(template: &str, slice: &Slice, out: &SliceOutput, doc: &Document) -> Result<String, CommandError> {
    expand_path_with_context(template, slice, out, doc, None, None)
}

pub fn expand_path_with_context(template: &str, slice: &Slice, out: &SliceOutput, doc: &Document, document_name: Option<&str>, document_file_name: Option<&str>) -> Result<String, CommandError> {
    let mut result = template.to_string();
    let dimensions = out.dimensions(slice, doc)?;

    for token in SLICE_TOKENS {
        let token_pattern = format!("{{{}}}", token);
        if !result.contains(&token_pattern) { continue; }
        let name = |value: &str| value.replace(['/', '\\'], "_");
        let replacement = match *token {
            "document_name" => name(document_name.ok_or_else(CommandError::invalid_params)?),
            "document_file_name" => name(document_file_name.ok_or_else(CommandError::invalid_params)?),
            "slice_name" => name(&slice.name),
            "scale" => match out.size {
                OutputSize::Scale(s) => s.to_string(),
                _ => "1".to_string(),
            },
            "scale_suffix" => match out.size {
                OutputSize::Scale(s) if s != 1.0 => format!("@{}x", s),
                _ => String::new(),
            },
            "width" => dimensions[0].to_string(),
            "height" => dimensions[1].to_string(),
            "format" => out.format.clone(),
            _ => String::new(),
        };

        result = result.replace(&token_pattern, &replacement);
    }

    let extension = format!(".{}", out.format);
    if !result.to_ascii_lowercase().ends_with(&extension.to_ascii_lowercase()) {
        if !result.is_empty() && !result.ends_with('.') {
            result.push('.');
        }
        result.push_str(&out.format);
    }

    Ok(result)
}

pub fn check_paths(doc: &Document, targets: &[(SliceId, OutputId)]) -> Vec<(SliceId, OutputId, PathProblem)> {
    let mut problems = Vec::new();
    let mut seen_paths: std::collections::HashMap<String, (SliceId, OutputId)> = std::collections::HashMap::new();

    for &(slice_id, output_id) in targets {
        if let Some(slice) = doc.slices.iter().find(|s| s.id == slice_id) {
            if let Some(out) = slice.outputs.iter().find(|o| o.id == output_id) {
                let path_str = out.path.clone().unwrap_or_else(|| slice.path.clone());

                if path_str.is_empty() {
                    problems.push((slice_id, output_id, PathProblem::EmptySegment));
                    continue;
                }

                let normalized = path_str.to_lowercase();

                if normalized.contains("..") || normalized.starts_with('/') || normalized.starts_with('\\') {
                    problems.push((slice_id, output_id, PathProblem::BadPath));
                    continue;
                }

                let segments: Vec<&str> = normalized.split('/').collect();
                
                for seg in &segments {
                    if seg.is_empty() {
                        problems.push((slice_id, output_id, PathProblem::EmptySegment));
                        break;
                    }
                    
                    if seg.starts_with('.') || seg.ends_with('.') || seg.contains(' ') {
                        problems.push((slice_id, output_id, PathProblem::DotsOrSpaces));
                        break;
                    }

                    let reserved_names = ["con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9"];
                    if reserved_names.contains(&seg) {
                        problems.push((slice_id, output_id, PathProblem::Reserved));
                        break;
                    }

                    let prohibited_chars = ['<', '>', ':', '"', '|', '?', '*'];
                    if seg.chars().any(|c| prohibited_chars.contains(&c)) {
                        problems.push((slice_id, output_id, PathProblem::Prohibited));
                        break;
                    }
                }

                if let Some(existing) = seen_paths.get(&normalized) {
                    problems.push((slice_id, output_id, PathProblem::Duplicate));
                } else {
                    seen_paths.insert(normalized.clone(), (slice_id, output_id));
                }
            }
        }
    }

    problems
}

pub fn source_bounds(doc: &Document, id: ObjectId) -> Result<[f32; 4], CommandError> {
    let object = doc.get_object(id).ok_or_else(CommandError::not_found)?;
    let [x, y, w, h] = crate::geometry::object_world_bounds(object);
    let bounds = [x, y, x + w, y + h];
    if bounds.iter().any(|value| !value.is_finite()) { return Err(CommandError::invalid_params()); }
    Ok(bounds)
}

impl Slice {
    pub fn resolved_bounds(&self, doc: &Document) -> Result<[f32; 4], CommandError> {
        match self.source {
            SliceSource::Linked(id) if self.auto_bounds => source_bounds(doc, id),
            _ => Ok(self.bounds),
        }
    }
}

pub fn refresh_auto_bounds(doc: &mut Document) {
    let updates: Vec<_> = doc.slices.iter().enumerate().filter_map(|(index, slice)| {
        if slice.auto_bounds { slice.resolved_bounds(doc).ok().map(|bounds| (index, bounds)) } else { None }
    }).collect();
    for (index, bounds) in updates { doc.slices[index].bounds = bounds; }
}

impl SliceOutput {
    pub fn dimensions(&self, slice: &Slice, doc: &Document) -> Result<[u32; 2], CommandError> {
        let bounds = slice.resolved_bounds(doc)?;
        let px = f64::from(crate::doc_setup::unit_factor(doc.units, doc.dpi, crate::document::Units::Px, doc.dpi));
        let width = (f64::from(bounds[2]) - f64::from(bounds[0])) * px;
        let height = (f64::from(bounds[3]) - f64::from(bounds[1])) * px;
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 { return Err(CommandError::invalid_params()); }
        let (w, h) = match self.size {
            OutputSize::Scale(scale) if scale.is_finite() && scale > 0.0 => (width * f64::from(scale), height * f64::from(scale)),
            OutputSize::Width(value) if value > 0 => (f64::from(value), height * f64::from(value) / width),
            OutputSize::Height(value) if value > 0 => (width * f64::from(value) / height, f64::from(value)),
            OutputSize::Square(value) if value > 0 => (f64::from(value), f64::from(value)),
            _ => return Err(CommandError::invalid_params()),
        };
        if !w.is_finite() || !h.is_finite() || w > f64::from(u32::MAX) || h > f64::from(u32::MAX) { return Err(CommandError::invalid_params()); }
        Ok([w.round().max(1.0) as u32, h.round().max(1.0) as u32])
    }
}


pub fn updated_slices(doc: &Document, patch: &serde_json::Value) -> Result<Vec<Slice>, CommandError> {
    use serde_json::Value;
    let fields = patch.as_object().ok_or_else(CommandError::invalid_params)?;
    if fields.keys().any(|key| !matches!(key.as_str(), "id" | "name" | "enabled" | "export_selected" | "path" | "order" | "outputs" | "bounds" | "rect")) || (fields.contains_key("bounds") && fields.contains_key("rect")) {
        return Err(CommandError::invalid_params());
    }
    let id = fields.get("id").and_then(Value::as_u64).map(SliceId).ok_or_else(CommandError::invalid_params)?;
    let index = doc.slices.iter().position(|slice| slice.id == id).ok_or_else(CommandError::not_found)?;
    let mut slices = doc.slices.clone();
    let slice = &mut slices[index];
    for (key, value) in fields {
        match key.as_str() {
            "name" => {
                let name = value.as_str().filter(|name| !name.trim().is_empty() && !name.chars().any(char::is_control)).ok_or_else(CommandError::invalid_params)?;
                slice.name = name.into();
            },
            "enabled" => slice.enabled = value.as_bool().ok_or_else(CommandError::invalid_params)?,
            "export_selected" => slice.export_selected = value.as_bool().ok_or_else(CommandError::invalid_params)?,
            "path" => slice.path = value.as_str().filter(|path| !path.chars().any(char::is_control)).ok_or_else(CommandError::invalid_params)?.into(),
            "bounds" | "rect" => {
                let bounds: [f32; 4] = serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?;
                if bounds.iter().any(|v| !v.is_finite()) || bounds[0] >= bounds[2] || bounds[1] >= bounds[3] { return Err(CommandError::invalid_params()); }
                slice.bounds = bounds;
                slice.auto_bounds = false;
            },
            "outputs" => slice.outputs = serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?,
            "id" | "order" => {},
            _ => return Err(CommandError::invalid_params()),
        }
    }
    let mut ids = std::collections::HashSet::new();
    for output in &slice.outputs {
        if output.id.0 == 0 || !ids.insert(output.id) || doc.slices.iter().any(|other| other.id != id && other.outputs.iter().any(|old| old.id == output.id)) {
            return Err(CommandError::invalid_params());
        }
        if output.format.is_empty() || !output.format.bytes().all(|byte| byte.is_ascii_alphanumeric()) || output.suffix.chars().any(|c| c.is_control() || matches!(c, '/' | '\\')) || output.path.as_ref().is_some_and(|path| path.chars().any(char::is_control)) || output.options.as_ref().is_some_and(|options| !options.is_object() && !options.is_null()) {
            return Err(CommandError::invalid_params());
        }
        output.dimensions(slice, doc)?;
    }
    if let Some(value) = fields.get("order") {
        let order = value.as_u64().and_then(|value| usize::try_from(value).ok()).filter(|order| *order < slices.len()).ok_or_else(CommandError::invalid_params)?;
        let slice = slices.remove(index);
        slices.insert(order, slice);
    }
    Ok(slices)
}
