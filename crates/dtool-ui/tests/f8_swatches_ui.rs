use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, Paint};
use dtool_core::Session;
use serde_json::Value;

fn add_to_swatches(rows: &Value) -> Option<&Value> {
    rows.as_array()?.iter().find(|row| row["id"] == "context.add_to_swatches")
}

#[test]
fn context_add_to_swatches_is_wired_and_enabled() {
    let mut rows = dtool_ui::context_menu::query("designer", "Object");
    let item = add_to_swatches(&rows).expect("context menu has add to swatches");
    assert_eq!(item["command"], dtool_ui::context_menu::ADD_TO_SWATCHES);
    assert_eq!(item["planned"], false);
    let mut document = Document::new([100.0, 100.0], Units::Pt, 72.0, 1);
    document.objects.push(Object { id: ObjectId(1), fill: Paint::Solid { colour: Colour::new(10, 20, 30, 255) }, ..Object::default() });
    let mut session = Session::new(document);
    session.selection.ids = vec![ObjectId(1)];
    dtool_core::commands::menus::resolve_items(&mut rows, &session);
    let item = add_to_swatches(&rows).unwrap();
    assert_eq!(item["enabled"], true);
    assert_eq!(item["planned"], false);
    session.run_named(dtool_ui::context_menu::ADD_TO_SWATCHES, serde_json::json!({})).unwrap();
    assert_eq!(session.document.swatches[0].colour, Colour::new(10, 20, 30, 255));
}