use serde::{Deserialize, Serialize};
use crate::curve::Curve;
use crate::table::model::{StoryId, TableNode};
use crate::paint::{Paint, StrokeStyle};
use crate::colour::model::ObjectIcc;
use crate::frame::FrameProps;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct ObjectId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PathFlow {
    #[serde(default)]
    pub start_pct: f32,
    #[serde(default)]
    pub end_pct: f32,
    #[serde(default)]
    pub baseline_pct: f32,
    #[serde(default)]
    pub reversed: bool,
    #[serde(default)]
    pub flipped: bool,
}

impl Default for PathFlow {
    fn default() -> Self {
        Self { start_pct: 0.0, end_pct: 1.0, baseline_pct: 0.5, reversed: false, flipped: false }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathTextData {
    pub path: ObjectId,
    pub story: StoryId,
    #[serde(default)]
    pub flows: Vec<PathFlow>,
    #[serde(default)]
    pub source_paint: Option<String>,
}

impl Default for PathTextData {
    fn default() -> Self {
        Self { path: ObjectId(0), story: StoryId(0), flows: vec![PathFlow::default()], source_paint: None }
    }
}

pub const IDENTITY_TEXT_SCALE: [f32; 2] = [1.0, 1.0];

fn default_text_scale() -> [f32; 2] { IDENTITY_TEXT_SCALE }

fn is_identity_text_scale(scale: &[f32; 2]) -> bool { *scale == IDENTITY_TEXT_SCALE }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Object {
    pub id: ObjectId,
    pub name: String,
    pub pos: [f32; 2],
    pub size: [f32; 2],
    #[serde(deserialize_with = "crate::paint::legacy_paint::deserialize")]
    pub fill: Paint,
    #[serde(deserialize_with = "crate::paint::legacy_stroke::deserialize")]
    pub stroke: StrokeStyle,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_fills: Vec<Paint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_strokes: Vec<StrokeStyle>,
    #[serde(default, skip_serializing_if = "crate::commands::appearance::HiddenRows::is_empty")]
    pub hidden_appearance: crate::commands::appearance::HiddenRows,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub appearance_stack: Vec<crate::commands::appearance::Kind>,
    #[serde(default, skip_serializing_if = "crate::commands::appearance::RowBlends::is_empty")]
    pub appearance_blends: crate::commands::appearance::RowBlends,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_brush: Option<crate::path_brush::Texture>,
    #[serde(default = "default_paint_order")]
    pub paint_order: crate::paint::PaintOrder,
    pub opacity: f32,
    pub kind: ObjectKind,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub layer_folder: bool,
    pub parent_id: Option<ObjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constraint: Option<crate::constraints::Constraint>,
    /// Ancestor warps whose deformation is already included in this geometry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub baked_warps: Vec<ObjectId>,
    pub rotation_deg: f32,
    pub shear_deg: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform_origin: Option<[f32; 2]>,
    /// Scale applied after shaping straight text. Linked stories keep their original metrics.
    #[serde(default = "default_text_scale", skip_serializing_if = "is_identity_text_scale")]
    pub text_scale: [f32; 2],
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub vertical_art_text: bool,
    /// Maps the path layout plane to this layer's document-space geometry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_text_linear: Option<[f32; 4]>,
    pub pickable: bool,
    pub locked: bool,
    #[serde(default)]
    pub shape: Option<crate::shape::ShapeRecipe>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fill_layer: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qr_payload: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qr_content: Option<crate::commands::qr::QrContent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_text_curve: Option<Curve>,
    #[serde(default)]
    pub raster_data: Option<Vec<u8>>,
    #[serde(default)]
    pub raster_size: Option<(u32, u32)>,
    #[serde(default)]
    pub icc: Option<ObjectIcc>,
    #[serde(default)]
    pub frame: Option<FrameProps>,
    #[serde(default)]
    pub vector_crop: Option<VectorCrop>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_id: Option<crate::document::PageId>,
    #[serde(default)]
    pub transparency: crate::paint::Transparency,
    #[serde(default, skip_serializing_if = "crate::layer_style::is_normal")]
    pub blend_mode: crate::blend::BlendMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blend_ranges: Option<crate::layer_style::BlendRanges>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone_layer: Option<crate::tone::ToneLayer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone_blend: Option<crate::tone_blend::ToneBlend>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<crate::layer_style::LayerEffect>,
    /// The style of an original textured object whose fill and outline now live in this group.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub expanded_stroke_effects: bool,
    /// Original silhouette retained inside a stroke expansion used as a vector mask.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stroke_mask_outline: bool,
    #[serde(default, skip_serializing_if = "ClipRole::is_none")]
    pub clip_role: ClipRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClipRole {
    #[default]
    None,
    Clip,
    Mask,
}

impl ClipRole {
    pub fn is_none(&self) -> bool {
        *self == ClipRole::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VectorCrop {
    pub rect: [f32; 4],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dimension {
    pub start: [f64; 2],
    pub end: [f64; 2],
    #[serde(default)]
    pub offset: f64,
    #[serde(default)]
    pub length: f64,
    #[serde(default)]
    pub units: String,
}

impl Default for Object {
    fn default() -> Self {
        Object {
            id: ObjectId(0),
            name: String::new(),
            pos: [0.0, 0.0],
            size: [0.0, 0.0],
            fill: Paint::None,
            stroke: StrokeStyle::default(),
            additional_fills: Vec::new(),
            additional_strokes: Vec::new(),
            hidden_appearance: Default::default(),
            appearance_stack: Vec::new(),
            appearance_blends: Default::default(),
            path_brush: None,
            paint_order: default_paint_order(),
            opacity: 1.0,
            kind: ObjectKind::default(),
            layer_folder: false,
            qr_payload: None,
            qr_content: None,
            parent_id: None,
            constraint: None,
            baked_warps: Vec::new(),
            rotation_deg: 0.0,
            shear_deg: 0.0,
            transform_origin: None,
            text_scale: IDENTITY_TEXT_SCALE,
            vertical_art_text: false,
            path_text_linear: None,
            pickable: true,
            locked: false,
            shape: None,
            fill_layer: false,
            path_text_curve: None,
            raster_data: None,
            raster_size: None,
            icc: None,
            frame: None,
            vector_crop: None,
            page_id: None,
            transparency: crate::paint::Transparency::None,
            blend_mode: crate::blend::BlendMode::Normal,
            blend_ranges: None,
            tone_layer: None,
            tone_blend: None,
            effects: Vec::new(),
            expanded_stroke_effects: false,
            stroke_mask_outline: false,
            clip_role: ClipRole::None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum ObjectKind {
    #[default]
    Shape,
    Table(TableNode),
    Group,
    Image,
    PictureFrame,
    Text(String),
    ArtText { story: StoryId },
    FrameText { story: StoryId },
    PathText(PathTextData),
    Curve(Curve),
    Dimension(Dimension),
    Artboard,
    Compound(crate::boolean::CompoundSpec),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectCreateArgs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub pos: [f32; 2],
    pub size: [f32; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<Paint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke: Option<StrokeStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectUpdateArgs {
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pos: Option<[f32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<[f32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<Paint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke: Option<StrokeStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transparency: Option<crate::paint::Transparency>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectDeleteArgs {
    pub id: u64,
}

pub fn validate_fill(fill: &Paint) -> bool {
    crate::paint::validate_paint(fill)
}

pub fn validate_opacity(opacity: f32) -> bool {
    opacity >= 0.0 && opacity <= 1.0
}

impl ObjectKind {
    pub fn kind_name(&self) -> &'static str {
        match self {
            ObjectKind::Shape => "Shape",
            ObjectKind::Table(_) => "Table",
            ObjectKind::Group => "Group",
            ObjectKind::Text(_) => "Text",
            ObjectKind::ArtText { .. } => "ArtText",
            ObjectKind::FrameText { .. } => "FrameText",
            ObjectKind::PathText(_) => "PathText",
            ObjectKind::Curve(_) => "Curve",
            ObjectKind::Dimension(_) => "Dimension",
            ObjectKind::Artboard => "Artboard",
            ObjectKind::Image => "Image",
            ObjectKind::PictureFrame => "PictureFrame",
            ObjectKind::Compound(_) => "Compound",
        }
    }

    pub fn as_path_text(&self) -> Option<&PathTextData> {
        match self {
            ObjectKind::PathText(data) => Some(data),
            _ => None,
        }
    }
}

impl Object {
    pub fn is_group(&self) -> bool {
        matches!(self.kind, ObjectKind::Group)
    }

    pub fn is_layer_folder(&self) -> bool {
        self.layer_folder && self.is_group()
    }

    pub fn is_text_frame(&self) -> bool {
        matches!(self.kind, ObjectKind::Text(_) | ObjectKind::FrameText { .. } | ObjectKind::PathText(_))
    }

    pub fn text_content(&self) -> Option<&str> {
        match &self.kind {
            ObjectKind::Text(content) => Some(content.as_str()),
            _ => None,
        }
    }

    pub fn story_id(&self) -> Option<StoryId> {
        match &self.kind {
            ObjectKind::ArtText { story } => Some(*story),
            ObjectKind::FrameText { story, .. } => Some(*story),
            ObjectKind::PathText(data) => Some(data.story),
            _ => None,
        }
    }

    pub fn path_text_data(&self) -> Option<&PathTextData> {
        match &self.kind {
            ObjectKind::PathText(data) => Some(data),
            _ => None,
        }
    }

    pub fn frame_props(&self) -> Option<FrameProps> {
        match &self.kind {
            ObjectKind::FrameText { .. } => {
                let layout_size = std::array::from_fn(|i| self.size[i].abs() / self.text_scale[i]);
                let mut props = self.frame.clone().unwrap_or_else(|| FrameProps::from_rect(layout_size));
                if let Some(path) = &mut props.shape_path
                    && props.size.iter().all(|side| side.is_finite() && *side > 0.0)
                {
                    for [x, y] in path {
                        *x *= layout_size[0] / props.size[0];
                        *y *= layout_size[1] / props.size[1];
                    }
                }
                props.size = layout_size;
                Some(props)
            }
            _ => None,
        }
    }

    pub fn is_vertical_art_text(&self) -> bool {
        self.vertical_art_text && matches!(self.kind, ObjectKind::ArtText { .. })
    }

    pub fn is_path_text(&self) -> bool {
        matches!(self.kind, ObjectKind::PathText(_))
    }

    pub fn is_curve(&self) -> bool {
        matches!(self.kind, ObjectKind::Curve(_))
    }

    pub fn curve(&self) -> Option<&Curve> {
        match &self.kind {
            ObjectKind::Curve(curve) => Some(curve),
            _ => None,
        }
    }

    pub fn curve_mut(&mut self) -> Option<&mut Curve> {
        match &mut self.kind {
            ObjectKind::Curve(curve) => Some(curve),
            _ => None,
        }
    }
}

impl PartialEq for ObjectKind {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (ObjectKind::Shape, ObjectKind::Shape) => true,
            (ObjectKind::Group, ObjectKind::Group) => true,
            (ObjectKind::Image, ObjectKind::Image) => true,
            (ObjectKind::PictureFrame, ObjectKind::PictureFrame) => true,
            (ObjectKind::Artboard, ObjectKind::Artboard) => true,
            (ObjectKind::Table(_), ObjectKind::Table(_)) => true,
            (ObjectKind::Text(_), ObjectKind::Text(_)) => true,
            (ObjectKind::ArtText { story: a }, ObjectKind::ArtText { story: b }) => a == b,
            (ObjectKind::FrameText { story: a }, ObjectKind::FrameText { story: b }) => a == b,
            (ObjectKind::PathText(a), ObjectKind::PathText(b)) => a == b,
            (ObjectKind::Curve(a), ObjectKind::Curve(b)) => a == b,
            (ObjectKind::Dimension(a), ObjectKind::Dimension(b)) => a == b,
            _ => false,
        }
    }
}

fn default_paint_order() -> crate::paint::PaintOrder { crate::paint::PaintOrder::StrokeOver }

pub const NODE_ICC_SET: &str = "node.icc.set";

impl Object {
    pub fn effective_intent(&self, document_intent: crate::colour::model::RenderingIntent) -> crate::colour::model::RenderingIntent {
        self.icc.as_ref().map(|icc| icc.effective_intent(document_intent)).unwrap_or(document_intent)
    }

    pub fn set_icc(&mut self, profile: Option<Option<String>>, intent: Option<Option<crate::colour::model::RenderingIntent>>) {
        let mut icc = self.icc.take().unwrap_or(ObjectIcc { profile: None, intent: None });
        if let Some(profile) = profile {
            icc.profile = profile;
        }
        if let Some(intent) = intent {
            icc.intent = intent;
        }
        self.icc = Some(icc);
    }

    pub fn convert_colour_for_document(&self, colour: crate::colour::model::Colour, format: crate::colour::model::ColourFormat, prefs: &crate::colour::model::ColourPrefs) -> crate::colour::model::Colour {
        colour.write_for_object(format, prefs, self.icc.as_ref())
    }

    pub fn convert_raster_for_document(&self, format: crate::colour::model::ColourFormat, prefs: &crate::colour::model::ColourPrefs) -> Option<Vec<crate::colour::model::Colour>> {
        let data = self.raster_data.as_ref()?;
        let (width, height) = self.raster_size?;
        let count = width as usize * height as usize;
        if width == 0 || height == 0 || data.len() < count * 4 {
            return None;
        }
        let mut colours = Vec::with_capacity(count);
        for pixel in data.chunks_exact(4).take(count) {
            let colour = crate::colour::model::Colour::rgb(
                pixel[0] as f32 / 255.0,
                pixel[1] as f32 / 255.0,
                pixel[2] as f32 / 255.0,
                pixel[3] as f32 / 255.0,
            );
            colours.push(self.convert_colour_for_document(colour, format, prefs));
        }
        Some(colours)
    }
}

fn parse_object_id(value: &serde_json::Value) -> Option<ObjectId> {
    if let Some(id) = value.as_u64() {
        return Some(ObjectId(id));
    }
    let text = value.as_str()?;
    let raw = text.strip_prefix("node:").unwrap_or(text);
    raw.parse::<u64>().ok().map(ObjectId)
}

fn parse_optional_intent(value: &serde_json::Value) -> Result<Option<crate::colour::model::RenderingIntent>, dtool_protocol::CommandError> {
    if value.is_null() {
        return Ok(None);
    }
    let name = value.as_str().ok_or_else(|| dtool_protocol::CommandError::new("UNSUPPORTED_INTENT"))?;
    crate::colour::model::RenderingIntent::parse(name).map(Some).ok_or_else(|| dtool_protocol::CommandError::new("UNSUPPORTED_INTENT"))
}

fn parse_optional_profile(value: &serde_json::Value) -> Result<Option<String>, dtool_protocol::CommandError> {
    if value.is_null() {
        return Ok(None);
    }
    let name = value.as_str().ok_or_else(dtool_protocol::CommandError::invalid_params)?;
    if name.is_empty() {
        return Ok(None);
    }
    let services = crate::colour::services().read().map_err(|_| dtool_protocol::CommandError::new("PROFILE_NOT_FOUND"))?;
    services.profiles.find(name).map(|profile| Some(profile.name.clone())).ok_or_else(|| dtool_protocol::CommandError::new("PROFILE_NOT_FOUND"))
}

pub fn apply_node_icc_set(object: &mut Object, params: &serde_json::Value) -> Result<(), dtool_protocol::CommandError> {
    if !params.is_object() {
        return Err(dtool_protocol::CommandError::invalid_params());
    }
    let profile = if params.get("profile").is_some() {
        Some(parse_optional_profile(&params["profile"])?)
    } else {
        None
    };
    let intent = if params.get("intent").is_some() {
        Some(parse_optional_intent(&params["intent"])?)
    } else {
        None
    };
    if profile.is_none() && intent.is_none() {
        return Err(dtool_protocol::CommandError::invalid_params());
    }
    object.set_icc(profile, intent);
    Ok(())
}

pub fn node_icc_set(objects: &mut [Object], params: &serde_json::Value) -> Result<(), dtool_protocol::CommandError> {
    let raw = params.get("id").or_else(|| params.get("node")).ok_or_else(dtool_protocol::CommandError::invalid_params)?;
    let id = parse_object_id(raw).ok_or_else(dtool_protocol::CommandError::invalid_params)?;
    let object = objects.iter_mut().find(|item| item.id == id).ok_or_else(dtool_protocol::CommandError::not_found)?;
    apply_node_icc_set(object, params)
}

pub fn is_node_icc_set(name: &str) -> bool {
    name == NODE_ICC_SET
}

#[cfg(test)]
mod intent_tests {
    use super::*;
    use crate::colour::model::{Colour, ColourFormat, ColourPrefs, ObjectIcc, RenderingIntent};

    #[test]
    fn perceptual_and_relative_convert_differ() {
        let prefs = ColourPrefs::default();
        let format = ColourFormat::CMYK_8;
        let colour = Colour::rgb(1.0, 0.0, 0.0, 1.0);
        let perceptual = Object {
            icc: Some(ObjectIcc { profile: Some(prefs.rgb.clone()), intent: Some(RenderingIntent::Perceptual) }),
            raster_data: Some(vec![255, 0, 0, 255]),
            raster_size: Some((1, 1)),
            ..Object::default()
        };
        let relative = Object {
            icc: Some(ObjectIcc { profile: Some(prefs.rgb.clone()), intent: Some(RenderingIntent::RelativeColorimetric) }),
            raster_data: Some(vec![255, 0, 0, 255]),
            raster_size: Some((1, 1)),
            ..Object::default()
        };
        let a = perceptual.convert_colour_for_document(colour.clone(), format, &prefs);
        let b = relative.convert_colour_for_document(colour, format, &prefs);
        assert_ne!(a.values, b.values);
        let ra = perceptual.convert_raster_for_document(format, &prefs).expect("raster");
        let rb = relative.convert_raster_for_document(format, &prefs).expect("raster");
        assert_ne!(ra[0].values, rb[0].values);
    }

    #[test]
    fn node_icc_set_none_uses_document_intent() {
        let mut object = Object { id: ObjectId(3), ..Object::default() };
        node_icc_set(std::slice::from_mut(&mut object), &serde_json::json!({"id": 3, "intent": "Saturation"})).expect("set");
        assert_eq!(object.icc.as_ref().and_then(|icc| icc.intent), Some(RenderingIntent::Saturation));
        node_icc_set(std::slice::from_mut(&mut object), &serde_json::json!({"id": 3, "intent": null})).expect("clear");
        assert_eq!(object.icc.as_ref().and_then(|icc| icc.intent), None);
        assert_eq!(object.effective_intent(RenderingIntent::RelativeColorimetric), RenderingIntent::RelativeColorimetric);
    }
}
