use dtool_core::colour::{Colour as ManagedColour, ColourModel, ProfileStore};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, Paint};
use dtool_core::{Command, Document, Scope, Workspace};
use serde_json::{json, Value};

fn new_doc(workspace: &mut Workspace, mut params: Value) -> Result<Value, String> {
    for (key, value) in [("width", json!(100.0)), ("height", json!(100.0)), ("units", json!("Px")), ("dpi", json!(72.0)), ("pages", json!(1))] {
        if params.get(key).is_none() { params[key] = value; }
    }
    workspace.run(Scope::default(), Command::with_params("doc.new", params)).1.map_err(|e| e.code)
}

#[test]
fn cmyk_8_defaults_to_fogra39l() {
    let mut workspace = Workspace::default();
    let reply = new_doc(&mut workspace, json!({"colour_format": "CMYK/8", "width": 100.0, "height": 100.0})).unwrap();
    assert_eq!(reply["colour"]["format"], json!("CMYK/8"));
    assert_eq!(reply["colour"]["profile"], json!(ProfileStore::FOGRA39L));
}

#[test]
fn every_listed_format_parses_and_bad_labels_fail() {
    for label in ["RGBA/8", "RGBA/16", "RGBA/32", "CMYK/8", "LAB/16", "GREY/8", "Grey/16"] {
        assert!(dtool_core::commands::document_colour::parse_colour_format(label).is_some(), "{label}");
    }
    assert!(dtool_core::commands::document_colour::parse_colour_format("CMYK/32").is_none());
    let mut workspace = Workspace::default();
    assert_eq!(new_doc(&mut workspace, json!({"colour_format": "HSV/8"})), Err("INVALID_PARAMS".into()));
    let lab = new_doc(&mut workspace, json!({"colour_format": "LAB/16"})).unwrap();
    assert_eq!(lab["colour"]["profile"], json!(ProfileStore::LAB_D50));
}

#[test]
fn missing_or_mismatched_profile_is_refused() {
    let mut workspace = Workspace::default();
    let before = workspace.contexts().len();
    assert_eq!(new_doc(&mut workspace, json!({"colour_format": "CMYK/8", "colour_profile": "No Such Press Profile"})), Err("PROFILE_NOT_FOUND".into()));
    assert_eq!(new_doc(&mut workspace, json!({"colour_format": "CMYK/8", "colour_profile": ProfileStore::SRGB})), Err("PROFILE_MODEL_MISMATCH".into()));
    assert_eq!(workspace.contexts().len(), before);
}

#[test]
fn cmyk_fill_round_trips_byte_equal() {
    let mut workspace = Workspace::default();
    let reply = new_doc(&mut workspace, json!({"colour_format": "CMYK/8"})).unwrap();
    let context = dtool_core::ContextId(reply["context_id"].as_u64().unwrap());
    let session = &mut workspace.get_mut(context).unwrap().session;
    let colour = Colour::from_managed(ManagedColour::cmyk(10.0, 20.0, 30.0, 40.0, 1.0));
    let id = session.document.create_object(Object { name: "box".into(), size: [10.0, 10.0], fill: Paint::Solid { colour: colour.clone() }, ..Object::default() });
    let saved = serde_json::to_vec(&session.document).unwrap();
    let loaded: Document = serde_json::from_slice(&saved).unwrap();
    assert_eq!(serde_json::to_vec(&loaded).unwrap(), saved);
    let Paint::Solid { colour: back } = &loaded.get_object(ObjectId(id.0)).unwrap().fill else { panic!("fill") };
    assert_eq!(back.model, ColourModel::Cmyk);
    assert_eq!(back.values, [10.0, 20.0, 30.0, 40.0]);
    assert_eq!(*back, colour);
    assert_eq!(loaded.colour.profile, ProfileStore::FOGRA39L);
}
