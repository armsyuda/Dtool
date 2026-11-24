use dtool_core::document::{Document, DocumentSetup, SetupMargins, Units};
use dtool_core::{Command, Session};
use serde_json::{json, Value};

fn custom_setup() -> DocumentSetup {
    DocumentSetup {
        landscape: true,
        facing_pages: false,
        arrange_vertical: true,
        start_on_right: false,
        reflow_pages: true,
        transparent_background: true,
        margins: SetupMargins { enabled: true, left: 18.0, right: 24.0, top: 12.0, bottom: 30.0, linked: false },
        bleed: SetupMargins { enabled: true, left: 3.0, right: 3.0, top: 3.0, bottom: 3.0, linked: true },
        page_origin: [0.0, 0.0],
    }
}

fn run(session: &mut Session, name: &str, params: Value) -> Value {
    session.run(&Command::with_params(name, params)).unwrap_or_else(|error| panic!("{name} failed: {error:?}"))
}

#[test]
fn setup_fields_round_trip_through_document_json() {
    let mut doc = Document::new([720.0, 360.0], Units::Pt, 72.0, 2);
    doc.setup = custom_setup();
    let text = serde_json::to_string(&doc).unwrap();
    let loaded: Document = serde_json::from_str(&text).unwrap();
    assert_eq!(loaded.setup, custom_setup());
    assert_eq!(loaded.pages.len(), 2);
}

#[test]
fn old_file_without_setup_loads_defaults() {
    let doc = Document::new([720.0, 360.0], Units::Pt, 72.0, 1);
    let mut value = serde_json::to_value(&doc).unwrap();
    value.as_object_mut().unwrap().remove("setup");
    let loaded: Document = serde_json::from_value(value).unwrap();
    assert_eq!(loaded.setup, DocumentSetup::default());
    let mut partial = serde_json::to_value(&doc).unwrap();
    partial["setup"] = json!({ "facing_pages": false, "landscape": null, "bleed": { "left": 2.0 } });
    let loaded: Document = serde_json::from_value(partial).unwrap();
    assert!(!loaded.setup.facing_pages);
    assert!(!loaded.setup.landscape);
    assert!(loaded.setup.start_on_right);
    assert_eq!(loaded.setup.bleed.left, 2.0);
    assert_eq!(loaded.setup.bleed.right, 0.0);
    assert_eq!(loaded.setup.margins, SetupMargins::default());
}

#[test]
fn setup_apply_writes_document_fields_and_undoes_once() {
    let mut session = Session::new(Document::new([720.0, 360.0], Units::Pt, 72.0, 1));
    let before = session.document.setup.clone();
    let reply = run(&mut session, "doc.setup.apply", json!({
        "landscape": true, "facing_pages": false, "arrange_vertical": true, "start_on_right": false,
        "reflow_pages": true, "transparent_background": true,
        "margins": { "enabled": true, "left": 18.0, "right": 24.0, "top": 12.0, "bottom": 30.0, "linked": false },
        "bleed": { "enabled": true, "left": 3.0, "right": 3.0, "top": 3.0, "bottom": 3.0, "linked": true }
    }));
    assert_eq!(reply["setup"]["facing_pages"], json!(false));
    assert_eq!(session.document.setup, custom_setup());
    let stored: Document = serde_json::from_str(&serde_json::to_string(&session.document).unwrap()).unwrap();
    assert_eq!(stored.setup, custom_setup());
    let query = run(&mut session, "doc.setup.query", json!({}));
    assert_eq!(query["start_on_right"], json!(false));
    assert_eq!(query["reflow_pages"], json!(true));
    assert_eq!(query["margins"]["bottom"], json!(30.0));
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(session.document.setup, before);
    assert!(!session.undo.can_undo());
    run(&mut session, "edit.redo", json!({}));
    assert_eq!(session.document.setup, custom_setup());
}

#[test]
fn preview_cancel_and_stale_token() {
    let mut session = Session::new(Document::new([720.0, 360.0], Units::Pt, 72.0, 1));
    let generation = session.generation;
    let preview = run(&mut session, "doc.setup.preview", json!({ "facing_pages": false }));
    let token = preview["token"].as_u64().unwrap();
    assert!(!session.document.setup.facing_pages);
    assert_eq!(session.generation, generation);
    run(&mut session, "doc.setup.cancel", json!({ "token": token }));
    assert!(session.document.setup.facing_pages);
    assert!(!session.undo.can_undo());
    let error = session.run(&Command::with_params("doc.setup.apply", json!({ "token": token, "reflow_pages": true }))).unwrap_err();
    assert_eq!(error.code, "STALE_PREVIEW");
    assert!(!session.document.setup.reflow_pages);
}

#[test]
fn transparent_background_toggle_is_undoable() {
    let mut session = Session::new(Document::new([720.0, 360.0], Units::Pt, 72.0, 1));
    run(&mut session, "doc.setup.transparent_background", json!({}));
    assert!(session.document.setup.transparent_background);
    run(&mut session, "doc.setup.transparent_background", json!({ "value": true }));
    assert!(session.document.setup.transparent_background);
    run(&mut session, "edit.undo", json!({}));
    run(&mut session, "edit.undo", json!({}));
    assert!(!session.document.setup.transparent_background);
}

#[test]
fn second_preview_replaces_the_first_and_apply_clears_it() {
    let mut session = Session::new(Document::new([720.0, 360.0], Units::Pt, 72.0, 1));
    let first = run(&mut session, "doc.setup.preview", json!({ "facing_pages": false }))["token"].as_u64().unwrap();
    run(&mut session, "doc.setup.cancel", json!({ "token": first }));
    let second = run(&mut session, "doc.setup.preview", json!({ "reflow_pages": true }))["token"].as_u64().unwrap();
    assert_ne!(first, second);
    assert_eq!(dtool_core::commands::doc_setup::active_preview(), Some(second));
    run(&mut session, "doc.setup.apply", json!({ "token": second, "reflow_pages": true }));
    assert_eq!(dtool_core::commands::doc_setup::active_preview(), None);
    assert!(session.document.setup.reflow_pages);
    assert!(session.document.setup.facing_pages);
}

#[test]
fn model_values_origin_and_profile_conversion() {
    let mut session = Session::new(Document::new([720.0, 360.0], Units::Pt, 72.0, 4));
    run(&mut session, "doc.setup.apply", json!({
        "facing_pages": true, "arrange_vertical": true, "start_on_right": false, "reflow_pages": true,
        "page_origin": [36.0, 18.0], "transparent_background": true
    }));
    let query = run(&mut session, "doc.setup.query", json!({}));
    assert_eq!(query["arrange_vertical"], json!(true));
    assert_eq!(query["start_on_right"], json!(false));
    assert_eq!(query["page_origin"], json!([36.0, 18.0]));
    assert_eq!(query["profile_conversion"], json!("convert"));
    let profile = session.document.colour.profile.clone();
    let renamed = format!("{profile} relabel");
    let fill = serde_json::to_value(&session.document.objects).unwrap();
    run(&mut session, "doc.setup.apply", json!({ "colour_profile": renamed, "profile_conversion": "assign" }));
    assert_eq!(session.document.colour.profile, renamed);
    assert_eq!(serde_json::to_value(&session.document.objects).unwrap(), fill);
    let error = session.run(&Command::with_params("doc.setup.apply", json!({ "colour_profile": "Missing profile", "profile_conversion": "convert" }))).unwrap_err();
    assert_eq!(error.code, "PROFILE_NOT_FOUND");
    assert_eq!(session.document.colour.profile, renamed);
    run(&mut session, "doc.setup.apply", json!({ "colour_profile": profile, "profile_conversion": "assign" }));
    assert_eq!(session.document.colour.profile, profile);
}

#[test]
fn setup_convert_changes_fill_values_and_undoes_once() {
    let mut document = Document::new([720.0, 360.0], Units::Pt, 72.0, 1);
    document.colour.profile = "sRGB IEC61966-2.1".to_string();
    document.objects.push(dtool_core::object::Object {
        id: dtool_core::object::ObjectId(5),
        fill: dtool_core::paint::Paint::Solid { colour: dtool_core::paint::Colour::new(180, 90, 45, 255) },
        ..dtool_core::object::Object::default()
    });
    let mut session = Session::new(document);
    let fill = serde_json::to_value(&session.document.objects[0].fill).unwrap();
    run(&mut session, "doc.setup.apply", json!({
        "colour_format": dtool_core::colour::ColourFormat::LAB_16.to_string(), "colour_profile": "LAB D50", "profile_conversion": "convert"
    }));
    assert_eq!(session.document.colour.profile, "LAB D50");
    assert_ne!(serde_json::to_value(&session.document.objects[0].fill).unwrap(), fill);
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(serde_json::to_value(&session.document.objects[0].fill).unwrap(), fill);
    assert_eq!(session.document.colour.profile, "sRGB IEC61966-2.1");
    assert!(!session.undo.can_undo());
}