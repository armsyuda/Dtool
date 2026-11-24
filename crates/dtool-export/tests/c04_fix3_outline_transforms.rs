use dtool_core::document::Units;
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::text_lines::text_outline_curve;
use dtool_core::{Command, Document, Session};
use dtool_text::canvas_text::text_outlines_with;
use dtool_text::{layout_story, story_from_core, FontLibrary};
use serde_json::{json, Value};

fn family() -> Option<String> {
    let lib = FontLibrary::shared();
    let families = lib.families();
    families.iter().find(|f| f.eq_ignore_ascii_case("Arial")).or(families.first()).map(|f| f.to_string())
}

fn art(session: &mut Session, text: &str, family: &str, extra: Value) -> ObjectId {
    let mut add = Command::new("text.art.add");
    add.params["pos"] = vec![10.0f64, 20.0].into();
    add.params["text"] = text.into();
    add.params["size_pt"] = 40.0.into();
    let id = ObjectId(session.run(&add).unwrap()["id"].as_u64().unwrap());
    let story = match session.document.get_object(id).unwrap().kind { ObjectKind::ArtText { story } => story, _ => unreachable!() };
    let story = session.document.stories.iter_mut().find(|s| s.id == story).unwrap();
    let mut range = json!({"start": 0, "end": text.chars().count(), "family": family, "size_pt": 40.0});
    for (key, value) in extra.as_object().unwrap() {
        range[key] = value.clone();
    }
    story.ranges = vec![range];
    id
}

fn untransformed(lib: &FontLibrary, session: &Session, id: ObjectId) -> Vec<Vec<[f32; 2]>> {
    let object = session.document.get_object(id).unwrap();
    let ObjectKind::ArtText { story } = object.kind else { unreachable!() };
    let story = story_from_core(session.document.stories.iter().find(|s| s.id == story).unwrap());
    let mut out = Vec::new();
    for line in layout_story(lib, &story, None) {
        let mut pen = 0.0;
        for glyph in &line.glyphs {
            let origin = [object.pos[0] + pen + glyph.offset[0], object.pos[1] + line.baseline + glyph.offset[1]];
            for contour in lib.glyph_contours(glyph.font, glyph.gid, story.chars[0].style.size_pt) {
                out.push(contour.iter().map(|p| [origin[0] + p[0], origin[1] - p[1]]).collect());
            }
            pen += glyph.advance;
        }
    }
    out
}

fn bounds(contours: &[Vec<[f32; 2]>]) -> [f32; 4] {
    contours.iter().flatten().fold([f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY], |b, p| [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])])
}

#[test]
fn default_scale_matches_untransformed_c04_fix2_outlines() {
    let Some(family) = family() else { return };
    let lib = FontLibrary::shared();
    let mut s = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let id = art(&mut s, "AV", &family, json!({}));
    let outlines = text_outlines_with(&lib, &s.document, id).unwrap();
    assert!(!outlines.is_empty());
    assert_eq!(outlines, untransformed(&lib, &s, id));
}

#[test]
fn h_scale_and_v_scale_stretch_the_outline() {
    let Some(family) = family() else { return };
    let lib = FontLibrary::shared();
    let mut s = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let plain = art(&mut s, "H", &family, json!({}));
    let wide = art(&mut s, "H", &family, json!({"h_scale": 2.0}));
    let tall = art(&mut s, "H", &family, json!({"v_scale": 1.5}));
    let (p, w, t) = (bounds(&text_outlines_with(&lib, &s.document, plain).unwrap()), bounds(&text_outlines_with(&lib, &s.document, wide).unwrap()), bounds(&text_outlines_with(&lib, &s.document, tall).unwrap()));
    let (pw, ph) = (p[2] - p[0], p[3] - p[1]);
    assert!(((w[2] - w[0]) / pw - 2.0).abs() < 0.05, "{p:?} {w:?}");
    assert!(((w[3] - w[1]) - ph).abs() < 0.01);
    assert!(((t[3] - t[1]) / ph - 1.5).abs() < 0.05, "{p:?} {t:?}");
    assert!(((t[2] - t[0]) - pw).abs() < 0.01);
    assert_ne!(text_outlines_with(&lib, &s.document, wide).unwrap(), untransformed(&lib, &s, wide));
}

#[test]
fn shear_leans_the_outline_about_the_baseline() {
    let Some(family) = family() else { return };
    let lib = FontLibrary::shared();
    let mut s = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let plain = art(&mut s, "I", &family, json!({}));
    let lean = art(&mut s, "I", &family, json!({"shear_deg": 20.0}));
    let a = text_outlines_with(&lib, &s.document, plain).unwrap();
    let b = text_outlines_with(&lib, &s.document, lean).unwrap();
    assert_ne!(a, b);
    let (pa, pb) = (bounds(&a), bounds(&b));
    assert!(((pb[3] - pb[1]) - (pa[3] - pa[1])).abs() < 0.01);
    let lean_extra = (pb[2] - pb[0]) - (pa[2] - pa[0]);
    let expected = (pa[3] - pa[1]) * 20f32.to_radians().tan();
    assert!((lean_extra - expected).abs() < 1.0, "{lean_extra} vs {expected}");
    let top = |c: &[Vec<[f32; 2]>], b: [f32; 4]| c.iter().flatten().filter(|p| p[1] < b[1] + 1.0).map(|p| p[0]).fold(f32::INFINITY, f32::min);
    assert!(top(&b, pb) > top(&a, pa) + expected * 0.8);
}

#[test]
fn convert_and_undo_restores_the_text_object() {
    assert!(dtool_text::canvas_text::install());
    let Some(family) = family() else { return };
    let mut s = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let id = art(&mut s, "Hi", &family, json!({"h_scale": 1.5, "shear_deg": 10.0}));
    let (_, size, curve) = text_outline_curve(&s.document, id).unwrap();
    assert!(curve.contour.points.len() >= 3 && size[0] > 5.0);
    let before_object = s.document.get_object(id).cloned().unwrap();
    let before_stories = s.document.stories.clone();
    s.run(&Command::with_params("text.to_path", json!({"id": id.0}))).unwrap();
    assert!(!matches!(s.document.get_object(id).map(|o| &o.kind), Some(ObjectKind::ArtText { .. })));
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(serde_json::to_value(s.document.get_object(id).unwrap()).unwrap(), serde_json::to_value(&before_object).unwrap());
    assert_eq!(s.document.stories, before_stories);
}