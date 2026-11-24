use std::collections::HashMap;
use std::path::{Path, PathBuf};

use dtool_core::{Document, ObjectId, ObjectKind};

use crate::import_report::{ImportLoss, ImportReport, LossKind};
use crate::native::{IoError, IoErrorKind};

#[derive(Debug, Clone)]
pub struct IdmlPackage {
    pub spreads: Vec<String>,
    pub masters: Vec<String>,
    pub stories: Vec<String>,
    pub styles: Vec<String>,
    pub links: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct StyleConflict {
    pub incoming: String,
    pub rename_to: String,
    pub conflicts: bool,
}

fn read_zip_entries(path: &Path) -> Result<HashMap<String, Vec<u8>>, IoError> {
    let file = std::fs::File::open(path).map_err(|e| IoError::from_io(&e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| IoError::new(IoErrorKind::Other))?;

    let mut entries = HashMap::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| IoError::new(IoErrorKind::Other))?;
        let name = match entry.name() { Ok(n) => n.to_string(), Err(_) => continue };
        if !name.ends_with(".xml") && !name.ends_with(".idml") {
            continue;
        }
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut buf).map_err(|e| IoError::new(IoErrorKind::Other))?;
        entries.insert(name, buf);
    }
    Ok(entries)
}

fn parse_spreads(entries: &HashMap<String, Vec<u8>>) -> Result<Vec<String>, IoError> {
    let mut spreads = Vec::new();
    for (name, data) in entries {
        if name.contains("spread") && name.ends_with(".xml") {
            let xml = String::from_utf8_lossy(data).into_owned();
            let doc = roxmltree::Document::parse(&xml).map_err(|_| IoError::new(IoErrorKind::InvalidFormat))?;
            if let Some(spread_id) = doc.root().children().find(|n| n.has_attribute("id")).and_then(|n| n.attribute("id")) {
                spreads.push(spread_id.to_string());
            } else {
                spreads.push(name.clone());
            }
        }
    }
    Ok(spreads)
}

fn parse_masters(entries: &HashMap<String, Vec<u8>>) -> Result<Vec<String>, IoError> {
    let mut masters = Vec::new();
    for (name, data) in entries {
        if name.contains("master") && name.ends_with(".xml") {
            let xml = String::from_utf8_lossy(data).into_owned();
            let doc = roxmltree::Document::parse(&xml).map_err(|_| IoError::new(IoErrorKind::InvalidFormat))?;
            if let Some(master_id) = doc.root().children().find(|n| n.has_attribute("id")).and_then(|n| n.attribute("id")) {
                masters.push(master_id.to_string());
            } else {
                masters.push(name.clone());
            }
        }
    }
    Ok(masters)
}

fn parse_stories(entries: &HashMap<String, Vec<u8>>) -> Result<Vec<String>, IoError> {
    let mut stories = Vec::new();
    for (name, data) in entries {
        if name.contains("story") && name.ends_with(".xml") {
            let xml = String::from_utf8_lossy(data).into_owned();
            let doc = roxmltree::Document::parse(&xml).map_err(|_| IoError::new(IoErrorKind::InvalidFormat))?;
            if let Some(story_id) = doc.root().children().find(|n| n.has_attribute("id")).and_then(|n| n.attribute("id")) {
                stories.push(story_id.to_string());
            } else {
                stories.push(name.clone());
            }
        }
    }
    Ok(stories)
}

fn parse_styles(entries: &HashMap<String, Vec<u8>>) -> Result<Vec<String>, IoError> {
    let mut styles = Vec::new();
    for (name, data) in entries {
        if name.contains("style") && name.ends_with(".xml") {
            let xml = String::from_utf8_lossy(data).into_owned();
            let doc = roxmltree::Document::parse(&xml).map_err(|_| IoError::new(IoErrorKind::InvalidFormat))?;
            for node in doc.root().descendants() {
                if node.has_attribute("name") {
                    if let Some(style_name) = node.attribute("name") {
                        styles.push(style_name.to_string());
                    }
                }
            }
        }
    }
    Ok(styles)
}

fn parse_links(entries: &HashMap<String, Vec<u8>>) -> Result<Vec<PathBuf>, IoError> {
    let mut links = Vec::new();
    for (name, data) in entries {
        if name.contains("link") && name.ends_with(".xml") {
            let xml = String::from_utf8_lossy(data).into_owned();
            let doc = roxmltree::Document::parse(&xml).map_err(|_| IoError::new(IoErrorKind::InvalidFormat))?;
            for node in doc.root().descendants() {
                if let Some(href) = node.attribute("href") {
                    links.push(PathBuf::from(href));
                }
            }
        }
    }
    Ok(links)
}

pub fn read_idml(path: &Path) -> Result<IdmlPackage, IoError> {
    let entries = read_zip_entries(path)?;

    let spreads = parse_spreads(&entries)?;
    let masters = parse_masters(&entries)?;
    let stories = parse_stories(&entries)?;
    let styles = parse_styles(&entries)?;
    let links = parse_links(&entries)?;

    Ok(IdmlPackage {
        spreads,
        masters,
        stories,
        styles,
        links,
    })
}

fn extract_text_frames(entries: &HashMap<String, Vec<u8>>, report: &mut ImportReport) -> Result<Vec<(String, String)>, IoError> {
    let mut frames = Vec::new();
    for (name, data) in entries {
        if name.contains("spread") && name.ends_with(".xml") {
            let xml = String::from_utf8_lossy(data).into_owned();
            let doc = roxmltree::Document::parse(&xml).map_err(|_| IoError::new(IoErrorKind::InvalidFormat))?;
            for node in doc.root().descendants() {
                if node.has_attribute("id") && node.tag_name().name() == "TextFrame" {
                    let id = node.attribute("id").unwrap_or("").to_string();
                    let content = node.children().filter(|n| n.tag_name().name() == "TextRun").map(|n| n.text().unwrap_or("")).collect::<Vec<_>>().join("");
                    frames.push((id, content));
                }
            }
        }
    }
    Ok(frames)
}

fn extract_shapes(entries: &HashMap<String, Vec<u8>>, report: &mut ImportReport) -> Result<Vec<(String, ObjectKind)>, IoError> {
    let mut shapes = Vec::new();
    for (name, data) in entries {
        if name.contains("spread") && name.ends_with(".xml") {
            let xml = String::from_utf8_lossy(data).into_owned();
            let doc = roxmltree::Document::parse(&xml).map_err(|_| IoError::new(IoErrorKind::InvalidFormat))?;
            for node in doc.root().descendants() {
                if !node.has_attribute("id") {
                    continue;
                }
                let id = node.attribute("id").unwrap_or("").to_string();
                match node.tag_name().name() {
                    "Rectangle" => shapes.push((id, ObjectKind::Shape)),
                    "Ellipse" | "Oval" => shapes.push((id, ObjectKind::Shape)),
                    "Polygon" => shapes.push((id, ObjectKind::Shape)),
                    _ => {}
                }
            }
        }
    }
    Ok(shapes)
}

fn extract_image_links(entries: &HashMap<String, Vec<u8>>, report: &mut ImportReport) -> Result<Vec<(String, PathBuf)>, IoError> {
    let mut image_links = Vec::new();
    for (name, data) in entries {
        if name.contains("spread") && name.ends_with(".xml") {
            let xml = String::from_utf8_lossy(data).into_owned();
            let doc = roxmltree::Document::parse(&xml).map_err(|_| IoError::new(IoErrorKind::InvalidFormat))?;
            for node in doc.root().descendants() {
                if node.tag_name().name() == "ImageFrame" && node.has_attribute("id") {
                    let id = node.attribute("id").unwrap_or("").to_string();
                    let href = node.children().find(|n| n.has_attribute("href")).and_then(|n| n.attribute("href"));
                    match href {
                        Some(h) => image_links.push((id, PathBuf::from(h))),
                        None => {
                            report.losses.push(ImportLoss::new(LossKind::Other, 1, false, format!("image frame {} has no link", id)));
                        }
                    }
                }
            }
        }
    }
    Ok(image_links)
}

fn detect_unmapped_features(entries: &HashMap<String, Vec<u8>>, report: &mut ImportReport) {
    for (name, data) in entries {
        if name.ends_with(".xml") {
            let content = String::from_utf8_lossy(data);
            if content.contains("Table") || content.contains("<table") {
                report.losses.push(ImportLoss::new(LossKind::Other, 1, false, format!("tables in {} not mapped", name)));
            }
            if content.contains("Footnote") || content.contains("<footnote") {
                report.losses.push(ImportLoss::new(LossKind::Other, 1, false, format!("footnotes in {} not mapped", name)));
            }
        }
    }
}

pub fn import_idml(path: &Path) -> Result<(Document, ImportReport), IoError> {
    let entries = read_zip_entries(path)?;
    let package = read_idml(path)?;

    let mut report = ImportReport::default();
    let mut doc = Document::new([800.0, 600.0], dtool_core::document::Units::Px, 72.0, 1);

    for spread in &package.spreads {
        let _spread_name = spread.clone();
        doc.push_page([800.0, 600.0]);
    }

    for master in &package.masters {
        let mut master_obj = dtool_core::Object::default();
        master_obj.kind = ObjectKind::Group;
        master_obj.name = format!("master:{}", master);
        doc.create_object(master_obj);
    }

    for style_name in &package.styles {
        let mut style_obj = dtool_core::Object::default();
        style_obj.kind = ObjectKind::Group;
        style_obj.name = format!("style:{}", style_name);
        doc.create_object(style_obj);
    }

    let text_frames = extract_text_frames(&entries, &mut report)?;
    for (id, content) in text_frames {
        if !content.is_empty() {
            let mut obj = dtool_core::Object::default();
            obj.kind = ObjectKind::Text(content);
            obj.name = id;
            doc.create_object(obj);
            report.texts += 1;
        }
    }

    let shapes = extract_shapes(&entries, &mut report)?;
    for (id, kind) in shapes {
        let mut obj = dtool_core::Object::default();
        obj.kind = kind;
        obj.name = id;
        doc.create_object(obj);
        report.objects += 1;
    }

    let image_links = extract_image_links(&entries, &mut report)?;
    for (id, link_path) in image_links {
        if !link_path.exists() {
            report.losses.push(ImportLoss::new(
                LossKind::Other,
                1,
                false,
                format!("image frame {} missing link {}", id, link_path.display()),
            ));
        } else {
            let mut obj = dtool_core::Object::default();
            obj.kind = ObjectKind::Shape;
            obj.name = id;
            if let Ok(bytes) = std::fs::read(&link_path) {
                obj.raster_data = Some(bytes);
            }
            doc.create_object(obj);
            report.images += 1;
        }
    }

    detect_unmapped_features(&entries, &mut report);

    Ok((doc, report))
}
