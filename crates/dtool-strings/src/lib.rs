use std::collections::HashMap;
use std::sync::OnceLock;

const KO_TABLE: &str = include_str!("../ko.json");

fn table() -> &'static HashMap<String, String> {
    static TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    TABLE.get_or_init(|| serde_json::from_str(KO_TABLE).unwrap_or_default())
}

pub fn text(key: &str) -> &str {
    match table().get(key) {
        Some(value) => value.as_str(),
        None => key,
    }
}

pub fn has(key: &str) -> bool {
    table().contains_key(key)
}

pub fn format(key: &str, args: &[(&str, &str)]) -> String {
    let mut result = text(key).to_string();
    for (name, value) in args {
        let pattern = format!("{{{}}}", name);
        result = result.replace(&pattern, value);
    }
    result
}

pub fn missing(keys: &[&str]) -> Vec<String> {
    keys.iter()
        .copied()
        .filter(|key| !has(*key))
        .map(|key| key.to_string())
        .collect()
}

pub fn require(keys: &[&str]) -> Result<(), Vec<String>> {
    let gaps = missing(keys);
    if gaps.is_empty() { Ok(()) } else { Err(gaps) }
}
