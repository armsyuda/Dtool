use std::fs;
use std::path::Path;

use serde_json::{json, Value};

use crate::native::{IoError, IoErrorKind};

fn map_fs_error(err: std::io::Error) -> IoError {
    match err.kind() {
        std::io::ErrorKind::NotFound => IoError::new(IoErrorKind::NotFound),
        std::io::ErrorKind::PermissionDenied => IoError::new(IoErrorKind::Permission),
        _ => IoError::from_io(&err),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddonKind {
    Brushes,
    Assets,
    Swatches,
    Styles,
}

impl AddonKind {
    pub fn extension(self) -> &'static str {
        match self {
            AddonKind::Brushes => "afbrushes",
            AddonKind::Assets => "afassets",
            AddonKind::Swatches => "afpalette",
            AddonKind::Styles => "afstyles",
        }
    }

    pub fn import_extensions(self) -> Vec<&'static str> {
        match self {
            AddonKind::Brushes => vec!["afbrushes", "abr"],
            AddonKind::Assets => vec!["afassets"],
            AddonKind::Swatches => vec!["afpalette", "ase"],
            AddonKind::Styles => vec!["afstyles"],
        }
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "afbrushes" | "abr" => Some(AddonKind::Brushes),
            "afassets" => Some(AddonKind::Assets),
            "afpalette" | "ase" => Some(AddonKind::Swatches),
            "afstyles" => Some(AddonKind::Styles),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AddonPack {
    pub kind: AddonKind,
    pub name: String,
    pub items: Vec<Value>,
}

#[derive(Debug, Clone)]
pub struct StyleConflict {
    pub incoming: String,
    pub rename_to: String,
    pub conflicts: bool,
}

fn read_json(path: &Path) -> Result<Value, IoError> {
    let data = fs::read_to_string(path).map_err(map_fs_error)?;
    let value: Value = serde_json::from_str(&data).map_err(|_| IoError::new(IoErrorKind::InvalidFormat))?;
    if !value.is_object() && !value.is_array() {
        return Err(IoError::new(IoErrorKind::InvalidFormat));
    }
    Ok(value)
}

fn write_json(path: &Path, value: &Value) -> Result<(), IoError> {
    let data = serde_json::to_string_pretty(value).map_err(|_| IoError::new(IoErrorKind::InvalidFormat))?;
    fs::write(path, data).map_err(map_fs_error)
}

fn parse_afbrushes(data: &Value) -> Result<AddonPack, IoError> {
    let name = data.get("name").and_then(|v| v.as_str()).unwrap_or("Brushes").to_string();
    let items = data.get("items").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    Ok(AddonPack { kind: AddonKind::Brushes, name, items })
}

fn parse_abr(data: &Value) -> Result<AddonPack, IoError> {
    let name = data.get("name").and_then(|v| v.as_str()).unwrap_or("Photoshop Brushes").to_string();
    let raw_items = data.get("items").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let items: Vec<Value> = raw_items.into_iter().map(|item| {
        json!({
            "name": item.get("name").and_then(|v| v.as_str()).unwrap_or("Unnamed"),
            "type": "brush",
            "source": "abr",
            "data": item
        })
    }).collect();
    Ok(AddonPack { kind: AddonKind::Brushes, name, items })
}

fn parse_afpalette(data: &Value) -> Result<AddonPack, IoError> {
    let name = data.get("name").and_then(|v| v.as_str()).unwrap_or("Swatches").to_string();
    let raw_items = data.get("items").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let items: Vec<Value> = raw_items.into_iter().map(|item| {
        json!({
            "name": item.get("name").and_then(|v| v.as_str()).unwrap_or("Unnamed"),
            "type": "swatch",
            "source": "afpalette",
            "color": item.get("color").cloned().unwrap_or(json!(null)),
            "spot": item.get("spot").and_then(|v| v.as_bool()).unwrap_or(false)
        })
    }).collect();
    Ok(AddonPack { kind: AddonKind::Swatches, name, items })
}

fn parse_ase(data: &Value) -> Result<AddonPack, IoError> {
    let name = data.get("name").and_then(|v| v.as_str()).unwrap_or("Adobe Swatch Exchange").to_string();
    let raw_items = data.get("items").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let items: Vec<Value> = raw_items.into_iter().map(|item| {
        let color_type = item.get("colorType").and_then(|v| v.as_str()).unwrap_or("RGB");
        let color_data = match color_type {
            "CMYK" => json!({
                "type": "cmyk",
                "c": item.get("c").and_then(|v| v.as_f64()).unwrap_or(0.0),
                "m": item.get("m").and_then(|v| v.as_f64()).unwrap_or(0.0),
                "y": item.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0),
                "k": item.get("k").and_then(|v| v.as_f64()).unwrap_or(0.0)
            }),
            "Lab" => json!({
                "type": "lab",
                "l": item.get("l").and_then(|v| v.as_f64()).unwrap_or(0.0),
                "a": item.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0),
                "b": item.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0)
            }),
            _ => json!({
                "type": "rgb",
                "r": item.get("r").and_then(|v| v.as_f64()).unwrap_or(0.0),
                "g": item.get("g").and_then(|v| v.as_f64()).unwrap_or(0.0),
                "b": item.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0)
            })
        };
        json!({
            "name": item.get("name").and_then(|v| v.as_str()).unwrap_or("Unnamed"),
            "type": "swatch",
            "source": "ase",
            "color": color_data,
            "spot": item.get("spot").and_then(|v| v.as_bool()).unwrap_or(false)
        })
    }).collect();
    Ok(AddonPack { kind: AddonKind::Swatches, name, items })
}

pub fn import_addon(path: &Path) -> Result<AddonPack, IoError> {
    let kind = AddonKind::from_path(path).ok_or_else(|| IoError::new(IoErrorKind::UnsupportedProfile))?;

    let data = read_json(path)?;

    match kind {
        AddonKind::Brushes => {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            if ext == "abr" {
                parse_abr(&data)
            } else {
                parse_afbrushes(&data)
            }
        }
        AddonKind::Assets => {
            let name = data.get("name").and_then(|v| v.as_str()).unwrap_or("Assets").to_string();
            let items = data.get("items").and_then(|v| v.as_array()).cloned().unwrap_or_default();
            Ok(AddonPack { kind, name, items })
        }
        AddonKind::Swatches => {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            if ext == "ase" {
                parse_ase(&data)
            } else {
                parse_afpalette(&data)
            }
        }
        AddonKind::Styles => {
            let name = data.get("name").and_then(|v| v.as_str()).unwrap_or("Styles").to_string();
            let items = data.get("items").and_then(|v| v.as_array()).cloned().unwrap_or_default();
            Ok(AddonPack { kind, name, items })
        }
    }
}

pub fn export_addon(pack: &AddonPack, path: &Path) -> Result<(), IoError> {
    let value = json!({
        "kind": format!("{:?}", pack.kind),
        "name": pack.name,
        "items": pack.items
    });
    write_json(path, &value)
}

pub fn merge_addon(existing: &AddonPack, incoming: &AddonPack) -> AddonPack {
    let mut merged_items = existing.items.clone();
    let existing_names: Vec<String> = merged_items.iter().filter_map(|item| item.get("name").and_then(|v| v.as_str()).map(String::from)).collect();

    for item in &incoming.items {
        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("Unnamed");
        if existing_names.contains(&name.to_string()) {
            let mut renamed_item = item.clone();
            renamed_item["name"] = json!(format!("{} (2)", name));
            merged_items.push(renamed_item);
        } else {
            merged_items.push(item.clone());
        }
    }

    AddonPack {
        kind: existing.kind,
        name: existing.name.clone(),
        items: merged_items,
    }
}

pub fn detect_style_conflicts(existing: &AddonPack, incoming: &AddonPack) -> Vec<StyleConflict> {
    let existing_names: Vec<String> = existing.items.iter().filter_map(|item| item.get("name").and_then(|v| v.as_str()).map(String::from)).collect();
    let mut conflicts = Vec::new();

    for item in &incoming.items {
        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("Unnamed");
        if existing_names.contains(&name.to_string()) {
            conflicts.push(StyleConflict {
                incoming: name.to_string(),
                rename_to: format!("{} (2)", name),
                conflicts: true,
            });
        } else {
            conflicts.push(StyleConflict {
                incoming: name.to_string(),
                rename_to: name.to_string(),
                conflicts: false,
            });
        }
    }

    conflicts
}

pub fn apply_style_conflicts(incoming: &AddonPack, conflicts: &[StyleConflict]) -> AddonPack {
    let mut items = Vec::new();
    for (item, conflict) in incoming.items.iter().zip(conflicts.iter()) {
        if conflict.conflicts {
            let mut renamed_item = item.clone();
            renamed_item["name"] = json!(conflict.rename_to);
            items.push(renamed_item);
        } else {
            items.push(item.clone());
        }
    }

    AddonPack {
        kind: incoming.kind,
        name: incoming.name.clone(),
        items,
    }
}

