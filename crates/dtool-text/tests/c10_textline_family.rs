use dtool_core::document::Units;
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::text_lines::{story_face, text_faces, text_lines, text_outline_curve};
use dtool_core::{Command, Document, Session};
use dtool_text::FontLibrary;

fn install() {
    assert!(dtool_text::canvas_text::install());
    assert!(dtool_text::text_faces::install());
}

fn two_families(lib: &FontLibrary) -> Option<(String, String)> {
    let usable: Vec<String> = lib.families().into_iter().map(str::to_owned).filter(|family| lib.resolve(family, "Regular").is_some()).collect();
    let pick = |names: &[&str]| names.iter().find_map(|name| usable.iter().find(|have| have.eq_ignore_ascii_case(name)).cloned());
    let first = pick(&["Arial", "Segoe UI", "Noto Sans", "DejaVu Sans", "Liberation Sans"]).or_else(|| usable.first().cloned())?;
    let second = pick(&["Times New Roman", "Georgia", "Cambria", "Noto Serif", "DejaVu Serif", "Liberation Serif"]).filter(|name| *name != first).or_else(|| usable.iter().find(|name| **name != first).cloned())?;
    Some((first, second))
}

fn art(session: &mut Session, text: &str, x: f64, ranges: &[(usize, usize, &str)]) -> (ObjectId, u64) {
    let mut add = Command::new("text.art.add");
    add.params["pos"] = vec![x, 40.0].into();
    add.params["text"] = text.into();
    add.params["size_pt"] = 40.0.into();
    let id = ObjectId(session.run(&add).unwrap()["id"].as_u64().unwrap());
    session.text_edit = None;
    let story = match session.document.get_object(id).unwrap().kind { ObjectKind::ArtText { story } => story, _ => unreachable!() };
    let core = session.document.stories.iter_mut().find(|s| s.id == story).unwrap();
    core.ranges = ranges
        .iter()
        .map(|(start, end, family)| {
            let mut range = Command::new("range").params;
            range["start"] = (*start as u64).into();
            range["end"] = (*end as u64).into();
            range["size_pt"] = 40.0.into();
            range["family"] = (*family).into();
            range["style"] = "Regular".into();
            range
        })
        .collect();
    (id, story.0)
}

#[test]
fn two_art_texts_report_their_own_family_per_line() {
    install();
    let lib = FontLibrary::shared();
    let Some((a, b)) = two_families(&lib) else { return };
    let mut session = Session::new(Document::new([800.0, 400.0], Units::Px, 72.0, 1));
    let (first, _) = art(&mut session, "Hamburg", 10.0, &[(0, 7, &a)]);
    let (second, _) = art(&mut session, "Hamburg", 400.0, &[(0, 7, &b)]);
    let first_faces = text_faces(&session.document, first);
    let second_faces = text_faces(&session.document, second);
    assert_eq!(first_faces.len(), text_lines(&session.document, first).len());
    assert_eq!(second_faces.len(), text_lines(&session.document, second).len());
    assert_eq!(first_faces[0].family, a);
    assert_eq!(second_faces[0].family, b);
    assert_eq!(first_faces[0].style, "Regular");
    assert!(!first_faces[0].mixed());
    assert_eq!(first_faces[0].runs[0].start, 0);
    assert_eq!(first_faces[0].runs[0].end, 7);
    assert_eq!(first_faces[0].runs[0].size, 40.0);
    let outline = |id| text_outline_curve(&session.document, id).map(|(_, size, curve)| (size, curve.contour.points.len())).ok();
    assert_ne!(outline(first), outline(second), "glyph outlines must come from different faces");
}

#[test]
fn mixed_families_inside_one_line_give_runs_in_line_coordinates() {
    install();
    let lib = FontLibrary::shared();
    let Some((a, b)) = two_families(&lib) else { return };
    let mut session = Session::new(Document::new([800.0, 400.0], Units::Px, 72.0, 1));
    let (id, _) = art(&mut session, "abc\ndefgh", 10.0, &[(0, 5, &a), (5, 9, &b)]);
    let faces = text_faces(&session.document, id);
    let lines = text_lines(&session.document, id);
    assert_eq!(faces.len(), 2);
    assert_eq!(lines[0].text, "abc");
    assert_eq!(faces[0].family, a);
    assert!(!faces[0].mixed());
    assert_eq!(lines[1].text, "defgh");
    assert_eq!(faces[1].runs.iter().map(|run| (run.start, run.end, run.family.clone())).collect::<Vec<_>>(), vec![(0, 1, a.clone()), (1, 5, b.clone())]);
    assert_eq!(faces[1].family, b, "dominant face of the line");
    assert!(faces[1].mixed());
    assert_eq!(faces[1].face_at(0).0, a);
    assert_eq!(faces[1].face_at(3).0, b);
    assert_eq!(faces[1].face_at(99).0, b);
}

#[test]
fn font_preview_changes_the_face_without_a_document_change() {
    install();
    dtool_text::text_host::install();
    let lib = FontLibrary::shared();
    let Some((a, b)) = two_families(&lib) else { return };
    let mut session = Session::new(Document::new([800.0, 400.0], Units::Px, 72.0, 1));
    let (id, story) = art(&mut session, "Preview", 10.0, &[(0, 7, &a)]);
    let version = session.document.version;
    let generation = session.generation;
    let mut preview = Command::new("text.font.preview");
    preview.params["story"] = story.into();
    preview.params["range"]["start"] = 0.into();
    preview.params["range"]["end"] = 7.into();
    preview.params["family"] = b.as_str().into();
    session.run(&preview).expect("preview");
    assert_eq!(text_faces(&session.document, id)[0].family, b);
    assert_eq!(session.document.version, version);
    assert_eq!(session.generation, generation);
    assert_eq!(story_face(session.document.stories.iter().find(|s| s.id.0 == story).unwrap()).family, a, "stored story keeps its family");
    assert!(dtool_text::font_preview::cancel_font_preview());
    assert_eq!(text_faces(&session.document, id)[0].family, a);
}

#[test]
fn core_story_face_reads_range_family() {
    let mut story = dtool_core::Story::default();
    let mut range = Command::new("range").params;
    range["start"] = 0.into();
    range["end"] = 3.into();
    range["family"] = "Fam".into();
    range["style"] = "Bold".into();
    range["weight"] = 700.into();
    range["italic"] = true.into();
    story.ranges.push(range);
    let face = story_face(&story);
    assert_eq!((face.family.as_str(), face.style.as_str(), face.weight, face.italic), ("Fam", "Bold", 700, true));
    assert_eq!(story_face(&dtool_core::Story::default()).weight, 400);
}