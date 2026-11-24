use dtool_core::command::Session;
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Arrow, BitmapPaint, Colour, GradientGeom, GradientKind, GradientStop, Hatch, HatchLine, HatchUnits, Paint, PaintRole, PatternPaint, StrokeStyle};
use dtool_protocol::{Command, CommandResult};
use dtool_ui::fill_stroke::{FillStrokeWells, PaintOwners, WellState};
use serde_json::{json, Value};

const IDENTITY: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

fn session_with(ids: &[u64]) -> Session {
    let mut document = Document::new([200.0, 200.0], Units::Pt, 72.0, 1);
    for id in ids {
        document.objects.push(Object { id: ObjectId(*id), fill: Paint::None, stroke: StrokeStyle::default(), ..Object::default() });
    }
    Session::new(document)
}

fn create_session_with_object(id: u64) -> Session {
    session_with(&[id])
}

fn create_session_with_two_objects() -> Session {
    session_with(&[1, 2])
}

fn execute(session: &mut Session, name: &str, params: Value) -> CommandResult {
    session.run(&Command::with_params(name, params))
}

fn object(session: &mut Session, id: u64) -> &mut Object {
    session.document.objects.iter_mut().find(|object| object.id == ObjectId(id)).unwrap()
}

fn owners(object: &mut Object) -> PaintOwners<'_> {
    PaintOwners { source: Some(&mut object.fill), frame: None, cell: None, edge: Some(&mut object.stroke), glyph: None }
}

fn solid(r: u8, g: u8, b: u8) -> Paint {
    Paint::Solid { colour: Colour::new(r, g, b, 255) }
}

fn write_through_popup(object: &mut Object, role: PaintRole, paint: Paint) -> bool {
    let mut owners = owners(object);
    let mut wells = FillStrokeWells::new(&owners);
    wells.open_popup_for(&owners, role);
    wells.preview_popup(&mut owners, paint);
    wells.commit_popup(&mut owners)
}

fn solid_rgb(paint: &Paint) -> (u8, u8, u8) {
    match paint {
        Paint::Solid { colour } => (colour.r, colour.g, colour.b),
        _ => panic!("Expected Solid paint"),
    }
}

#[test]
fn test_fill_well_sets_paint() {
    let mut session = create_session_with_object(1);
    assert!(write_through_popup(object(&mut session, 1), PaintRole::SourceObject, solid(255, 0, 0)));
    assert_eq!(solid_rgb(&object(&mut session, 1).fill), (255, 0, 0));
}

#[test]
fn test_stroke_well_sets_paint() {
    let mut session = create_session_with_object(1);
    assert!(write_through_popup(object(&mut session, 1), PaintRole::Edge, solid(0, 255, 0)));
    assert!(execute(&mut session, "stroke.set", json!({ "object_id": 1, "width": 2.0 })).is_ok());
    let obj = object(&mut session, 1);
    assert_eq!(solid_rgb(&obj.stroke.paint), (0, 255, 0));
    assert_eq!(obj.stroke.width, 2.0);
}

#[test]
fn test_fill_popup_writes_same_owner() {
    let mut session = create_session_with_object(1);
    write_through_popup(object(&mut session, 1), PaintRole::SourceObject, solid(10, 20, 30));
    let result = execute(&mut session, "appearance.query", json!({ "node": 1 })).unwrap();
    assert_eq!(result["fills"][0], serde_json::to_value(solid(10, 20, 30)).unwrap());
    let obj = object(&mut session, 1);
    assert_eq!(solid_rgb(&obj.fill), (10, 20, 30));
    assert_eq!(obj.stroke.paint, Paint::None);
}

#[test]
fn test_cancel_restores_original() {
    let mut session = create_session_with_object(1);
    write_through_popup(object(&mut session, 1), PaintRole::SourceObject, solid(100, 100, 100));
    let obj = object(&mut session, 1);
    let mut owners = owners(obj);
    let mut wells = FillStrokeWells::new(&owners);
    wells.open_popup_for(&owners, PaintRole::SourceObject);
    wells.preview_popup(&mut owners, solid(200, 200, 200));
    assert_eq!(solid_rgb(&owners.display(PaintRole::SourceObject)).0, 200);
    wells.cancel_popup(&mut owners);
    assert_eq!(solid_rgb(&owners.display(PaintRole::SourceObject)), (100, 100, 100));
    assert!(wells.active_popup.is_none());
}

#[test]
fn test_paint_swap() {
    let mut session = create_session_with_object(1);
    let obj = object(&mut session, 1);
    obj.fill = solid(255, 0, 0);
    obj.stroke.paint = solid(0, 0, 255);
    let mut owners = owners(obj);
    let mut wells = FillStrokeWells::new(&owners);
    wells.swap_paints();
    wells.write_owner(&mut owners, PaintRole::SourceObject);
    wells.write_owner(&mut owners, PaintRole::Edge);
    assert_eq!(solid_rgb(&obj.fill).2, 255);
    assert_eq!(solid_rgb(&obj.stroke.paint).0, 255);
}

#[test]
fn test_paint_none() {
    let mut session = create_session_with_object(1);
    write_through_popup(object(&mut session, 1), PaintRole::SourceObject, solid(255, 0, 0));
    let obj = object(&mut session, 1);
    let mut owners = owners(obj);
    let mut wells = FillStrokeWells::new(&owners);
    wells.open_popup_for(&owners, PaintRole::SourceObject);
    wells.set_none();
    assert!(wells.write_owner(&mut owners, PaintRole::SourceObject));
    assert!(matches!(obj.fill, Paint::None));
}

#[test]
fn test_paint_default() {
    let mut session = create_session_with_object(1);
    write_through_popup(object(&mut session, 1), PaintRole::SourceObject, solid(255, 0, 0));
    let obj = object(&mut session, 1);
    let mut owners = owners(obj);
    let mut wells = FillStrokeWells::new(&owners);
    wells.set_default();
    wells.write_owner(&mut owners, PaintRole::SourceObject);
    assert_eq!(obj.fill, dtool_core::commands::defaults::current().fill);
}

#[test]
fn test_mixed_selection_does_not_silently_write_one_object() {
    let mut session = create_session_with_two_objects();
    write_through_popup(object(&mut session, 1), PaintRole::SourceObject, solid(128, 128, 128));
    let obj2 = object(&mut session, 2);
    let mut owners = owners(obj2);
    let mut wells = FillStrokeWells::new(&owners);
    wells.fill_state = WellState::Mixed;
    assert!(!wells.write_owner(&mut owners, PaintRole::SourceObject));
    assert_eq!(solid_rgb(&object(&mut session, 1).fill).0, 128);
    assert!(matches!(object(&mut session, 2).fill, Paint::None));
}

#[test]
fn test_stroke_arrows_swap() {
    let mut session = create_session_with_object(1);
    assert!(execute(&mut session, "stroke.set", json!({ "object_id": 1, "width": 1.0 })).is_ok());
    assert!(execute(&mut session, "stroke.arrows.swap", json!({ "object_id": 1 })).is_ok());
    let obj = object(&mut session, 1);
    assert!(obj.stroke.start_arrow.is_none());
    assert!(obj.stroke.end_arrow.is_none());
    obj.stroke.start_arrow = Some(Arrow { name: "triangle".to_string(), scale: 1.0 });
    assert!(execute(&mut session, "stroke.arrows.swap", json!({ "object_id": 1 })).is_ok());
    let obj = object(&mut session, 1);
    assert!(obj.stroke.start_arrow.is_none());
    assert_eq!(obj.stroke.end_arrow.as_ref().map(|arrow| arrow.name.as_str()), Some("triangle"));
}

#[test]
fn test_fill_gradient_set() {
    let mut session = create_session_with_object(1);
    let gradient = Paint::Gradient {
        kind: GradientKind::Linear,
        stops: vec![GradientStop { offset: 0.0, colour: Colour::new(0, 0, 0, 255) }, GradientStop { offset: 1.0, colour: Colour::new(255, 255, 255, 255) }],
        geom: GradientGeom::default(),
    };
    assert!(write_through_popup(object(&mut session, 1), PaintRole::SourceObject, gradient));
    match &object(&mut session, 1).fill {
        Paint::Gradient { kind, .. } => assert!(matches!(kind, GradientKind::Linear)),
        _ => panic!("Expected Gradient paint"),
    }
}

#[test]
fn test_fill_hatch_set() {
    let mut session = create_session_with_object(1);
    let hatch = Hatch {
        lines: vec![HatchLine { angle_rad: 0.785, origin: [0.0, 0.0], shift: 0.0, spacing: 4.0, dash: Vec::new() }],
        colour: Colour::new(0, 0, 0, 255),
        background: None,
        weight: 1.0,
        units: HatchUnits::Document,
        matrix: IDENTITY,
    };
    assert!(write_through_popup(object(&mut session, 1), PaintRole::SourceObject, Paint::Hatch(hatch)));
    assert!(matches!(object(&mut session, 1).fill, Paint::Hatch(_)));
}

#[test]
fn test_fill_pattern_set() {
    let mut session = create_session_with_object(1);
    let pattern = Paint::Pattern(PatternPaint { resource_id: "checker".to_string(), matrix: IDENTITY });
    assert!(write_through_popup(object(&mut session, 1), PaintRole::SourceObject, pattern));
    assert!(matches!(object(&mut session, 1).fill, Paint::Pattern(_)));
}

#[test]
fn test_fill_bitmap_set() {
    let mut session = create_session_with_object(1);
    let bitmap = Paint::Bitmap(BitmapPaint { resource_id: "img1".to_string(), matrix: IDENTITY, seed: None });
    assert!(write_through_popup(object(&mut session, 1), PaintRole::SourceObject, bitmap));
    assert!(matches!(object(&mut session, 1).fill, Paint::Bitmap(_)));
}

#[test]
fn test_appearance_fill_add() {
    let mut session = create_session_with_object(1);
    let result = execute(&mut session, "appearance.add_fill", json!({ "node": 1, "paint": solid(50, 50, 50) }));
    assert!(result.is_ok());
    assert_eq!(solid_rgb(&object(&mut session, 1).fill), (50, 50, 50));
}

#[test]
fn test_appearance_stroke_add() {
    let mut session = create_session_with_object(1);
    let stroke = StrokeStyle { paint: solid(50, 50, 50), width: 1.0, ..StrokeStyle::default() };
    let result = execute(&mut session, "appearance.add_stroke", json!({ "node": 1, "stroke": stroke }));
    assert!(result.is_ok());
    assert_eq!(object(&mut session, 1).stroke.width, 1.0);
}

#[test]
fn test_stroke_texture_set() {
    let mut session = create_session_with_object(1);
    let texture = Paint::Pattern(PatternPaint { resource_id: "noise".to_string(), matrix: IDENTITY });
    assert!(write_through_popup(object(&mut session, 1), PaintRole::Edge, texture));
    assert!(matches!(object(&mut session, 1).stroke.paint, Paint::Pattern(_)));
}

#[test]
fn test_stroke_query() {
    let mut session = create_session_with_object(1);
    assert!(execute(&mut session, "stroke.set", json!({ "object_id": 1, "width": 3.0 })).is_ok());
    let result = execute(&mut session, "stroke.query", json!({ "object_id": 1 }));
    assert!(result.is_ok());
    assert_eq!(result.unwrap()["stroke"]["width"], json!(3.0));
}

#[test]
fn test_fill_query() {
    let mut session = create_session_with_object(1);
    write_through_popup(object(&mut session, 1), PaintRole::SourceObject, solid(0, 0, 0));
    let result = execute(&mut session, "appearance.query", json!({ "node": 1 }));
    assert!(result.is_ok());
    assert_eq!(result.unwrap()["fills"].as_array().map(Vec::len), Some(1));
}

#[test]
fn test_stroke_arrows_list() {
    let mut session = create_session_with_object(1);
    let result = execute(&mut session, "stroke.arrows.list", json!({}));
    assert!(result.is_ok());
}

#[test]
fn test_swatch_hatch_add() {
    let mut session = create_session_with_object(1);
    let result = execute(&mut session, "swatch.add", json!({ "name": "diagonal", "colour": "#000000" }));
    assert!(result.is_ok());
    assert!(session.document.swatches.iter().any(|swatch| swatch.name == "diagonal"));
}