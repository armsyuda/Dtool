use std::path::Path;

fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn every_multiline_input_is_marked() {
    let mut files = Vec::new();
    rust_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
    let mut bad = Vec::new();
    for file in files {
        let src = std::fs::read_to_string(&file).unwrap();
        if src.contains("text_edit_multiline(") {
            bad.push(format!("{} raw text_edit_multiline", file.display()));
        }
        let raw = src.matches("TextEdit::multiline(").count();
        if raw > 0 && src.matches("mark_multiline(").count() < raw {
            bad.push(format!("{} TextEdit::multiline without mark_multiline", file.display()));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}
