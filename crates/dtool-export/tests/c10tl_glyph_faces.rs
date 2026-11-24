use dtool_core::document::Units;
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::text_lines::{text_faces, text_lines, text_outlines};
use dtool_core::{Command, Document, Session};
use dtool_export::DocumentExportExt;
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

fn art(session: &mut Session, text: &str, y: f64, ranges: &[(usize, usize, &str)]) -> ObjectId {
    let mut add = Command::new("text.art.add");
    add.params["pos"] = vec![10.0, y].into();
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
    session.document.version += 1;
    id
}

const W: usize = 400;
const H: usize = 200;

fn band(session: &Session, top: f32) -> Vec<u8> {
    session.document.render_rgba_region((0.0, top, W as f32, H as f32), W, H)
}

fn inked(px: &[u8]) -> usize {
    px.chunks(4).filter(|p| p[3] > 0).count()
}

fn rows(px: &[u8], from: usize, to: usize) -> Vec<u8> {
    px[from * W * 4..to * W * 4].to_vec()
}

#[test]
fn c10tl_export_fills_glyph_outlines_with_each_runs_face() {
    install();
    let lib = FontLibrary::shared();
    let Some((a, b)) = two_families(&lib) else { return };
    let mut session = Session::new(Document::new([W as f32, 800.0], Units::Px, 72.0, 1));
    let first = art(&mut session, "Hamburgs\nHamburgs", 0.0, &[(0, 17, &a)]);
    let mixed = art(&mut session, "Hamburgs\nHamburgs", 200.0, &[(0, 9, &a), (9, 17, &b)]);
    let other = art(&mut session, "Hamburgs\nHamburgs", 400.0, &[(0, 17, &b)]);
    let faces = text_faces(&session.document, mixed);
    assert_eq!(faces.iter().map(|face| face.family.clone()).collect::<Vec<_>>(), vec![a.clone(), b.clone()]);
    assert!(text_outlines(&session.document, mixed).is_some());

    let pa = band(&session, 0.0);
    let pm = band(&session, 200.0);
    let pb = band(&session, 400.0);
    for px in [&pa, &pm, &pb] {
        assert!(inked(px) > 400, "glyphs drawn");
    }
    let line = &text_lines(&session.document, first)[0];
    let split = line.origin[1].floor() as usize;
    let boxed = (line.width * line.size) as usize * 2;
    assert!(inked(&pa) < boxed * 3 / 4, "glyph outlines, not solid line boxes: {} vs {}", inked(&pa), boxed);
    assert!(pa != pb, "different families draw different pixels");
    assert!(rows(&pm, 0, split) == rows(&pa, 0, split), "run in the first family matches the single-face render");
    assert!(rows(&pm, split, H) != rows(&pa, split, H), "run in the second family is not drawn in the first family");
    assert!(inked(&rows(&pm, split, H)) > 100);
    let _ = other;
}

#[test]
fn c10tl_missing_family_renders_with_the_fallback_face() {
    install();
    let mut session = Session::new(Document::new([W as f32, 400.0], Units::Px, 72.0, 1));
    let id = art(&mut session, "Hamburgs", 0.0, &[(0, 8, "NoSuchFamily Zz9")]);
    assert!(!text_lines(&session.document, id).is_empty());
    let px = band(&session, 0.0);
    assert!(inked(&px) > 200);
    let line = &text_lines(&session.document, id)[0];
    assert!(inked(&px) < ((line.width * line.size) as usize) * 3 / 4);
}

fn columns(px: &[u8], from: usize, to: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for row in px.chunks(W * 4) {
        out.extend_from_slice(&row[from * 4..to * 4]);
    }
    out
}

#[test]
fn c10tl_mixed_faces_inside_one_line_export_each_run_in_its_face() {
    install();
    let lib = FontLibrary::shared();
    let Some((a, b)) = two_families(&lib) else { return };
    let mut session = Session::new(Document::new([W as f32, 800.0], Units::Px, 72.0, 1));
    art(&mut session, "Hamburgs", 0.0, &[(0, 8, &a)]);
    art(&mut session, "Hamburgs", 200.0, &[(0, 8, &b)]);
    let mixed = art(&mut session, "Hamburgs", 400.0, &[(0, 4, &a), (4, 8, &b)]);
    assert!(text_faces(&session.document, mixed)[0].mixed());
    let pa = band(&session, 0.0);
    let pb = band(&session, 200.0);
    let pm = band(&session, 400.0);
    assert!(columns(&pm, 0, 30) == columns(&pa, 0, 30), "first run in the first face");
    assert!(columns(&pm, 0, W) != columns(&pa, 0, W), "second run not in the first face");
    assert!(columns(&pm, 0, 30) != columns(&pb, 0, 30), "first run not in the second face");
    assert!(inked(&pm) > 200);
}