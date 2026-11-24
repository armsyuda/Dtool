use std::fs;
use std::path::Path;

use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectKind};
use dtool_core::paint::{Colour, Paint, StrokeStyle};
use roxmltree::Document as XmlDoc;

use crate::import_report::{ImportLoss, ImportReport, LossKind};
use crate::{IoError, IoErrorKind};

const MAX_SVG_SIZE: u64 = 10 * 1024 * 1024;
const MAX_IMAGE_DEPTH: usize = 3;
const PX_TO_PT_SCALE: f32 = 0.75;

fn colour_from_usvg(color: usvg::Color, alpha: f32) -> Colour {
    let a = (alpha * 255.0).clamp(0.0, 255.0) as u8;
    Colour::new(color.red, color.green, color.blue, a)
}

fn paint_from_usvg(paint: &usvg::Paint, opacity: f32, report: &mut ImportReport, context: &str) -> Paint {
    match paint {
        usvg::Paint::Color(color) => Paint::Solid {
            colour: colour_from_usvg(*color, opacity),
        },
        usvg::Paint::LinearGradient(grad) => {
            report.losses.push(ImportLoss::new(
                LossKind::Gradient,
                1,
                false,
                format!("linear gradient converted to solid approximation in {}", context),
            ));
            if let Some(first_stop) = grad.stops().first() {
                let alpha = first_stop.opacity().get() * opacity;
                Paint::Solid {
                    colour: colour_from_usvg(first_stop.color(), alpha),
                }
            } else {
                Paint::None
            }
        }
        usvg::Paint::RadialGradient(grad) => {
            report.losses.push(ImportLoss::new(
                LossKind::Gradient,
                1,
                false,
                format!("radial gradient converted to solid approximation in {}", context),
            ));
            if let Some(first_stop) = grad.stops().first() {
                let alpha = first_stop.opacity().get() * opacity;
                Paint::Solid {
                    colour: colour_from_usvg(first_stop.color(), alpha),
                }
            } else {
                Paint::None
            }
        }
        usvg::Paint::Pattern(_) => {
            report.losses.push(ImportLoss::new(
                LossKind::Pattern,
                1,
                false,
                format!("pattern paint approximated in {}", context),
            ));
            Paint::None
        }
    }
}

fn fill_to_paint(fill: &usvg::Fill, report: &mut ImportReport, context: &str) -> Paint {
    paint_from_usvg(fill.paint(), fill.opacity().get(), report, context)
}

fn stroke_to_style(stroke: &usvg::Stroke, report: &mut ImportReport, context: &str) -> StrokeStyle {
    let mut style = StrokeStyle::default();
    style.paint = paint_from_usvg(stroke.paint(), stroke.opacity().get(), report, context);
    style.width = stroke.width().get();
    if let Some(dash) = stroke.dasharray() {
        style.dash = dash.to_vec();
    }
    style
}

fn object_bounds(node_bbox: usvg::Rect) -> ([f32; 2], [f32; 2]) {
    (
        [node_bbox.x(), node_bbox.y()],
        [node_bbox.width(), node_bbox.height()],
    )
}

fn parse_length(value: &str) -> Result<f32, ()> {
    let trimmed = value.trim();
    let unit_prefixes = ["px", "pt", "mm", "cm", "in", "pc", "em", "ex", "%"];
    let mut number_part = trimmed;
    let mut scale = 1.0f32;
    for unit in unit_prefixes {
        if let Some(stripped) = trimmed.strip_suffix(unit) {
            number_part = stripped.trim();
            scale = match unit {
                "pt" => 1.0 / PX_TO_PT_SCALE,
                "in" => 96.0,
                "mm" => 96.0 / 25.4,
                "cm" => 96.0 / 2.54,
                "pc" => 16.0,
                "%" => 1.0,
                _ => 1.0,
            };
            break;
        }
    }
    number_part.parse::<f32>().map(|n| n * scale).map_err(|_| ())
}

pub fn admit_svg(path: &Path) -> Result<crate::ImportProfile, IoError> {
    let metadata = fs::metadata(path).map_err(|e| IoError::from_io(&e))?;
    if metadata.len() > MAX_SVG_SIZE {
        return Err(IoError::new(IoErrorKind::UnsupportedProfile));
    }

    let content = fs::read_to_string(path).map_err(|e| IoError::from_io(&e))?;

    let doc = XmlDoc::parse(&content).map_err(|_| IoError::new(IoErrorKind::UnsupportedProfile))?;

    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        return Err(IoError::new(IoErrorKind::UnsupportedProfile));
    }

    let width_attr = root.attribute("width");
    let height_attr = root.attribute("height");
    let view_box_attr = root.attribute("viewBox");

    let mut size_found = false;
    let mut width = 0.0f32;
    let mut height = 0.0f32;

    if let Some(w) = width_attr {
        if let Some(h) = height_attr {
            if let Ok(w_val) = parse_length(w) {
                if let Ok(h_val) = parse_length(h) {
                    width = w_val;
                    height = h_val;
                    size_found = true;
                }
            }
        }
    }

    if !size_found {
        if let Some(vb) = view_box_attr {
            let parts: Vec<&str> = vb.split_whitespace().collect();
            if parts.len() == 4 {
                if let (Ok(w), Ok(h)) = (parts[2].parse::<f32>(), parts[3].parse::<f32>()) {
                    width = w;
                    height = h;
                    size_found = true;
                }
            }
        }
    }

    if !size_found {
        return Err(IoError::new(IoErrorKind::UnsupportedProfile));
    }

    let _admitted_width = width;
    let _admitted_height = height;
    let version = root.attribute("version").unwrap_or("1.1").to_string();

    Ok(crate::ImportProfile {
        format: crate::ImportFormat::Svg,
        version,
        encrypted: false,
        pages: 1,
        supported: true,
        reason_key: None,
    })
}

pub fn import_svg(path: &Path) -> Result<(Document, ImportReport), IoError> {
    let profile = admit_svg(path)?;
    if !profile.supported {
        return Err(IoError::new(IoErrorKind::UnsupportedProfile));
    }

    let content = fs::read_to_string(path).map_err(|e| IoError::from_io(&e))?;
    import_svg_data(&content, path.parent())
}

pub fn import_svg_data(content: &str, resources_dir: Option<&Path>) -> Result<(Document, ImportReport), IoError> {
    let mut options = usvg::Options::default();
    options.font_size = 16.0;
    if let Some(dir) = resources_dir {
        options.resources_dir = Some(dir.to_path_buf());
    }

    let tree = usvg::Tree::from_data(content.as_bytes(), &options).map_err(|_| {
        IoError::new(IoErrorKind::InvalidFormat)
    })?;

    let size = tree.size();
    let mut doc = Document::new([size.width(), size.height()], Units::Px, 96.0, 1);
    let mut report = ImportReport {
        objects: 0,
        texts: 0,
        images: 0,
        losses: Vec::new(),
    };

    let svg_dir = resources_dir.unwrap_or(Path::new("."));
    process_group(tree.root(), &mut doc, &mut report, svg_dir, 0);

    Ok((doc, report))
}

fn process_group(
    group: &usvg::Group,
    doc: &mut Document,
    report: &mut ImportReport,
    base_dir: &Path,
    depth: usize,
) {
    let group_name = group.id().to_string();
    if group.clip_path().is_some() {
        report.losses.push(ImportLoss::new(
            LossKind::Mask,
            1,
            false,
            format!("clip path applied to group {}", group_name),
        ));
    }
    if group.mask().is_some() {
        report.losses.push(ImportLoss::new(
            LossKind::Mask,
            1,
            false,
            format!("mask applied to group {}", group_name),
        ));
    }
    if !group.filters().is_empty() {
        report.losses.push(ImportLoss::new(
            LossKind::Filter,
            1,
            false,
            format!("filter applied to group {}", group_name),
        ));
    }

    for child in group.children() {
        process_node(child, doc, report, base_dir, depth);
    }
}

fn process_node(
    node: &usvg::Node,
    doc: &mut Document,
    report: &mut ImportReport,
    base_dir: &Path,
    depth: usize,
) {
    let _ = base_dir;
    match node {
        usvg::Node::Group(g) => {
            process_group(g, doc, report, base_dir, depth + 1);
        }
        usvg::Node::Path(p) => {
            let (pos, size) = object_bounds(p.bounding_box());
            let mut obj = Object::default();
            obj.kind = ObjectKind::Shape;
            obj.name = p.id().to_string();
            obj.pos = pos;
            obj.size = size;
            if let Some(fill) = p.fill() {
                obj.fill = fill_to_paint(fill, report, "path");
            }
            if let Some(stroke) = p.stroke() {
                obj.stroke = stroke_to_style(stroke, report, "path");
            }
            doc.create_object(obj);
            report.objects += 1;
        }
        usvg::Node::Text(t) => {
            for chunk in t.chunks() {
                let chunk_text = chunk.text().to_string();
                let mut obj = Object::default();
                obj.kind = ObjectKind::Text(chunk_text);
                obj.name = t.id().to_string();
                let (pos, size) = object_bounds(t.bounding_box());
                obj.pos = pos;
                obj.size = size;
                if let Some(span) = chunk.spans().first() {
                    if let Some(fill) = span.fill() {
                        obj.fill = fill_to_paint(fill, report, "text");
                    }
                    if let Some(stroke) = span.stroke() {
                        obj.stroke = stroke_to_style(stroke, report, "text");
                    }
                    let _font_size = span.font_size().get();
                    let _font = span.font();
                }
                doc.create_object(obj);
                report.texts += 1;
            }
        }
        usvg::Node::Image(img) => {
            if depth >= MAX_IMAGE_DEPTH {
                report.losses.push(ImportLoss::new(
                    LossKind::Image,
                    1,
                    false,
                    "max image depth exceeded",
                ));
                return;
            }
            match img.kind() {
                usvg::ImageKind::JPEG(data)
                | usvg::ImageKind::PNG(data)
                | usvg::ImageKind::GIF(data)
                | usvg::ImageKind::WEBP(data) => {
                    let (pos, size) = object_bounds(img.bounding_box());
                    let mut obj = Object::default();
                    obj.kind = ObjectKind::Shape;
                    obj.name = img.id().to_string();
                    obj.pos = pos;
                    obj.size = size;
                    obj.raster_data = Some(data.as_ref().clone());
                    let image_size = img.size();
                    obj.raster_size = Some((image_size.width() as u32, image_size.height() as u32));
                    doc.create_object(obj);
                    report.images += 1;
                }
                usvg::ImageKind::SVG(nested) => {
                    if depth + 1 >= MAX_IMAGE_DEPTH {
                        report.losses.push(ImportLoss::new(
                            LossKind::Image,
                            1,
                            false,
                            "nested svg image exceeded max image depth",
                        ));
                    } else {
                        process_group(nested.root(), doc, report, base_dir, depth + 1);
                    }
                }
            }
        }
    }
}
