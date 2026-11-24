use dtool_core::commands::warp::{kind_is_warp_group, warp_group, warp_group_kind};
use dtool_core::document::{Story, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::workspace::{ContextId, Scope, Workspace};
use dtool_core::StoryId;
use dtool_protocol::Command;
use serde_json::{json, Value};

fn new_workspace() -> (Workspace, ContextId) {
    let mut workspace = Workspace::default();
    let (target, result) = workspace.run(
        Scope::active(),
        Command::with_params(
            "doc.new",
            json!({"width": 400.0, "height": 200.0, "units": Units::Px, "dpi": 72.0, "pages": 1}),
        ),
    );
    let reply = result.expect("doc.new");
    let context = target
        .or_else(|| reply["context_id"].as_u64().map(ContextId))
        .expect("context");
    (workspace, context)
}

fn run(workspace: &mut Workspace, context: ContextId, name: &str, params: Value) -> Value {
    let (_, result) = workspace.run(Scope::of(context), Command::with_params(name, params));
    result.unwrap_or_else(|err| panic!("{name} failed: {} {}", err.code, err.message_key))
}

#[test]
fn mute_and_bake_and_text_kind_are_preserved() {
    let (mut workspace, context) = new_workspace();
    run(
        &mut workspace,
        context,
        "object.create",
        json!({"name": "child", "pos": [0.0, 0.0], "size": [100.0, 100.0]}),
    );
    let child = workspace.get(context).unwrap().session.document.objects.last().unwrap().id.0;
    let group_reply = run(&mut workspace, context, "warp.group.create", json!({ "nodes": [child] }));
    let group = ObjectId(group_reply["group_id"].as_u64().unwrap());
    let session = &workspace.get(context).unwrap().session;
    let carrier = session.document.get_object(group).unwrap();
    assert!(kind_is_warp_group(&carrier.kind));
    assert_eq!(format!("{:?}", carrier.kind), format!("{:?}", warp_group_kind()));

    let mesh = run(&mut workspace, context, "warp.mesh.query", json!({}));
    let corner = mesh["mesh"]["points"][0]["id"].as_u64().unwrap();
    run(
        &mut workspace,
        context,
        "warp.mesh.edit",
        json!({ "ops": [{ "point": corner, "pos": [-40.0, -40.0] }] }),
    );
    run(&mut workspace, context, "warp.group.update", json!({ "muted": true }));
    let before = workspace.get(context).unwrap().session.document.get_object(ObjectId(child)).unwrap().clone();
    run(&mut workspace, context, "warp.convert_to_curves", json!({ "group": group.0 }));
    let baked = workspace.get(context).unwrap().session.document.get_object(ObjectId(child)).unwrap();
    assert!(before.pos[0] - baked.pos[0] < 1.0);
    assert!(warp_group(&workspace.get(context).unwrap().session, group).is_none());
}

#[test]
fn create_over_frame_text_does_not_bake_story() {
    let (mut workspace, context) = new_workspace();
    let story = StoryId(1);
    {
        let session = &mut workspace.get_mut(context).unwrap().session;
        session.document.stories.push(Story {
            id: story,
            text: "hello".into(),
            ranges: Vec::new(),
            specials: Vec::new(),
            paras: Vec::new(),
        });
        session.document.create_object(Object {
            pos: [10.0, 10.0],
            size: [120.0, 40.0],
            kind: ObjectKind::FrameText { story },
            frame: Some(dtool_core::FrameProps::from_rect([120.0, 40.0])),
            ..Object::default()
        });
    }
    let child = workspace.get(context).unwrap().session.document.objects.last().unwrap().id;
    run(&mut workspace, context, "warp.group.create", json!({ "nodes": [child.0] }));
    let object = workspace.get(context).unwrap().session.document.get_object(child).unwrap();
    match object.kind {
        ObjectKind::FrameText { story: kept } => assert_eq!(kept, story),
        _ => panic!("kind changed"),
    }
}
