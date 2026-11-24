use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const KEY_FUNCTIONS: &[&str] = &["text", "format", "has", "require"];

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn read_literal(rest: &str) -> Option<String> {
    let mut chars = rest.chars();
    if chars.next()? != '"' {
        return None;
    }
    let mut out = String::new();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => out.push(chars.next()?),
            _ => out.push(c),
        }
    }
    None
}

pub fn collect_text_keys(content: &str) -> Vec<(String, usize)> {
    let mut keys = Vec::new();
    for (line_index, line) in content.lines().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        for name in KEY_FUNCTIONS {
            let pattern = format!("{name}(");
            let mut from = 0;
            while let Some(found) = line[from..].find(&pattern) {
                let start = from + found;
                from = start + pattern.len();
                let before = line[..start].chars().next_back();
                let prefixed = line[..start].ends_with("dtool_strings::");
                if !prefixed && before.is_some_and(|c| is_ident(c) || c == '.' || c == ':') {
                    continue;
                }
                let mut rest = line[from..].trim_start();
                rest = rest.strip_prefix("&[").map(str::trim_start).unwrap_or(rest);
                if let Some(key) = read_literal(rest) {
                    if key.contains('.') && !key.contains(' ') {
                        keys.push((key, line_index + 1));
                    }
                }
            }
        }
    }
    keys
}

fn module_names(content: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        let t = t.strip_prefix("pub(crate) ").or_else(|| t.strip_prefix("pub ")).unwrap_or(t);
        if let Some(rest) = t.strip_prefix("mod ") {
            if let Some(name) = rest.strip_suffix(';') {
                let name = name.trim();
                if !name.is_empty() && name.chars().all(is_ident) {
                    names.push(name.to_string());
                }
            }
        }
    }
    names
}

pub fn compiled_rs_files(crate_src: &Path, root: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![(crate_src.join(root), crate_src.to_path_buf())];
    let mut seen = HashSet::new();
    while let Some((file, child_dir)) = stack.pop() {
        if !seen.insert(file.clone()) {
            continue;
        }
        let Ok(content) = fs::read_to_string(&file) else { continue };
        for name in module_names(&content) {
            let flat = child_dir.join(format!("{name}.rs"));
            let nested = child_dir.join(&name).join("mod.rs");
            if flat.exists() {
                stack.push((flat, child_dir.join(&name)));
            } else if nested.exists() {
                stack.push((nested, child_dir.join(&name)));
            }
        }
        out.push(file);
    }
    out
}

#[test]
fn test_b01_text_keys_exist_in_ko_json() {
    let strings_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let crates = strings_dir.parent().unwrap();
    let ko_content = fs::read_to_string(strings_dir.join("ko.json")).unwrap();
    let table: HashMap<String, String> = serde_json::from_str(&ko_content).unwrap();
    let mut files = compiled_rs_files(&crates.join("dtool-ui").join("src"), "lib.rs");
    files.extend(compiled_rs_files(&crates.join("dtool-app").join("src"), "main.rs"));
    let mut missing: Vec<String> = Vec::new();
    for file in files {
        let content = fs::read_to_string(&file).unwrap();
        for (key, line) in collect_text_keys(&content) {
            if !table.contains_key(&key) {
                missing.push(format!("{key} ({}:{line})", file.display()));
            }
        }
    }
    missing.sort();
    missing.dedup();
    assert!(missing.is_empty(), "Missing keys in ko.json: {:?}", missing);
}

#[test]
fn literal_outside_module_tree_is_ignored() {
    let dir = std::env::temp_dir().join(format!("dtool_b01_tree_{}", std::process::id()));
    let _ = fs::create_dir_all(dir.join("sub"));
    fs::write(dir.join("lib.rs"), "pub mod a;\nmod sub;\n").unwrap();
    fs::write(dir.join("a.rs"), "fn f() { text(\"menu.file\"); }\n").unwrap();
    fs::write(dir.join("sub.rs"), "mod b;\n").unwrap();
    fs::write(dir.join("sub").join("b.rs"), "fn g() {}\n").unwrap();
    fs::write(dir.join("orphan.rs"), "fn h() { text(\"zz.not.real\"); }\n").unwrap();
    let files = compiled_rs_files(&dir, "lib.rs");
    assert_eq!(files.len(), 4);
    assert!(files.iter().all(|f| !f.ends_with("orphan.rs")));
}

#[test]
fn only_first_literal_of_key_functions() {
    let keys = collect_text_keys("let a = text(\"menu.file\"); let b = \"tool.move\"; ui.text(\"x.y\"); dtool_strings::format(\"status.zoom\", &[(\"z\", \"1.0\")]);");
    let names: Vec<_> = keys.into_iter().map(|k| k.0).collect();
    assert_eq!(names, vec!["menu.file".to_string(), "status.zoom".to_string()]);
}
