use dtool_core::document::Units;
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::text_lines::{text_faces, text_lines};
use dtool_core::{Command, Document, Session};
use dtool_text::FontLibrary;
use dtool_core::serde_json::json;

#[test]
fn canvas_text_install_alone_wires_the_text_faces_hook() {
    assert!(dtool_text::canvas_text::install());
    let lib = FontLibrary::shared();
    let usable: Vec<String> = lib.families().into_iter().map(str::to_owned).filter(|family| lib.resolve(family, "Regular").is_some()).collect();
    let Some(a) = usable.first().cloned() else { return };
    let Some(b) = usable.iter().find(|name| **name != a).cloned() else { return };
    let mut session = Session::new(Document::new([800.0, 400.0], Units::Px, 72.0, 1));
    let id = ObjectId(session.run(&Command::with_params("text.art.add", json!({"pos": [10.0, 40.0], "text": "abcdef", "size_pt": 40.0}))).unwrap()["id"].as_u64().unwrap());
    let story = match session.document.get_object(id).unwrap().kind { ObjectKind::ArtText { story } => story, _ => unreachable!() };
    session.document.stories.iter_mut().find(|s| s.id == story).unwrap().ranges = vec![
        json!({"start": 0, "end": 2, "family": a, "style": "Regular", "size_pt": 40.0}),
        json!({"start": 2, "end": 6, "family": b, "style": "Regular", "size_pt": 40.0}),
    ];
    let faces = text_faces(&session.document, id);
    assert_eq!(faces.len(), text_lines(&session.document, id).len());
    assert!(faces[0].mixed(), "{faces:?}");
    assert_eq!(faces[0].runs.iter().map(|run| (run.start, run.end, run.family.clone())).collect::<Vec<_>>(), vec![(0, 2, a.clone()), (2, 6, b.clone())]);
    assert_eq!(faces[0].family, b);
}