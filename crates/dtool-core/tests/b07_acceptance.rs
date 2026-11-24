use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::selection::SelectOp;
use dtool_core::workspace::{ContextId, Scope, Workspace};
use dtool_protocol::{Command, CommandResult};

struct Fixture {
    ws: Workspace,
    ctx: ContextId,
}

impl Fixture {
    fn new() -> Self {
        let mut ws = Workspace::default();
        let ctx = ws.new_document("b07");
        ws.get_mut(ctx).unwrap().session.document = Document::new([1000.0, 1000.0], Units::Px, 96.0, 1);
        Self { ws, ctx }
    }

    fn add_object(&mut self, name: &str, pos: [f32; 2], size: [f32; 2]) -> ObjectId {
        let doc = &mut self.ws.get_mut(self.ctx).unwrap().session.document;
        doc.create_object(Object { name: name.into(), pos, size, ..Object::default() })
    }

    fn select(&mut self, id: ObjectId) {
        self.ws.get_mut(self.ctx).unwrap().session.selection.set(vec![id], SelectOp::Add);
    }

    fn run(&mut self, cmd: Command) -> CommandResult {
        self.ws.run(Scope::of(self.ctx), cmd).1
    }

    fn undo(&mut self) -> CommandResult {
        self.run(Command::with_params("edit.undo", serde_json::json!({})))
    }

    fn object(&self, id: ObjectId) -> Object {
        self.ws.get(self.ctx).unwrap().session.document.get_object(id).unwrap().clone()
    }
}

#[test]
fn test_transform_set_anchor_top_left() {
    let mut fx = Fixture::new();
    let obj_id = fx.add_object("rect", [10.0, 10.0], [100.0, 50.0]);
    
    let cmd = Command {
        name: "transform.set".to_string(),
        params: serde_json::json!({
            "ids": [obj_id.0],
            "x": 20.0,
            "y": 30.0,
            "anchor": "TopLeft"
        }),
    };
    
    let result = fx.run(cmd);
    assert!(matches!(result, Ok(_)));
    
    let obj = fx.object(obj_id);
    assert_eq!(obj.pos[0], 20.0);
    assert_eq!(obj.pos[1], 30.0);
}

#[test]
fn test_transform_set_anchor_centre() {
    let mut fx = Fixture::new();
    let obj_id = fx.add_object("rect", [10.0, 10.0], [100.0, 50.0]);
    
    let cmd = Command {
        name: "transform.set".to_string(),
        params: serde_json::json!({
            "ids": [obj_id.0],
            "x": 20.0,
            "y": 30.0,
            "anchor": "Centre"
        }),
    };
    
    let result = fx.run(cmd);
    assert!(matches!(result, Ok(_)));
    
    let obj = fx.object(obj_id);
    assert_eq!(obj.pos[0], -30.0);
    assert_eq!(obj.pos[1], 5.0);
}

#[test]
fn test_transform_set_default_anchor_top_left() {
    let mut fx = Fixture::new();
    let obj_id = fx.add_object("rect", [10.0, 10.0], [100.0, 50.0]);
    let cmd = Command {
        name: "transform.set".to_string(),
        params: serde_json::json!({ "ids": [obj_id.0], "x": 20.0, "y": 30.0 }),
    };
    assert!(fx.run(cmd).is_ok());
    let obj = fx.object(obj_id);
    assert_eq!(obj.pos, [20.0, 30.0]);
}

#[test]
fn test_transform_aspect_lock() {
    let mut fx = Fixture::new();
    let obj_id = fx.add_object("rect", [0.0, 0.0], [100.0, 50.0]);
    
    let cmd = Command {
        name: "transform.set".to_string(),
        params: serde_json::json!({
            "ids": [obj_id.0],
            "w": 200.0,
            "aspect_locked": true
        }),
    };
    
    let result = fx.run(cmd);
    assert!(matches!(result, Ok(_)));
    
    let obj = fx.object(obj_id);
    assert_eq!(obj.size[0], 200.0);
    assert_eq!(obj.size[1], 100.0);
}

#[test]
fn test_transform_mixed_w_two_objects() {
    let mut fx = Fixture::new();
    let obj1_id = fx.add_object("rect", [0.0, 0.0], [100.0, 50.0]);
    let obj2_id = fx.add_object("rect", [10.0, 10.0], [200.0, 80.0]);
    fx.select(obj1_id);
    fx.select(obj2_id);
    
    let cmd = Command {
        name: "transform.query".to_string(),
        params: serde_json::json!({}),
    };
    
    let result = fx.run(cmd);
    assert!(matches!(result, Ok(_)));
}

#[test]
fn test_parse_field_with_unit_suffix() {
    use dtool_core::transform::parse_field;
    use dtool_core::document::Units;
    
    let result = parse_field("10mm", Units::Mm, 96.0);
    assert!(result.is_ok());
    let value = result.unwrap();
    assert!(value > 0.0);
}

#[test]
fn test_parse_field_invalid_text() {
    use dtool_core::transform::parse_field;
    use dtool_core::document::Units;
    
    let result = parse_field("invalid", Units::Px, 96.0);
    assert!(result.is_err());
}

#[test]
fn test_transform_flip_horizontal() {
    let mut fx = Fixture::new();
    let obj_id = fx.add_object("rect", [10.0, 10.0], [100.0, 50.0]);
    
    let cmd = Command {
        name: "transform.flip".to_string(),
        params: serde_json::json!({
            "ids": [obj_id.0],
            "axis": "Horizontal",
            "anchor": "Centre"
        }),
    };
    
    let result = fx.run(cmd);
    assert!(matches!(result, Ok(_)));
}

#[test]
fn test_transform_rotate_90_left() {
    let mut fx = Fixture::new();
    let obj_id = fx.add_object("rect", [10.0, 10.0], [100.0, 50.0]);
    
    let cmd = Command {
        name: "transform.rotate_90".to_string(),
        params: serde_json::json!({
            "ids": [obj_id.0],
            "direction": "Left",
            "anchor": "Centre"
        }),
    };
    
    let result = fx.run(cmd);
    assert!(matches!(result, Ok(_)));
}

#[test]
fn test_transform_set_rotation_shear() {
    let mut fx = Fixture::new();
    let obj_id = fx.add_object("rect", [0.0, 0.0], [100.0, 50.0]);
    
    let cmd = Command {
        name: "transform.set".to_string(),
        params: serde_json::json!({
            "ids": [obj_id.0],
            "rotation_deg": 45.0,
            "shear_deg": 10.0,
            "anchor": "Centre"
        }),
    };
    
    let result = fx.run(cmd);
    assert!(matches!(result, Ok(_)));
    
    let obj = fx.object(obj_id);
    assert_eq!(obj.rotation_deg, 45.0);
    assert_eq!(obj.shear_deg, 10.0);
}

#[test]
fn test_transform_panel_focus_selection() {
    let mut fx = Fixture::new();
    let obj_id = fx.add_object("rect", [0.0, 0.0], [100.0, 50.0]);
    fx.select(obj_id);
    
    let cmd = Command {
        name: "transform.query".to_string(),
        params: serde_json::json!({}),
    };
    
    let result = fx.run(cmd);
    assert!(matches!(result, Ok(_)));
}

#[test]
fn test_transform_flip_horizontal_centre() {
    let mut fx = Fixture::new();
    let obj_id = fx.add_object("rect", [10.0, 10.0], [100.0, 50.0]);
    
    let cmd = Command {
        name: "transform.flip".to_string(),
        params: serde_json::json!({
            "ids": [obj_id.0],
            "axis": "Horizontal",
            "anchor": "Centre"
        }),
    };
    
    let result = fx.run(cmd);
    assert!(matches!(result, Ok(_)));
    
    let obj = fx.object(obj_id);
    assert_eq!(obj.pos[0], 10.0);
    assert_eq!(obj.pos[1], 10.0);
}

#[test]
fn test_transform_flip_horizontal_top_left() {
    let mut fx = Fixture::new();
    let obj_id = fx.add_object("rect", [10.0, 10.0], [100.0, 50.0]);
    
    let cmd = Command {
        name: "transform.flip".to_string(),
        params: serde_json::json!({
            "ids": [obj_id.0],
            "axis": "Horizontal",
            "anchor": "TopLeft"
        }),
    };
    
    let result = fx.run(cmd);
    assert!(matches!(result, Ok(_)));
    
    let obj = fx.object(obj_id);
    assert_eq!(obj.pos[0], 10.0);
    assert_eq!(obj.pos[1], 10.0);
}

#[test]
fn test_transform_flip_vertical_centre() {
    let mut fx = Fixture::new();
    let obj_id = fx.add_object("rect", [10.0, 10.0], [100.0, 50.0]);
    
    let cmd = Command {
        name: "transform.flip".to_string(),
        params: serde_json::json!({
            "ids": [obj_id.0],
            "axis": "Vertical",
            "anchor": "Centre"
        }),
    };
    
    let result = fx.run(cmd);
    assert!(matches!(result, Ok(_)));
    
    let obj = fx.object(obj_id);
    assert_eq!(obj.pos[0], 10.0);
    assert_eq!(obj.pos[1], 10.0);
}

#[test]
fn test_transform_flip_vertical_top_left() {
    let mut fx = Fixture::new();
    let obj_id = fx.add_object("rect", [10.0, 10.0], [100.0, 50.0]);
    
    let cmd = Command {
        name: "transform.flip".to_string(),
        params: serde_json::json!({
            "ids": [obj_id.0],
            "axis": "Vertical",
            "anchor": "TopLeft"
        }),
    };
    
    let result = fx.run(cmd);
    assert!(matches!(result, Ok(_)));
    
    let obj = fx.object(obj_id);
    assert_eq!(obj.pos[0], 10.0);
    assert_eq!(obj.pos[1], 10.0);
}

#[test]
fn test_transform_rotate_90_left_centre() {
    let mut fx = Fixture::new();
    let obj_id = fx.add_object("rect", [10.0, 10.0], [100.0, 50.0]);
    
    let cmd = Command {
        name: "transform.rotate_90".to_string(),
        params: serde_json::json!({
            "ids": [obj_id.0],
            "direction": "Left",
            "anchor": "Centre"
        }),
    };
    
    let result = fx.run(cmd);
    assert!(matches!(result, Ok(_)));
    
    let obj = fx.object(obj_id);
    assert_eq!(obj.rotation_deg, -90.0);
    assert_eq!(obj.size, [100.0, 50.0]);
}

#[test]
fn test_transform_rotate_90_right_centre() {
    let mut fx = Fixture::new();
    let obj_id = fx.add_object("rect", [10.0, 10.0], [100.0, 50.0]);
    
    let cmd = Command {
        name: "transform.rotate_90".to_string(),
        params: serde_json::json!({
            "ids": [obj_id.0],
            "direction": "Right",
            "anchor": "Centre"
        }),
    };
    
    let result = fx.run(cmd);
    assert!(matches!(result, Ok(_)));
    
    let obj = fx.object(obj_id);
    assert_eq!(obj.rotation_deg, 90.0);
    assert_eq!(obj.size, [100.0, 50.0]);
}

#[test]
fn test_transform_flip_horizontal_undo() {
    let mut fx = Fixture::new();
    let obj_id = fx.add_object("rect", [10.0, 10.0], [100.0, 50.0]);
    
    let obj_before = fx.object(obj_id);
    let pos_before = obj_before.pos;

    let cmd = Command {
        name: "transform.flip".to_string(),
        params: serde_json::json!({
            "ids": [obj_id.0],
            "axis": "Horizontal",
            "anchor": "Centre"
        }),
    };
    
    let result = fx.run(cmd);
    assert!(matches!(result, Ok(_)));
    
    let _ = fx.undo();
    
    let obj_after = fx.object(obj_id);
    assert_eq!(obj_after.pos, pos_before);
}

#[test]
fn test_transform_rotate_90_left_undo() {
    let mut fx = Fixture::new();
    let obj_id = fx.add_object("rect", [10.0, 10.0], [100.0, 50.0]);
    
    let obj_before = fx.object(obj_id);
    let rot_before = obj_before.rotation_deg;

    let cmd = Command {
        name: "transform.rotate_90".to_string(),
        params: serde_json::json!({
            "ids": [obj_id.0],
            "direction": "Left",
            "anchor": "Centre"
        }),
    };
    
    let result = fx.run(cmd);
    assert!(matches!(result, Ok(_)));
    
    let _ = fx.undo();
    
    let obj_after = fx.object(obj_id);
    assert_eq!(obj_after.rotation_deg, rot_before);
}

#[test]
fn test_transform_rotate_90_right_undo() {
    let mut fx = Fixture::new();
    let obj_id = fx.add_object("rect", [10.0, 10.0], [100.0, 50.0]);
    
    let obj_before = fx.object(obj_id);
    let rot_before = obj_before.rotation_deg;

    let cmd = Command {
        name: "transform.rotate_90".to_string(),
        params: serde_json::json!({
            "ids": [obj_id.0],
            "direction": "Right",
            "anchor": "Centre"
        }),
    };
    
    let result = fx.run(cmd);
    assert!(matches!(result, Ok(_)));
    
    let _ = fx.undo();
    
    let obj_after = fx.object(obj_id);
    assert_eq!(obj_after.rotation_deg, rot_before);
}

#[test]
fn test_transform_flip_vertical_undo() {
    let mut fx = Fixture::new();
    let obj_id = fx.add_object("rect", [10.0, 10.0], [100.0, 50.0]);
    
    let obj_before = fx.object(obj_id);
    let pos_before = obj_before.pos;

    let cmd = Command {
        name: "transform.flip".to_string(),
        params: serde_json::json!({
            "ids": [obj_id.0],
            "axis": "Vertical",
            "anchor": "Centre"
        }),
    };
    
    let result = fx.run(cmd);
    assert!(matches!(result, Ok(_)));
    
    let _ = fx.undo();
    
    let obj_after = fx.object(obj_id);
    assert_eq!(obj_after.pos, pos_before);
}
