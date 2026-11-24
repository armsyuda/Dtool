use std::fs;
use std::path::Path;

use dtool_core::{Document, Object, ObjectId, ObjectKind};
use crate::import_report::{ImportReport, LossKind};
use crate::native::{IoError, IoErrorKind};

#[derive(Debug, Clone)]
pub enum CadSelection {
    AllPages,
    SinglePage(u32),
    Model,
}

#[derive(Debug, Clone)]
pub struct CadImportOptions {
    pub selection: CadSelection,
    pub units: String,
    pub dpi: u32,
    pub background: Option<String>,
    pub colour_override: Option<String>,
    pub remove_hidden: bool,
    pub entity_handles: bool,
    pub override_line_weights: bool,
    pub sanitize: bool,
}

impl Default for CadImportOptions {
    fn default() -> Self {
        Self {
            selection: CadSelection::Model,
            units: "mm".into(),
            dpi: 300,
            background: None,
            colour_override: None,
            remove_hidden: false,
            entity_handles: false,
            override_line_weights: false,
            sanitize: false,
        }
    }
}

struct DxfEntity {
    kind: String,
    layer: String,
    points: Vec<(f64, f64)>,
    radius: Option<f64>,
    start_angle: Option<f64>,
    end_angle: Option<f64>,
    major_axis: Option<f64>,
    minor_axis: Option<f64>,
    text: Option<String>,
    insert_point: Option<(f64, f64)>,
    block_name: Option<String>,
    line_weight: Option<f64>,
}

fn parse_dxf(content: &str) -> Result<Vec<DxfEntity>, IoError> {
    let lines: Vec<&str> = content.lines().collect();
    let mut entities = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if lines[i].trim() == "ENTITIES" {
            i += 1;
            break;
        }
        i += 1;
    }

    while i < lines.len() {
        let line = lines[i].trim();
        if line == "ENDSEC" || line == "EOF" {
            break;
        }

        if line == "LINE" {
            let mut entity = DxfEntity {
                kind: "LINE".into(),
                layer: String::new(),
                points: Vec::new(),
                radius: None,
                start_angle: None,
                end_angle: None,
                major_axis: None,
                minor_axis: None,
                text: None,
                insert_point: None,
                block_name: None,
                line_weight: None,
            };
            i += 1;
            while i < lines.len() {
                let code = lines[i].trim();
                if code == "LINE" || code == "LWPOLYLINE" || code == "POLYLINE" || code == "CIRCLE" || code == "ARC" || code == "ELLIPSE" || code == "SPLINE" || code == "TEXT" || code == "MTEXT" || code == "INSERT" || code == "ENDSEC" || code == "EOF" {
                    break;
                }
                if code == "8" {
                    i += 1;
                    entity.layer = lines[i].trim().to_string();
                } else if code == "10" {
                    i += 1;
                    let x: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    i += 1;
                    let y: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    entity.points.push((x, y));
                } else if code == "11" {
                    i += 1;
                    let x: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    i += 1;
                    let y: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    entity.points.push((x, y));
                } else if code == "370" {
                    i += 1;
                    let w: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    entity.line_weight = Some(w);
                }
                i += 1;
            }
            entities.push(entity);
        } else if line == "LWPOLYLINE" || line == "POLYLINE" {
            let mut entity = DxfEntity {
                kind: "POLYLINE".into(),
                layer: String::new(),
                points: Vec::new(),
                radius: None,
                start_angle: None,
                end_angle: None,
                major_axis: None,
                minor_axis: None,
                text: None,
                insert_point: None,
                block_name: None,
                line_weight: None,
            };
            i += 1;
            while i < lines.len() {
                let code = lines[i].trim();
                if code == "LINE" || code == "LWPOLYLINE" || code == "POLYLINE" || code == "CIRCLE" || code == "ARC" || code == "ELLIPSE" || code == "SPLINE" || code == "TEXT" || code == "MTEXT" || code == "INSERT" || code == "ENDSEC" || code == "EOF" {
                    break;
                }
                if code == "8" {
                    i += 1;
                    entity.layer = lines[i].trim().to_string();
                } else if code == "10" {
                    i += 1;
                    let x: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    i += 1;
                    let y: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    entity.points.push((x, y));
                } else if code == "370" {
                    i += 1;
                    let w: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    entity.line_weight = Some(w);
                }
                i += 1;
            }
            entities.push(entity);
        } else if line == "CIRCLE" {
            let mut entity = DxfEntity {
                kind: "CIRCLE".into(),
                layer: String::new(),
                points: Vec::new(),
                radius: None,
                start_angle: None,
                end_angle: None,
                major_axis: None,
                minor_axis: None,
                text: None,
                insert_point: None,
                block_name: None,
                line_weight: None,
            };
            i += 1;
            while i < lines.len() {
                let code = lines[i].trim();
                if code == "LINE" || code == "LWPOLYLINE" || code == "POLYLINE" || code == "CIRCLE" || code == "ARC" || code == "ELLIPSE" || code == "SPLINE" || code == "TEXT" || code == "MTEXT" || code == "INSERT" || code == "ENDSEC" || code == "EOF" {
                    break;
                }
                if code == "8" {
                    i += 1;
                    entity.layer = lines[i].trim().to_string();
                } else if code == "10" {
                    i += 1;
                    let x: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    i += 1;
                    let y: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    entity.points.push((x, y));
                } else if code == "40" {
                    i += 1;
                    let r: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    entity.radius = Some(r);
                } else if code == "370" {
                    i += 1;
                    let w: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    entity.line_weight = Some(w);
                }
                i += 1;
            }
            entities.push(entity);
        } else if line == "ARC" {
            let mut entity = DxfEntity {
                kind: "ARC".into(),
                layer: String::new(),
                points: Vec::new(),
                radius: None,
                start_angle: None,
                end_angle: None,
                major_axis: None,
                minor_axis: None,
                text: None,
                insert_point: None,
                block_name: None,
                line_weight: None,
            };
            i += 1;
            while i < lines.len() {
                let code = lines[i].trim();
                if code == "LINE" || code == "LWPOLYLINE" || code == "POLYLINE" || code == "CIRCLE" || code == "ARC" || code == "ELLIPSE" || code == "SPLINE" || code == "TEXT" || code == "MTEXT" || code == "INSERT" || code == "ENDSEC" || code == "EOF" {
                    break;
                }
                if code == "8" {
                    i += 1;
                    entity.layer = lines[i].trim().to_string();
                } else if code == "10" {
                    i += 1;
                    let x: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    i += 1;
                    let y: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    entity.points.push((x, y));
                } else if code == "40" {
                    i += 1;
                    let r: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    entity.radius = Some(r);
                } else if code == "50" {
                    i += 1;
                    let a: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    entity.start_angle = Some(a);
                } else if code == "51" {
                    i += 1;
                    let a: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    entity.end_angle = Some(a);
                } else if code == "370" {
                    i += 1;
                    let w: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    entity.line_weight = Some(w);
                }
                i += 1;
            }
            entities.push(entity);
        } else if line == "ELLIPSE" {
            let mut entity = DxfEntity {
                kind: "ELLIPSE".into(),
                layer: String::new(),
                points: Vec::new(),
                radius: None,
                start_angle: None,
                end_angle: None,
                major_axis: None,
                minor_axis: None,
                text: None,
                insert_point: None,
                block_name: None,
                line_weight: None,
            };
            i += 1;
            while i < lines.len() {
                let code = lines[i].trim();
                if code == "LINE" || code == "LWPOLYLINE" || code == "POLYLINE" || code == "CIRCLE" || code == "ARC" || code == "ELLIPSE" || code == "SPLINE" || code == "TEXT" || code == "MTEXT" || code == "INSERT" || code == "ENDSEC" || code == "EOF" {
                    break;
                }
                if code == "8" {
                    i += 1;
                    entity.layer = lines[i].trim().to_string();
                } else if code == "10" {
                    i += 1;
                    let x: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    i += 1;
                    let y: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    entity.points.push((x, y));
                } else if code == "11" {
                    i += 1;
                    let x: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    i += 1;
                    let y: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    entity.major_axis = Some((x * x + y * y).sqrt());
                } else if code == "40" {
                    i += 1;
                    let ratio: f64 = lines[i].trim().parse().unwrap_or(1.0);
                    if let Some(major) = entity.major_axis {
                        entity.minor_axis = Some(major * ratio);
                    }
                } else if code == "370" {
                    i += 1;
                    let w: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    entity.line_weight = Some(w);
                }
                i += 1;
            }
            entities.push(entity);
        } else if line == "SPLINE" {
            let mut entity = DxfEntity {
                kind: "SPLINE".into(),
                layer: String::new(),
                points: Vec::new(),
                radius: None,
                start_angle: None,
                end_angle: None,
                major_axis: None,
                minor_axis: None,
                text: None,
                insert_point: None,
                block_name: None,
                line_weight: None,
            };
            i += 1;
            while i < lines.len() {
                let code = lines[i].trim();
                if code == "LINE" || code == "LWPOLYLINE" || code == "POLYLINE" || code == "CIRCLE" || code == "ARC" || code == "ELLIPSE" || code == "SPLINE" || code == "TEXT" || code == "MTEXT" || code == "INSERT" || code == "ENDSEC" || code == "EOF" {
                    break;
                }
                if code == "8" {
                    i += 1;
                    entity.layer = lines[i].trim().to_string();
                } else if code == "10" {
                    i += 1;
                    let x: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    i += 1;
                    let y: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    entity.points.push((x, y));
                } else if code == "370" {
                    i += 1;
                    let w: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    entity.line_weight = Some(w);
                }
                i += 1;
            }
            entities.push(entity);
        } else if line == "TEXT" || line == "MTEXT" {
            let mut entity = DxfEntity {
                kind: "TEXT".into(),
                layer: String::new(),
                points: Vec::new(),
                radius: None,
                start_angle: None,
                end_angle: None,
                major_axis: None,
                minor_axis: None,
                text: None,
                insert_point: None,
                block_name: None,
                line_weight: None,
            };
            i += 1;
            while i < lines.len() {
                let code = lines[i].trim();
                if code == "LINE" || code == "LWPOLYLINE" || code == "POLYLINE" || code == "CIRCLE" || code == "ARC" || code == "ELLIPSE" || code == "SPLINE" || code == "TEXT" || code == "MTEXT" || code == "INSERT" || code == "ENDSEC" || code == "EOF" {
                    break;
                }
                if code == "8" {
                    i += 1;
                    entity.layer = lines[i].trim().to_string();
                } else if code == "10" {
                    i += 1;
                    let x: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    i += 1;
                    let y: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    entity.insert_point = Some((x, y));
                } else if code == "1" {
                    i += 1;
                    entity.text = Some(lines[i].trim().to_string());
                } else if code == "370" {
                    i += 1;
                    let w: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    entity.line_weight = Some(w);
                }
                i += 1;
            }
            entities.push(entity);
        } else if line == "INSERT" {
            let mut entity = DxfEntity {
                kind: "INSERT".into(),
                layer: String::new(),
                points: Vec::new(),
                radius: None,
                start_angle: None,
                end_angle: None,
                major_axis: None,
                minor_axis: None,
                text: None,
                insert_point: None,
                block_name: None,
                line_weight: None,
            };
            i += 1;
            while i < lines.len() {
                let code = lines[i].trim();
                if code == "LINE" || code == "LWPOLYLINE" || code == "POLYLINE" || code == "CIRCLE" || code == "ARC" || code == "ELLIPSE" || code == "SPLINE" || code == "TEXT" || code == "MTEXT" || code == "INSERT" || code == "ENDSEC" || code == "EOF" {
                    break;
                }
                if code == "8" {
                    i += 1;
                    entity.layer = lines[i].trim().to_string();
                } else if code == "2" {
                    i += 1;
                    entity.block_name = Some(lines[i].trim().to_string());
                } else if code == "10" {
                    i += 1;
                    let x: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    i += 1;
                    let y: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    entity.insert_point = Some((x, y));
                } else if code == "370" {
                    i += 1;
                    let w: f64 = lines[i].trim().parse().unwrap_or(0.0);
                    entity.line_weight = Some(w);
                }
                i += 1;
            }
            entities.push(entity);
        } else {
            i += 1;
        }
    }

    Ok(entities)
}

fn units_to_scale(units: &str) -> f64 {
    match units.to_lowercase().as_str() {
        "mm" => 1.0,
        "cm" => 10.0,
        "m" => 1000.0,
        "in" => 25.4,
        "ft" => 304.8,
        _ => 1.0,
    }
}

pub fn import_cad(path: &Path, opts: &CadImportOptions) -> Result<(Document, ImportReport), IoError> {
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

    if extension == "dwg" {
        return Err(IoError::new(IoErrorKind::Other));
    }

    let content = fs::read_to_string(path).map_err(|e| IoError::from_io(&e))?;

    let entities = parse_dxf(&content)?;
    let scale = units_to_scale(&opts.units);

    let mut doc = Document::new([800.0, 600.0], dtool_core::document::Units::Px, 72.0, 1);
    let mut report = ImportReport::default();
    let mut seen_layers: std::collections::HashSet<String> = std::collections::HashSet::new();

    for entity in &entities {
        if opts.remove_hidden && entity.layer.starts_with("0") {
            continue;
        }

        let layer_name = entity.layer.clone();
        if seen_layers.insert(layer_name.clone()) {
            let mut layer_obj = dtool_core::Object::default();
            layer_obj.kind = ObjectKind::Group;
            layer_obj.name = layer_name.clone();
            doc.create_object(layer_obj);
        }

        match entity.kind.as_str() {
            "LINE" => {
                if entity.points.len() >= 2 {
                    let p1 = entity.points[0];
                    let p2 = entity.points[1];
                    let dx = (p2.0 - p1.0) * scale;
                    let dy = (p2.1 - p1.1) * scale;
                    let length = (dx * dx + dy * dy).sqrt();
                    report.add_loss(crate::import_report::LossKind::Other, 1, false, format!("LINE on layer {} length {}", layer_name, length));
                }
            }
            "POLYLINE" => {
                if !entity.points.is_empty() {
                    let mut path = dtool_core::Object::default();
                    {
                        let pts: Vec<[f32; 2]> = entity.points.iter().map(|pt| [(pt.0 * scale) as f32, (pt.1 * scale) as f32]).collect();
                        if let Some(first) = pts.first() {
                            path.pos = *first;
                        }
                        if !pts.is_empty() {
                            let min_x = pts.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
                            let max_x = pts.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max);
                            let min_y = pts.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
                            let max_y = pts.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max);
                            path.pos = [min_x, min_y];
                            path.size = [(max_x - min_x).max(0.0), (max_y - min_y).max(0.0)];
                        }
                        path.kind = ObjectKind::Shape;
                    }
                    doc.create_object(path);
                }
            }
            "CIRCLE" => {
                if let Some(r) = entity.radius {
                    let mut obj = dtool_core::Object::default();
                    obj.size = [((r * scale) as f32) * 2.0, ((r * scale) as f32) * 2.0];
                    if let Some(&pt) = entity.points.first() {
                        obj.pos = [(pt.0 * scale) as f32, (pt.1 * scale) as f32];
                    }
                    doc.create_object(obj);
                }
            }
            "ARC" => {
                if let Some(r) = entity.radius {
                    let mut obj = dtool_core::Object::default();
                    obj.size = [((r * scale) as f32) * 2.0, ((r * scale) as f32) * 2.0];
                    if let Some(&pt) = entity.points.first() {
                        obj.pos = [(pt.0 * scale) as f32, (pt.1 * scale) as f32];
                    }
                    doc.create_object(obj);
                }
            }
            "ELLIPSE" => {
                if let (Some(major), Some(minor)) = (entity.major_axis, entity.minor_axis) {
                    let mut obj = dtool_core::Object::default();
                    obj.size = [((major * scale) as f32) * 2.0, ((minor * scale) as f32) * 2.0];
                    if let Some(&pt) = entity.points.first() {
                        obj.pos = [(pt.0 * scale) as f32, (pt.1 * scale) as f32];
                    }
                    doc.create_object(obj);
                }
            }
            "SPLINE" => {
                if !entity.points.is_empty() {
                    let mut path = dtool_core::Object::default();
                    {
                        let pts: Vec<[f32; 2]> = entity.points.iter().map(|pt| [(pt.0 * scale) as f32, (pt.1 * scale) as f32]).collect();
                        if let Some(first) = pts.first() {
                            path.pos = *first;
                        }
                        if !pts.is_empty() {
                            let min_x = pts.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
                            let max_x = pts.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max);
                            let min_y = pts.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
                            let max_y = pts.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max);
                            path.pos = [min_x, min_y];
                            path.size = [(max_x - min_x).max(0.0), (max_y - min_y).max(0.0)];
                        }
                        path.kind = ObjectKind::Shape;
                    }
                    doc.create_object(path);
                }
            }
            "TEXT" => {
                if let Some(text) = &entity.text {
                    let mut obj = dtool_core::Object::default();
                    obj.kind = ObjectKind::Text(text.clone());
                    if let Some(pt) = entity.insert_point {
                        obj.pos = [(pt.0 * scale) as f32, (pt.1 * scale) as f32];
                    }
                    doc.create_object(obj);
                }
            }
            "INSERT" => {
                report.add_loss(crate::import_report::LossKind::Other, 1, false, format!("INSERT block {}", entity.block_name.as_deref().unwrap_or("unknown")));
            }
            _ => {}
        }
    }

    Ok((doc, report))
}
