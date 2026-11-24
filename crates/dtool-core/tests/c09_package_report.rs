use std::path::PathBuf;
use dtool_core::{Document, Session};
use dtool_core::document::Units;
use dtool_core::resources::{ImageFormat, ImageResource};
use serde_json::json;

fn root(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c09_package_{}_{name}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn package_reports_each_resource_and_lists_missing_files() {
    let root = root("report");
    let present = root.join("present.png");
    std::fs::write(&present, b"png").unwrap();
    let mut document = Document::new([100.0, 100.0], Units::Px, 72.0, 1);
    let linked = document.add_image_resource(ImageResource::new_linked(present.clone(), ImageFormat::Png, [1, 1], 72.0));
    let gone = document.add_image_resource(ImageResource::new_linked(root.join("gone.png"), ImageFormat::Png, [1, 1], 72.0));
    let embedded = document.add_image_resource(ImageResource::new_embedded(ImageFormat::Png, [1, 1], 72.0, vec![1]));
    let mut session = Session::new(document);
    let folder = root.join("out");
    let report = session.run_named("doc.package.save", json!({"folder": folder})).unwrap();
    let status = |id: u64| report["items"].as_array().unwrap().iter().find(|item| item["resource"] == json!(id)).unwrap()["status"].clone();
    assert_eq!(status(linked.0), json!("copied"));
    assert_eq!(status(gone.0), json!("missing"));
    assert_eq!(status(embedded.0), json!("embedded"));
    assert_eq!(report["missing"].as_array().unwrap().len(), 1);
    assert_eq!(report["fonts"], json!("skipped"));
    let again = session.run_named("doc.package.save", json!({"folder": folder, "images": false})).unwrap();
    assert_eq!(again["items"].as_array().unwrap().iter().filter(|item| item["status"] == json!("skipped")).count(), 2);
    let error = session.run_named("doc.package.save", json!({"folder": folder, "fonts": true})).unwrap_err();
    assert_eq!(error.code, "PACKAGE_FONT_SOURCES_UNAVAILABLE");
}
