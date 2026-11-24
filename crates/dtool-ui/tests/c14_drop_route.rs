use dtool_core::Workspace;
use dtool_ui::place_tool::{drop_commands, PlaceTool};
use std::path::PathBuf;

fn fixture(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dtool_c14_drop_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn drop_commands_route_documents_to_place_and_text_to_import() {
    let paths: Vec<PathBuf> = ["a.PDF", "b.svg", "c.ai", "d.eps", "e.txt", "f.RTF", "g.docx", "h.png", "i.xyz"].iter().map(PathBuf::from).collect();
    let (texts, opens) = drop_commands(&paths);
    assert_eq!(texts.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["text.import"; 3]);
    assert_eq!(opens.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["import.place"; 4]);
    assert_eq!(opens[0].params["path"], "a.PDF");
    assert_eq!(texts[2].params["path"], "g.docx");
}

#[test]
fn dropping_svg_places_into_the_live_document_with_one_undo_and_txt_imports() {
    dtool_io::install_import_commands();
    let mut ws = Workspace::default();
    let live = ws.new_document("Live");
    let svg = fixture("art.svg", br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect id="box" x="10" y="10" width="50" height="40" fill="#ff0000"/></svg>"##);
    let txt = fixture("story.txt", b"Dropped story");
    let mut tool = PlaceTool::default();
    let before = ws.get(live).unwrap().session.document.objects.len();
    assert!(tool.drop_files(&mut ws, &[txt]).expect("txt drop"));
    assert!(ws.get(live).unwrap().session.document.objects.len() > before);
    assert!(ws.get(live).unwrap().session.document.stories.iter().any(|story| story.text.contains("Dropped story")));
    let objects = ws.get(live).unwrap().session.document.objects.len();
    let steps = ws.get(live).unwrap().session.undo.applied_steps();
    assert!(tool.drop_files(&mut ws, &[svg.clone()]).expect("svg drop"));
    assert_eq!(ws.contexts().len(), 1);
    assert_eq!(ws.active(), Some(live));
    let session = &ws.get(live).unwrap().session;
    assert!(session.document.objects.iter().any(|o| o.name == "box"));
    assert_eq!(session.undo.applied_steps(), steps + 1);
    assert!(session.selection.ids.iter().any(|id| session.document.get_object(*id).is_some_and(|o| o.name == "box")));
    ws.get_mut(live).unwrap().session.run(&dtool_core::Command::new("edit.undo")).expect("undo place");
    assert_eq!(ws.get(live).unwrap().session.document.objects.len(), objects);
    let mut empty = Workspace::default();
    assert!(tool.drop_files(&mut empty, &[svg]).expect("svg drop without a document"));
    assert_eq!(empty.contexts().len(), 1);
    assert!(empty.get(empty.active().unwrap()).unwrap().session.document.objects.iter().any(|o| o.name == "box"));
}

#[test]
fn unknown_drop_is_ignored_and_bad_document_reports_an_error_without_a_context() {
    dtool_io::install_import_commands();
    let mut ws = Workspace::default();
    ws.new_document("Live");
    let mut tool = PlaceTool::default();
    assert!(!tool.drop_files(&mut ws, &[fixture("x.xyz", b"?")]).unwrap());
    let live = ws.active().unwrap();
    let before = serde_json::to_value(&ws.get(live).unwrap().session.document).unwrap();
    assert!(tool.drop_files(&mut ws, &[fixture("bad.svg", b"<svg")]).is_err());
    assert_eq!(ws.contexts().len(), 1);
    assert_eq!(serde_json::to_value(&ws.get(live).unwrap().session.document).unwrap(), before);
}