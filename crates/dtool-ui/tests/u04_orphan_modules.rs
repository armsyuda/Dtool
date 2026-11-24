use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

const NOT_COMPILED: [&str; 23] = [
    "af_report_dialog.rs",
    "cad_import_dialog.rs",
    "canvas/path_text_handles.rs",
    "colour_settings.rs",
    "data_merge_bar.rs",
    "dialogs/data_merge_manager.rs",
    "dialogs/doc_metadata.rs",
    "menus/caption.rs",
    "ocio.rs",
    "overlays/measure.rs",
    "paint_context.rs",
    "panels/appearance.rs",
    "panels/assets.rs",
    "panels/data_merge.rs",
    "panels/metadata.rs",
    "panels/pinning.rs",
    "pdf_import_dialog.rs",
    "resize_canvas_dialog.rs",
    "resize_pixel_art_dialog.rs",
    "settings/autocorrect_old.rs",
    "shape_context.rs",
    "studio_editor.rs",
    "style_conflict_dialog.rs",
];

fn src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn normal(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

fn rel(path: &Path) -> String {
    normal(path).strip_prefix(normal(&src())).unwrap().to_string_lossy().replace('\\', "/")
}

fn mod_decl(line: &str) -> Option<(String, bool)> {
    let mut rest = line.trim_start();
    if let Some(after) = rest.strip_prefix("pub") {
        let after = after.trim_start();
        rest = if after.starts_with('(') { after.split_once(')')?.1.trim_start() } else { after };
    }
    let rest = rest.strip_prefix("mod ")?.trim_start();
    let name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
    if name.is_empty() {
        return None;
    }
    let tail = rest[name.len()..].trim_start();
    if tail.starts_with(';') {
        Some((name, false))
    } else if tail.starts_with('{') {
        Some((name, true))
    } else {
        None
    }
}

fn path_attr(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix("#[path")?.trim_start().strip_prefix('=')?.trim_start().strip_prefix('"')?;
    rest.split_once('"').map(|(p, _)| p.to_string())
}

fn children(file: &Path) -> Vec<PathBuf> {
    let text = std::fs::read_to_string(file).unwrap();
    let dir = file.parent().unwrap().to_path_buf();
    let stem = file.file_stem().unwrap().to_string_lossy().to_string();
    let base = if matches!(stem.as_str(), "lib" | "main" | "mod") { dir.clone() } else { dir.join(&stem) };
    let mut out = Vec::new();
    let mut stack: Vec<(usize, PathBuf)> = vec![(usize::MAX, base)];
    let mut pending = None;
    for line in text.lines() {
        let indent = line.len() - line.trim_start().len();
        if line.trim() == "}" && stack.len() > 1 && stack.last().unwrap().0 == indent {
            stack.pop();
            continue;
        }
        if let Some(p) = path_attr(line) {
            pending = Some(p);
            continue;
        }
        let current = stack.last().unwrap().1.clone();
        match mod_decl(line) {
            Some((name, false)) => {
                let target = match pending.take() {
                    Some(p) => dir.join(p),
                    None => {
                        let flat = current.join(format!("{name}.rs"));
                        if flat.is_file() { flat } else { current.join(&name).join("mod.rs") }
                    }
                };
                if target.is_file() {
                    out.push(target);
                }
            }
            Some((name, true)) => {
                pending = None;
                if !line.trim_end().ends_with('}') {
                    stack.push((indent, current.join(name)));
                }
            }
            None => {
                if !line.trim_start().starts_with('#') {
                    pending = None;
                }
            }
        }
    }
    out
}

fn reachable() -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::from([src().join("lib.rs")]);
    while let Some(file) = queue.pop_front() {
        if !seen.insert(rel(&file)) {
            continue;
        }
        queue.extend(children(&file));
    }
    seen
}

fn all_sources(dir: &Path, out: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            all_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.insert(rel(&path));
        }
    }
}

#[test]
fn every_ui_source_is_compiled_or_listed() {
    let seen = reachable();
    let mut all = BTreeSet::new();
    all_sources(&src(), &mut all);
    let listed: BTreeSet<&str> = NOT_COMPILED.into_iter().collect();
    let orphans: Vec<&String> = all.iter().filter(|f| !seen.contains(*f) && !listed.contains(f.as_str())).collect();
    assert!(orphans.is_empty(), "{orphans:?}");
}

#[test]
fn listed_files_exist() {
    for file in NOT_COMPILED {
        assert!(src().join(file).is_file(), "{file}");
    }
}

#[test]
fn walker_follows_inline_and_nested_modules() {
    let seen = reachable();
    for file in ["menus/file_menu.rs", "tools/fill_tool_bar.rs", "panels/character.rs", "dialogs/locked_delete.rs", "resize_document_dialog.rs", "controls/tab_stop_editor.rs"] {
        assert!(seen.contains(file), "{file}");
    }
}