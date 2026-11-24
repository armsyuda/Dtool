use dtool_core::document::Units;
use dtool_core::serde_json::json;
use dtool_core::{Document, Session, Story, StoryId};
use dtool_text::fonts::FontLibrary;
use std::path::PathBuf;

#[test]
fn package_copies_the_real_font_files_a_document_uses() {
    assert!(dtool_text::text_host::install());
    let lib = FontLibrary::shared();
    let Some(face) = lib.families().into_iter().flat_map(|family| lib.faces(family)).find(|face| face.path.is_file()) else {
        eprintln!("no system font files");
        return;
    };
    let family = face.family.clone();
    let style = face.style.clone();
    let source = face.path.clone();
    let reply = dtool_text::font_preview::font_files(&lib, &json!({"faces": [{"family": family, "style": style}, {"family": "No Such Family Zz", "style": ""}]}));
    assert_eq!(reply["files"][0]["paths"][0].as_str().map(PathBuf::from), Some(source.clone()));
    assert_eq!(reply["files"][1]["paths"], json!([]));

    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c09b_package_fonts_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let mut document = Document::new([100.0, 100.0], Units::Px, 72.0, 1);
    document.stories.push(Story { id: StoryId(1), text: "Hi".into(), ranges: vec![json!({"start": 0, "end": 2, "family": family, "style": style})], specials: Vec::new(), paras: Vec::new() });
    let mut session = Session::new(document);
    let folder = root.join("pkg");
    let report = session.run_named("doc.package.save", json!({"folder": folder, "fonts": true, "zip": true})).unwrap();
    assert_eq!(report["fonts"][0]["status"], json!("copied"));
    let copied = folder.join("Fonts").join(source.file_name().unwrap());
    assert_eq!(std::fs::metadata(&copied).unwrap().len(), std::fs::metadata(&source).unwrap().len());
    assert!(root.join("pkg.zip").is_file());
    session.document.stories.clear();
    let again = session.run_named("doc.package.resave", json!({"folder": folder, "fonts": true})).unwrap();
    assert!(!copied.exists());
    assert!(source.is_file());
    assert_eq!(again["removed"].as_array().unwrap().len(), 1);
}