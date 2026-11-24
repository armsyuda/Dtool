use std::collections::{BTreeSet, HashMap};
use std::sync::{Mutex, OnceLock};

use egui::Color32;

const TOKENS_JSON: &str = include_str!("../tokens.json");

fn table() -> &'static HashMap<String, serde_json::Value> {
    static TABLE: OnceLock<HashMap<String, serde_json::Value>> = OnceLock::new();
    TABLE.get_or_init(|| serde_json::from_str(TOKENS_JSON).unwrap_or_default())
}


fn seen_misses() -> &'static Mutex<BTreeSet<String>> {
    static SEEN: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    SEEN.get_or_init(|| Mutex::new(BTreeSet::new()))
}

fn record_miss(key: &str) {
    if let Ok(mut seen) = seen_misses().lock() {
        seen.insert(key.to_string());
    }
}

pub fn missing_tokens_seen() -> Vec<String> {
    seen_misses()
        .lock()
        .map(|seen| seen.iter().cloned().collect())
        .unwrap_or_default()
}

pub fn has(key: &str) -> bool {
    table().contains_key(key)
}

pub fn missing(keys: &[&str]) -> Vec<String> {
    keys.iter()
        .filter(|key| !has(key))
        .map(|key| key.to_string())
        .collect()
}

fn lookup_in<'a>(key: &str, kind: &str, style: &str) -> Option<&'a serde_json::Value> {
    let table = table();
    let candidates = [
        key.to_string(),
        format!("{kind}.{key}"),
        format!("{kind}.{style}.{key}"),
        format!("color.{style}.{key}"),
    ];
    for cand in candidates {
        if let Some(v) = table.get(&cand) {
            return Some(v);
        }
    }
    None
}

fn lookup<'a>(key: &str, kind: &str) -> Option<&'a serde_json::Value> {
    lookup_in(key, kind, crate::theme::current_style_name())
}

pub fn color(key: &str) -> Color32 {
    color_in(crate::theme::current_style_name(), key)
}

pub fn color_in(style: &str, key: &str) -> Color32 {
    match lookup_in(key, "color", style) {
        Some(serde_json::Value::String(s)) => parse_hex(s).unwrap_or_else(|| {
            record_miss(key);
            Color32::from_rgb(255, 0, 255)
        }),
        Some(_) => {
            record_miss(key);
            Color32::from_rgb(255, 0, 255)
        }
        None => {
            record_miss(key);
            Color32::from_rgb(255, 0, 255)
        }
    }
}

pub fn number(key: &str) -> f32 {
    match lookup(key, number_kind(key)) {
        Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or(0.0) as f32,
        Some(serde_json::Value::String(s)) => s.parse().unwrap_or(0.0),
        Some(_) => {
            record_miss(key);
            0.0
        }
        None => {
            record_miss(key);
            0.0
        }
    }
}

fn number_kind(key: &str) -> &'static str {
    if key.starts_with("space") || key.contains("space.") {
        "space"
    } else if key.starts_with("radius") {
        "radius"
    } else if key.starts_with("stroke") {
        "stroke"
    } else if key.starts_with("font.size") || key.starts_with("size.") {
        "font.size"
    } else if key.starts_with("icon") {
        "icon.size"
    } else if key.starts_with("bar") || key.starts_with("panel") || key.starts_with("dialog") {
        "bar"
    } else {
        "space"
    }
}

pub fn space(key: &str) -> f32 {
    let full = if key.starts_with("space.") {
        key.to_string()
    } else {
        format!("space.{key}")
    };
    number(&full)
}

pub fn radius(key: &str) -> f32 {
    let full = if key.starts_with("radius.") {
        key.to_string()
    } else {
        format!("radius.{key}")
    };
    number(&full)
}

pub fn stroke_width(key: &str) -> f32 {
    let full = if key.starts_with("stroke.") {
        key.to_string()
    } else {
        format!("stroke.{key}")
    };
    number(&full)
}

pub fn font_size(key: &str) -> f32 {
    let full = if key.starts_with("font.size.") {
        key.to_string()
    } else {
        format!("font.size.{key}")
    };
    number(&full)
}

pub fn size(key: &str) -> f32 {
    if has(key) {
        return number(key);
    }
    for prefix in ["icon.size.", "bar.", "panel.", "dialog.", "font.size."] {
        let full = format!("{prefix}{key}");
        if has(&full) {
            return number(&full);
        }
    }
    number(key)
}

pub fn font_family(key: &str) -> String {
    let full = if key.starts_with("font.family.") {
        key.to_string()
    } else {
        format!("font.family.{key}")
    };
    let table = table();
    let resolved = table
        .get(&full)
        .or_else(|| table.get(key))
        .and_then(|v| v.as_str());
    resolved.unwrap_or(key).to_string()
}

pub fn parse_hex(s: &str) -> Option<Color32> {
    let t = s.trim().trim_start_matches('#');
    if !t.is_ascii() { return None; }
    let (r, g, b, a) = match t.len() {
        3 => {
            let r = u8::from_str_radix(&t[0..1].repeat(2), 16).ok()?;
            let g = u8::from_str_radix(&t[1..2].repeat(2), 16).ok()?;
            let b = u8::from_str_radix(&t[2..3].repeat(2), 16).ok()?;
            (r, g, b, 255)
        }
        6 => {
            let r = u8::from_str_radix(&t[0..2], 16).ok()?;
            let g = u8::from_str_radix(&t[2..4], 16).ok()?;
            let b = u8::from_str_radix(&t[4..6], 16).ok()?;
            (r, g, b, 255)
        }
        8 => {
            let r = u8::from_str_radix(&t[0..2], 16).ok()?;
            let g = u8::from_str_radix(&t[2..4], 16).ok()?;
            let b = u8::from_str_radix(&t[4..6], 16).ok()?;
            let a = u8::from_str_radix(&t[6..8], 16).ok()?;
            (r, g, b, a)
        }
        _ => return None,
    };
    Some(Color32::from_rgba_unmultiplied(r, g, b, a))
}
