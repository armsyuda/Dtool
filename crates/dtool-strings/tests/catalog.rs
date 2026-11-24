use dtool_strings::{format, missing, text};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn load_ko_json() -> HashMap<String, String> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = PathBuf::from(manifest_dir).join("ko.json");
    let content = fs::read_to_string(&path).expect("Failed to read ko.json");
    serde_json::from_str(&content).expect("ko.json must be a flat string map")
}

#[test]
fn test_ko_json_parses_as_flat_string_map() {
    let map = load_ko_json();
    assert!(!map.is_empty(), "ko.json should not be empty");
    for (key, value) in &map {
        assert!(!key.is_empty(), "Key should not be empty");
        assert!(!value.is_empty(), "Value for key '{}' should not be filled", key);
    }
}

#[test]
fn test_missing_key_returns_key() {
    let missing_key = "non.existent.key";
    let result = text(missing_key);
    assert_eq!(result, missing_key);
}

#[test]
fn test_format_fills_named_placeholder_and_leaves_unknown() {
    let key = "test.format.placeholder";
    let map = load_ko_json();
    
    if !map.contains_key(key) {
        return;
    }

    let template = &map[key];
    assert!(template.contains("{name}"), "Test key '{}' must contain {{name}}", key);

    let filled = format(key, &[("name", "Alice")]);
    assert!(filled.contains("Alice"), "Format should replace {{name}} with value");
    assert!(!filled.contains("{name}"), "Format should remove placeholder {{name}}");

    let unknown_placeholder_key = "test.format.unknown.placeholder";
    if map.contains_key(unknown_placeholder_key) {
        let template_unknown = &map[unknown_placeholder_key];
        assert!(template_unknown.contains("{unknown}"), "Test key '{}' must contain {{unknown}}", unknown_placeholder_key);
        
        let result_unknown = format(unknown_placeholder_key, &[("name", "Alice")]);
        assert!(result_unknown.contains("{unknown}"), "Format should leave unknown placeholders as they are");
    }
}

#[test]
fn test_missing_lists_only_absent_keys() {
    let map = load_ko_json();
    
    if map.is_empty() {
        return;
    }

    let existing_key = map.keys().next().unwrap().clone();
    let missing_key = "definitely.missing.key.for.test";

    assert!(!map.contains_key(missing_key), "Test setup: key should be absent");

    let result_missing = missing(&[existing_key.as_str(), missing_key]);
    
    assert_eq!(result_missing.len(), 1, "missing() should return only the absent key");
    assert_eq!(result_missing[0], missing_key, "missing() should return the correct absent key");
}
