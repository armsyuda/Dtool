use std::path::Path;

use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectKind};
use crate::import_report::{ImportReport, LossKind};
use crate::native::{IoError, IoErrorKind};
use crate::af::{AfNode, AfValue, read_container, read_entry, read_tree};

pub fn open_af(path: &Path) -> Result<(Document, ImportReport), IoError> {
    let container = read_container(path)?;
    let doc_dat_entry = container.entries.iter().find(|e| e.name == "doc.dat");

    if doc_dat_entry.is_none() {
        return Err(IoError::new(IoErrorKind::InvalidFormat));
    }

    let bytes = read_entry(path, doc_dat_entry.unwrap())?;
    let root_node = read_tree(&bytes)?;

    let mut document = Document::new([800.0, 600.0], Units::Px, 72.0, 0);
    let mut report = ImportReport::default();

    map_node(&root_node, &mut document, &mut report);

    if document.pages.is_empty() {
        document.push_page([800.0, 600.0]);
    }

    Ok((document, report))
}

fn field_text(node: &AfNode, key: &str) -> Option<String> {
    for (name, value) in &node.fields {
        if name.eq_ignore_ascii_case(key) {
            match value {
                AfValue::Text(s) => return Some(s.clone()),
                AfValue::Int(v) => return Some(v.to_string()),
                AfValue::Float(v) => return Some(v.to_string()),
                AfValue::Bool(v) => return Some(v.to_string()),
                _ => {}
            }
        }
    }
    None
}

fn field_f32(node: &AfNode, key: &str) -> Option<f32> {
    for (name, value) in &node.fields {
        if name.eq_ignore_ascii_case(key) {
            match value {
                AfValue::Float(v) => return Some(*v as f32),
                AfValue::Int(v) => return Some(*v as f32),
                AfValue::Text(s) => return s.parse().ok(),
                _ => {}
            }
        }
    }
    None
}

fn node_name(node: &AfNode) -> String {
    field_text(node, "name")
        .or_else(|| field_text(node, "id"))
        .unwrap_or_else(|| node.tag.clone())
}

fn node_pos_size(node: &AfNode) -> ([f32; 2], [f32; 2]) {
    let x = field_f32(node, "x").or_else(|| field_f32(node, "left")).unwrap_or(0.0);
    let y = field_f32(node, "y").or_else(|| field_f32(node, "top")).unwrap_or(0.0);
    let w = field_f32(node, "w")
        .or_else(|| field_f32(node, "width"))
        .unwrap_or(0.0);
    let h = field_f32(node, "h")
        .or_else(|| field_f32(node, "height"))
        .unwrap_or(0.0);
    ([x, y], [w, h])
}

fn push_object(doc: &mut Document, kind: ObjectKind, node: &AfNode) {
    let (pos, size) = node_pos_size(node);
    let mut obj = Object::default();
    obj.kind = kind;
    obj.name = node_name(node);
    obj.pos = pos;
    obj.size = size;
    doc.create_object(obj);
}

fn map_node(node: &AfNode, doc: &mut Document, report: &mut ImportReport) {
    match node.tag.to_ascii_lowercase().as_str() {
        "page" | "artboard" => {
            let (_pos, size) = node_pos_size(node);
            let page_size = if size[0] > 0.0 && size[1] > 0.0 {
                size
            } else if let Some(page) = doc.pages.first() {
                page.size
            } else {
                [800.0, 600.0]
            };
            doc.push_page(page_size);
            for child in &node.children {
                map_node(child, doc, report);
            }
        }
        "layer" | "group" => {
            push_object(doc, ObjectKind::Group, node);
            for child in &node.children {
                map_node(child, doc, report);
            }
        }
        "shape" | "curve" => {
            push_object(doc, ObjectKind::Shape, node);
            for child in &node.children {
                map_node(child, doc, report);
            }
        }
        "text" => {
            let text = field_text(node, "text")
                .or_else(|| field_text(node, "content"))
                .unwrap_or_default();
            push_object(doc, ObjectKind::Text(text), node);
            for child in &node.children {
                map_node(child, doc, report);
            }
        }
        "image" => {
            let mut obj = Object::default();
            let (pos, size) = node_pos_size(node);
            obj.kind = ObjectKind::Shape;
            obj.name = node_name(node);
            obj.pos = pos;
            obj.size = size;
            if let Some(AfValue::Bytes(bytes)) = node.fields.iter().find(|(n, _)| {
                n.eq_ignore_ascii_case("data") || n.eq_ignore_ascii_case("bytes")
            }).map(|(_, v)| v)
            {
                obj.raster_data = Some(bytes.clone());
            }
            doc.create_object(obj);
            report.images += 1;
            for child in &node.children {
                map_node(child, doc, report);
            }
        }
        _ => {
            report.add_loss(
                LossKind::Other,
                1,
                false,
                format!("unmapped node tag: {}", node.tag),
            );
            for child in &node.children {
                map_node(child, doc, report);
            }
        }
    }
}
