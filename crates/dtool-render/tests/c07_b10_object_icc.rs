use dtool_core::colour::ProfileStore;
use dtool_core::paint::{Colour, Paint};
use dtool_core::serde_json::{json, Value};
use dtool_core::{CanvasFlags, Command, CommandResult, Object, ObjectId, Scope, Workspace};

fn cmyk() -> Colour {
    Colour::from_managed(dtool_core::colour::Colour::cmyk(20.0, 80.0, 0.0, 10.0, 1.0))
}

fn fresh() -> (Workspace, Vec<ObjectId>) {
    let mut workspace = Workspace::default();
    workspace.new_document("c07_b10_render_icc");
    let session = workspace.session_mut().unwrap();
    let mut ids = Vec::new();
    for (index, colour) in [cmyk(), cmyk(), cmyk(), Colour::new(200, 40, 40, 255)].into_iter().enumerate() {
        ids.push(session.document.create_object(Object { name: format!("o{index}"), pos: [index as f32 * 20.0, 0.0], size: [10.0, 10.0], fill: Paint::Solid { colour }, ..Object::default() }));
    }
    (workspace, ids)
}

fn call(workspace: &mut Workspace, name: &str, params: Value) -> CommandResult {
    workspace.run(Scope::active(), Command::with_params(name, params)).1
}

fn rendered(workspace: &Workspace, id: ObjectId) -> Colour {
    let scene = dtool_render::scene::build_scene(&workspace.session().unwrap().document, &CanvasFlags::default());
    let paint = scene.prims.iter().find_map(|prim| match prim {
        dtool_core::scene::Prim::Fill { object, paint, .. } if *object == id => Some(paint.clone()),
        _ => None,
    });
    match paint {
        Some(Paint::Solid { colour }) => colour,
        other => panic!("{other:?}"),
    }
}

fn rgb(colour: Colour) -> (u8, u8, u8) {
    (colour.r, colour.g, colour.b)
}

#[test]
fn c07_b10_render_honours_object_profile_and_intent() {
    let (mut workspace, ids) = fresh();
    let plain = rgb(rendered(&workspace, ids[2]));
    assert_ne!(plain, rgb(cmyk()), "no-profile CMYK goes through the default CMYK profile");
    call(&mut workspace, "node.icc.set", json!({ "id": ids[0].0, "profile": ProfileStore::FOGRA39L, "intent": "Perceptual" })).unwrap();
    call(&mut workspace, "node.icc.set", json!({ "id": ids[1].0, "profile": ProfileStore::FOGRA39L, "intent": "AbsoluteColorimetric" })).unwrap();
    let perceptual = rgb(rendered(&workspace, ids[0]));
    let absolute = rgb(rendered(&workspace, ids[1]));
    assert_ne!(perceptual, plain, "object profile must change the rendered colour");
    assert_ne!(absolute, perceptual, "object intent must change the rendered colour");
    assert_eq!(rgb(rendered(&workspace, ids[2])), plain, "object without ObjectIcc is unchanged");
    let services = dtool_core::colour::services().read().unwrap();
    let ctx = dtool_core::colour::ConvertCtx { from: ProfileStore::FOGRA39L.to_string(), to: ProfileStore::SRGB.to_string(), intent: dtool_core::colour::RenderingIntent::Perceptual, bpc: services.preferences.bpc };
    let expected = dtool_core::colour::convert::convert(&cmyk().managed(), dtool_core::colour::ColourModel::Rgb, &ctx, &services.profiles).unwrap();
    drop(services);
    let expected = (expected.values[0], expected.values[1], expected.values[2]);
    let to_u8 = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    assert_eq!(perceptual, (to_u8(expected.0), to_u8(expected.1), to_u8(expected.2)));
    assert_eq!(rendered(&workspace, ids[0]).a, 255);
    assert_eq!(workspace.session().unwrap().document.get_object(ids[0]).unwrap().fill, Paint::Solid { colour: cmyk() }, "stored colour is not rewritten");
}

#[test]
fn c07_b10_render_icc_undo_restores_and_rgb_srgb_is_identity() {
    let (mut workspace, ids) = fresh();
    let plain = rgb(rendered(&workspace, ids[0]));
    call(&mut workspace, "node.icc.set", json!({ "id": ids[0].0, "profile": ProfileStore::FOGRA39L, "intent": "Saturation" })).unwrap();
    let managed = rgb(rendered(&workspace, ids[0]));
    assert_ne!(managed, plain);
    call(&mut workspace, "edit.undo", json!({})).unwrap();
    assert_eq!(rgb(rendered(&workspace, ids[0])), plain);
    call(&mut workspace, "edit.redo", json!({})).unwrap();
    assert_eq!(rgb(rendered(&workspace, ids[0])), managed);
    call(&mut workspace, "node.icc.set", json!({ "id": ids[3].0, "profile": "srgb", "intent": "Perceptual" })).unwrap();
    assert_eq!(rgb(rendered(&workspace, ids[3])), (200, 40, 40));
    call(&mut workspace, "node.icc.set", json!({ "id": ids[3].0, "profile": ProfileStore::FOGRA39L })).unwrap();
    assert_eq!(rgb(rendered(&workspace, ids[3])), (200, 40, 40), "profile of another model leaves RGB fills alone");
}

#[test]
fn c07_b10_render_icc_intent_only_uses_document_cmyk_profile() {
    let (mut workspace, ids) = fresh();
    let plain = rgb(rendered(&workspace, ids[0]));
    call(&mut workspace, "node.icc.set", json!({ "id": ids[0].0, "intent": "Perceptual" })).unwrap();
    call(&mut workspace, "node.icc.set", json!({ "id": ids[1].0, "intent": "AbsoluteColorimetric" })).unwrap();
    assert_ne!(rgb(rendered(&workspace, ids[0])), plain);
    assert_ne!(rgb(rendered(&workspace, ids[0])), rgb(rendered(&workspace, ids[1])));
    for _ in 0..200 {
        rendered(&workspace, ids[0]);
    }
}