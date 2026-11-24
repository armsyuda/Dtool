use dtool_core::document::{Document, Units};
use dtool_core::{Command, Session};
use serde_json::{json, Value};

fn run(session: &mut Session, name: &str, params: Value) -> Value {
    session.run(&Command::with_params(name, params)).unwrap_or_else(|error| panic!("{name} failed: {error:?}"))
}

#[test]
fn ui_panel_set_swatches_view_sort_search() {
    let mut session = Session::new(Document::new([100.0, 100.0], Units::Pt, 72.0, 1));
    run(&mut session, "palette.create", json!({ "name": "Fixture" }));
    let mut ids = Vec::new();
    for (name, colour) in [("zeta red", "#FF0000"), ("Alpha blue", "#0000FF"), ("bright red", "#FF3333")] {
        ids.push(run(&mut session, "swatch.add", json!({ "name": name, "colour": colour, "palette": "Fixture" }))["id"].as_u64().unwrap());
    }
    let stored: Vec<u64> = session.document.swatches.iter().map(|swatch| swatch.id).collect();
    let version = session.document.version;
    let params = json!({ "panel": "swatches", "set": { "scope": "document", "palette": "Fixture", "view": "list", "sort": "name", "search": "red" } });
    let reply = run(&mut session, "ui.panel.set", params.clone());
    assert_eq!(reply["panel"], "swatches");
    assert_eq!(reply["state"]["scope"], "document");
    assert_eq!(reply["state"]["palette"], "Fixture");
    assert_eq!(reply["state"]["view"], "list");
    assert_eq!(reply["state"]["sort"], "name");
    assert_eq!(reply["state"]["search"], "red");
    assert_eq!(reply["order"], json!([ids[2], ids[0]]));
    assert_eq!(run(&mut session, "ui.panel.set", params)["order"], reply["order"]);
    assert_eq!(session.document.swatches.iter().map(|swatch| swatch.id).collect::<Vec<_>>(), stored);
    assert_eq!(session.document.version, version);
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(session.document.swatches.len(), 2);
    let bad = session.run(&Command::with_params("ui.panel.set", json!({ "panel": "swatches", "set": { "view": "cards" } })));
    assert!(bad.is_err());
}