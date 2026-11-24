use dtool_core::command::{anchor_fraction, Session};
use dtool_core::object::Object;
use dtool_core::transform::Anchor;
use dtool_core::{Document, ObjectId};
use dtool_tools::move_tool::{GestureKind, MoveTool, MoveToolAction, PointerEvent};
use dtool_core::serde_json::json;

const ANCHORS: [Anchor; 9] = [
    Anchor::TopLeft,
    Anchor::Top,
    Anchor::TopRight,
    Anchor::Left,
    Anchor::Centre,
    Anchor::Right,
    Anchor::BottomLeft,
    Anchor::Bottom,
    Anchor::BottomRight,
];

fn pointer(x: f32, y: f32, pressed: bool) -> PointerEvent {
    PointerEvent { x, y, pressed, released: false, shift: false, ctrl: false, alt: false }
}

fn rotate_origin(bounds: [f32; 4], anchor: Option<Anchor>) -> [f32; 2] {
    let mut tool = MoveTool::default();
    tool.set_anchor(anchor);
    tool.set_selection_box(Some(bounds));
    let grip = [bounds[0] + bounds[2] * 0.5, bounds[1] - tool.rotate_offset];
    assert!(matches!(tool.on_pointer(pointer(grip[0], grip[1], true)), MoveToolAction::GestureStart { kind: GestureKind::Rotate }));
    tool.on_pointer(pointer(grip[0] + 30.0, grip[1] + 30.0, false));
    tool.current_args().unwrap().origin
}

#[test]
fn move_tool_rotate_pivot_uses_core_anchor_fraction_for_every_anchor() {
    let bounds = [10.0, 20.0, 100.0, 50.0];
    for anchor in ANCHORS {
        let frac = anchor_fraction(anchor);
        let expected = [bounds[0] + bounds[2] * frac[0], bounds[1] + bounds[3] * frac[1]];
        assert_eq!(rotate_origin(bounds, Some(anchor)), expected, "{anchor:?}");
    }
}

#[test]
fn default_rotation_pivot_is_the_box_centre_for_tool_and_transform_set() {
    let bounds = [10.0, 20.0, 100.0, 50.0];
    assert_eq!(rotate_origin(bounds, None), [60.0, 45.0]);
    let mut session = Session::new(Document::default());
    let id = session.document.next_object_id.max(1);
    session.document.objects.push(Object { id: ObjectId(id), pos: [10.0, 20.0], size: [100.0, 50.0], ..Object::default() });
    session.document.next_object_id = id + 1;
    session.run_named("transform.set", json!({"ids": [id], "rotation_deg": 30.0})).unwrap();
    let object = session.document.get_object(ObjectId(id)).unwrap();
    assert_eq!(object.pos, [10.0, 20.0]);
    assert_eq!([object.pos[0] + object.size[0] * 0.5, object.pos[1] + object.size[1] * 0.5], [60.0, 45.0]);
}

#[test]
fn transform_set_position_default_stays_top_left_and_explicit_anchor_is_kept() {
    let mut session = Session::new(Document::default());
    let id = session.document.next_object_id.max(1);
    session.document.objects.push(Object { id: ObjectId(id), pos: [10.0, 20.0], size: [100.0, 50.0], ..Object::default() });
    session.document.next_object_id = id + 1;
    session.run_named("transform.set", json!({"ids": [id], "x": 0.0})).unwrap();
    assert_eq!(session.document.get_object(ObjectId(id)).unwrap().pos, [0.0, 20.0]);
    session.run_named("transform.set", json!({"ids": [id], "x": 100.0, "anchor": "Centre"})).unwrap();
    assert_eq!(session.document.get_object(ObjectId(id)).unwrap().pos, [50.0, 20.0]);
    let query = session.run_named("transform.query", json!({"ids": [id]})).unwrap();
    assert_eq!(query["pos"], json!([50.0, 20.0]));
}