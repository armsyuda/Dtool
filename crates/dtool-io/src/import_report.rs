use std::fmt;

use dtool_core::object::ObjectKind;
use dtool_core::paint::{Colour, Paint};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LossKind {
    Font,
    Glyph,
    Gradient,
    Pattern,
    Mask,
    Filter,
    Blend,
    Icc,
    VerticalText,
    Image,
    Annotation,
    Other,
}

impl fmt::Display for LossKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LossKind::Font => write!(f, "Font"),
            LossKind::Glyph => write!(f, "Glyph"),
            LossKind::Gradient => write!(f, "Gradient"),
            LossKind::Pattern => write!(f, "Pattern"),
            LossKind::Mask => write!(f, "Mask"),
            LossKind::Filter => write!(f, "Filter"),
            LossKind::Blend => write!(f, "Blend"),
            LossKind::Icc => write!(f, "ICC"),
            LossKind::VerticalText => write!(f, "Vertical Text"),
            LossKind::Image => write!(f, "Image"),
            LossKind::Annotation => write!(f, "Annotation"),
            LossKind::Other => write!(f, "Other"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImportLoss {
    pub kind: LossKind,
    pub count: u32,
    pub required: bool,
    pub detail: String,
}

impl ImportLoss {
    pub fn new(kind: LossKind, count: u32, required: bool, detail: impl Into<String>) -> Self {
        Self {
            kind,
            count,
            required,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ImportReport {
    pub objects: u32,
    pub texts: u32,
    pub images: u32,
    pub losses: Vec<ImportLoss>,
}

impl ImportReport {
    pub fn add_loss(&mut self, kind: LossKind, count: u32, required: bool, detail: impl Into<String>) {
        if count > 0 {
            self.losses.push(ImportLoss::new(kind, count, required, detail));
        }
    }

    pub fn is_acceptable(&self) -> bool {
        !self.losses.iter().any(|l| l.required)
    }
}

#[derive(Debug, Clone)]
pub struct FidelityManifest {
    pub objects: Vec<(String, [f32; 4])>,
    pub texts: Vec<String>,
    pub colours: Vec<String>,
}

fn rects_close(actual: [f32; 4], expected: [f32; 4], tol: f32) -> bool {
    actual
        .iter()
        .zip(expected.iter())
        .all(|(a, e)| (a - e).abs() <= tol)
}

fn object_rect(obj: &dtool_core::Object) -> [f32; 4] {
    [obj.pos[0], obj.pos[1], obj.size[0], obj.size[1]]
}

fn object_has_text(obj: &dtool_core::Object, expected: &str) -> bool {
    match &obj.kind {
        ObjectKind::Text(text) => text.contains(expected),
        _ => false,
    }
}

fn colour_to_keys(colour: Colour) -> Vec<String> {
    vec![
        format!("#{:02x}{:02x}{:02x}", colour.r, colour.g, colour.b),
        format!("#{:02x}{:02x}{:02x}{:02x}", colour.r, colour.g, colour.b, colour.a),
        format!("{},{},{}", colour.r, colour.g, colour.b),
        format!("{},{},{},{}", colour.r, colour.g, colour.b, colour.a),
        format!("rgb({},{},{})", colour.r, colour.g, colour.b),
    ]
}

fn paint_colours(paint: &Paint) -> Vec<Colour> {
    match paint {
        Paint::Solid { colour } => vec![*colour],
        Paint::Gradient { stops, .. } => stops.iter().map(|s| s.colour).collect(),
        Paint::Hatch(hatch) => {
            let mut colours = vec![hatch.colour];
            if let Some(bg) = hatch.background {
                colours.push(bg);
            }
            colours
        }
        Paint::Mesh(mesh) => mesh.colours.clone(),
        _ => Vec::new(),
    }
}

fn document_has_colour(doc: &dtool_core::Document, expected: &str) -> bool {
    let needle = expected.trim().to_ascii_lowercase();
    for obj in &doc.objects {
        let mut colours = paint_colours(&obj.fill);
        colours.extend(paint_colours(&obj.stroke.paint));
        for colour in colours {
            if colour_to_keys(colour)
                .iter()
                .any(|key| key.to_ascii_lowercase() == needle)
            {
                return true;
            }
        }
    }
    false
}

fn document_has_text(doc: &dtool_core::Document, expected: &str) -> bool {
    if doc.objects.iter().any(|obj| object_has_text(obj, expected)) {
        return true;
    }
    doc.stories.iter().any(|story| story.text.contains(expected))
}

fn document_has_object_with_bounds(
    doc: &dtool_core::Document,
    name: &str,
    rect: &[f32; 4],
    tol: f32,
) -> bool {
    doc.objects.iter().any(|obj| {
        (obj.name == name || name.is_empty()) && rects_close(object_rect(obj), *rect, tol)
    })
}

pub fn fidelity_check(doc: &dtool_core::Document, expected: &FidelityManifest, tol: f32) -> Vec<String> {
    let mut mismatches = Vec::new();

    for (name, rect) in &expected.objects {
        if !document_has_object_with_bounds(doc, name, rect, tol) {
            mismatches.push(format!("object:{}:bounds", name));
        }
    }

    for text in &expected.texts {
        if !document_has_text(doc, text) {
            mismatches.push(format!("text:{}", text));
        }
    }

    for colour in &expected.colours {
        if !document_has_colour(doc, colour) {
            mismatches.push(format!("colour:{}", colour));
        }
    }

    mismatches
}
