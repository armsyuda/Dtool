use dtool_core::document::Units;
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::scene::{text_glyph_bounds, text_glyph_image};
use dtool_core::text_lines::{text_faces, text_lines};
use dtool_core::{Command, Document, Session};
use dtool_export::DocumentExportExt;
use dtool_text::FontLibrary;

const W: usize = 400;
const H: usize = 200;

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

fn art(session: &mut Session, text: &str, y: f64, ranges: &[(usize, usize, &str, [u8; 3])]) -> ObjectId {
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
        .map(|(start, end, family, rgb)| {
            let mut range = Command::new("range").params;
            range["start"] = (*start as u64).into();
            range["end"] = (*end as u64).into();
            range["size_pt"] = 40.0.into();
            range["family"] = (*family).into();
            range["style"] = "Regular".into();
            range["fill"] = serde_json::to_value(dtool_core::paint::Paint::Solid { colour: dtool_core::paint::Colour::new(rgb[0], rgb[1], rgb[2], 255) }).unwrap();
            range
        })
        .collect();
    session.document.version += 1;
    id
}

fn canvas_image(session: &Session, id: ObjectId, top: f32) -> Vec<u8> {
    let lines = text_lines(&session.document, id);
    text_glyph_image(&session.document, id, &lines, [0.0, top, W as f32, top + H as f32], [W, H]).expect("glyph image")
}

fn export_band(session: &Session, top: f32) -> Vec<u8> {
    session.document.render_rgba_region((0.0, top, W as f32, H as f32), W, H)
}

fn coverage_mismatch(a: &[u8], b: &[u8]) -> usize {
    a.chunks(4).zip(b.chunks(4)).filter(|(x, y)| (x[3] > 0) != (y[3] > 0) || (x[3] == 255 && y[3] == 255 && x[..3] != y[..3])).count()
}

fn inked(px: &[u8]) -> usize {
    px.chunks(4).filter(|p| p[3] > 0).count()
}

fn columns(px: &[u8], from: usize, to: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for row in px.chunks(W * 4) {
        out.extend_from_slice(&row[from * 4..to * 4]);
    }
    out
}

#[test]
fn c10_canvas_glyph_image_matches_export_for_a_mixed_face_line() {
    install();
    let lib = FontLibrary::shared();
    let Some((a, b)) = two_families(&lib) else { return };
    let mut session = Session::new(Document::new([W as f32, 800.0], Units::Px, 72.0, 1));
    let single = art(&mut session, "Hamburgs", 0.0, &[(0, 8, &a, [0, 0, 0])]);
    let mixed = art(&mut session, "Hamburgs", 200.0, &[(0, 4, &a, [0, 0, 0]), (4, 8, &b, [0, 0, 0])]);
    assert!(text_faces(&session.document, mixed)[0].mixed());
    let canvas_single = canvas_image(&session, single, 0.0);
    let canvas_mixed = canvas_image(&session, mixed, 200.0);
    assert!(inked(&canvas_mixed) > 200);
    assert_eq!(coverage_mismatch(&canvas_single, &export_band(&session, 0.0)), 0);
    assert_eq!(coverage_mismatch(&canvas_mixed, &export_band(&session, 200.0)), 0);
    assert!(columns(&canvas_mixed, 0, 30) == columns(&canvas_single, 0, 30), "first run in face A on the canvas");
    assert!(columns(&canvas_mixed, 0, W) != columns(&canvas_single, 0, W), "second run in face B on the canvas");
    let bounds = text_glyph_bounds(&session.document, mixed).unwrap();
    assert!(bounds[0] >= 10.0 - 1.0 && bounds[1] >= 200.0 && bounds[3] <= 400.0, "{bounds:?}");
}

#[test]
fn c10_canvas_glyph_image_keeps_the_line_colour_and_falls_back_for_missing_family() {
    install();
    let mut session = Session::new(Document::new([W as f32, 400.0], Units::Px, 72.0, 1));
    let red = art(&mut session, "Hamburgs", 0.0, &[(0, 8, "NoSuchFamily Zz9", [255, 0, 0])]);
    let image = canvas_image(&session, red, 0.0);
    assert!(inked(&image) > 200);
    assert!(image.chunks(4).filter(|p| p[3] == 255).all(|p| p[..3] == [255, 0, 0]));
    assert_eq!(coverage_mismatch(&image, &export_band(&session, 0.0)), 0);
    let lines = text_lines(&session.document, red);
    assert!(text_glyph_image(&session.document, red, &lines, [0.0, 0.0, 0.0, 10.0], [4, 4]).is_none());
    assert!(text_glyph_image(&session.document, ObjectId(999_999), &lines, [0.0, 0.0, 10.0, 10.0], [4, 4]).is_none());
}