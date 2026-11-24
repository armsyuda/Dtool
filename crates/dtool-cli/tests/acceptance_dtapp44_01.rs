
mod workspace_b28_contract {
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::paint::{Colour, Paint};
use dtool_core::workspace::{ContextId, Scope, Workspace};
use dtool_core::document::Units;
use dtool_protocol::{Command, CommandError};
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn id(value:&Value)->u64 {
    value.as_u64().or_else(||value.as_str().and_then(|text|text.strip_prefix("node:").unwrap_or(text).parse().ok())).expect("missing actual node identity")
}
fn strings(value:&Value,result:&mut Vec<String>) {
    match value {
        Value::String(text)=>result.push(text.clone()),
        Value::Array(items)=>for item in items {strings(item,result);},
        Value::Object(fields)=>for item in fields.values() {strings(item,result);},
        _=>{}
    }
}
struct Fixture {workspace:Workspace,context:ContextId,ids:[u64;3]}
impl Fixture {
    fn new()->Self {
        let mut workspace=Workspace::default();
        let (target,result)=workspace.run(Scope::active(),Command::with_params("doc.new",json!({"width":100.0,"height":100.0,"units":Units::Px,"dpi":72.0,"pages":1})));
        let reply=result.expect("canonical Workspace doc.new route missing");
        let context=target.or_else(||reply["context_id"].as_u64().map(ContextId)).expect("new document returned no context");
        assert!(workspace.get(context).is_some());
        let mut fixture=Self {workspace,context,ids:[0;3]};
        for (index,(name,pos,colour)) in [("A",[10.0,10.0],[255,0,0,255]),("B",[20.0,20.0],[0,255,0,255]),("C",[60.0,60.0],[0,0,255,255])].into_iter().enumerate() {
            let fill=Paint::Solid {colour:Colour::new(colour[0],colour[1],colour[2],colour[3])};
            let reply=fixture.run("object.create",json!({"name":name,"pos":pos,"size":[30.0,30.0],"fill":fill,"opacity":1.0}));
            fixture.ids[index]=id(&reply["changes"][0]["id"]);
        }
        assert_eq!(fixture.ids.iter().copied().collect::<BTreeSet<_>>().len(),3);
        fixture
    }
    fn call(&mut self,name:&str,params:Value)->Result<Value,CommandError> {
        let (target,result)=self.workspace.run(Scope::of(self.context),Command::with_params(name,params));
        assert_eq!(target,Some(self.context));result
    }
    fn run(&mut self,name:&str,params:Value)->Value {self.call(name,params).unwrap_or_else(|error|panic!("{name} failed: {error:?}"))}
    fn generation(&self)->u64 {self.workspace.get(self.context).unwrap().session.generation}
    fn objects(&self)->Value {serde_json::to_value(&self.workspace.get(self.context).unwrap().session.document.objects).unwrap()}
    fn state(&self)->Value {
        let session=&self.workspace.get(self.context).unwrap().session;
        json!({"objects":session.document.objects,"selection":session.selection.ids})
    }
    fn object(&self,node:u64)->Object {self.workspace.get(self.context).unwrap().session.document.get_object(ObjectId(node)).expect("original object disappeared").clone()}
    fn undo(&mut self) {self.run("edit.undo",json!({}));}
    fn group(&mut self,nodes:Vec<u64>,name:&str)->u64 {
        let generation=self.generation();let targets:Vec<_>=nodes.iter().map(|node|format!("node:{node}")).collect();
        let reply=self.run("node.group",json!({"nodes":targets,"name":name}));assert_eq!(self.generation(),generation+1);id(&reply["group"])
    }
    fn display(&mut self)->Vec<Value> {
        let reply=self.run("canvas.display_list",json!({}));
        reply["items"].as_array().expect("real canvas display list required").clone()
    }
    fn tree(&mut self)->Vec<Value> {
        let reply=self.run("node.tree.query",json!({}));
        let entries=reply.as_array().or_else(||reply["tree"].as_array()).or_else(||reply["nodes"].as_array()).expect("real layer tree entries required");
        assert!(!entries.is_empty());entries.clone()
    }
    fn fixture_undo_only(&mut self) {
        for node in self.ids.into_iter().rev() {
            self.undo();assert!(self.workspace.get(self.context).unwrap().session.document.get_object(ObjectId(node)).is_none(),"extra operation undo step remained before fixture creation history");
        }
        assert!(self.workspace.get(self.context).unwrap().session.document.objects.is_empty());
        assert!(!self.workspace.get(self.context).unwrap().session.undo.can_undo());
    }
}
fn displayed_bounds(items:&[Value],node:u64)->Value {
    items.iter().find(|item|id(&item["object_id"])==node).expect("visible original missing from display list")["bounds"].clone()
}
fn assert_flag(entries:&[Value],node:u64,visible:bool,locked:bool) {
    let entry=entries.iter().find(|entry|id(&entry["id"])==node).expect("original missing from tree");
    assert_eq!(entry["flags"]["visible"],json!(visible));assert_eq!(entry["flags"]["locked"],json!(locked));
}

#[test]
fn group_01_groupab_preserves_original_ids_visible_bounds_and_ungroup_undo() {
    let mut fixture=Fixture::new();let [a,b,c]=fixture.ids;
    let original=fixture.state();let original_objects=fixture.objects();let c_before=serde_json::to_value(fixture.object(c)).unwrap();let displayed=fixture.display();
    let group=fixture.group(vec![a,b],"GroupAB");let grouped=fixture.state();
    assert_eq!(fixture.object(group).name,"GroupAB");assert!(matches!(fixture.object(group).kind,ObjectKind::Group));
    assert_eq!(fixture.object(a).parent_id,Some(ObjectId(group)));assert_eq!(fixture.object(b).parent_id,Some(ObjectId(group)));
    let after_group=fixture.display();for node in [a,b,c] {assert_eq!(displayed_bounds(&after_group,node),displayed_bounds(&displayed,node));}
    assert_eq!(serde_json::to_value(fixture.object(c)).unwrap(),c_before);
    let generation=fixture.generation();fixture.run("node.ungroup",json!({"node":group,"all":false}));assert_eq!(fixture.generation(),generation+1);
    assert_eq!(fixture.objects(),original_objects);
    fixture.undo();assert_eq!(fixture.state(),grouped);
    fixture.undo();assert_eq!(fixture.state(),original);
    fixture.fixture_undo_only();
}

#[test]
fn nested_ungroup_all_and_select_inside_keep_original_identities() {
    let mut fixture=Fixture::new();let [a,b,c]=fixture.ids;let original=fixture.state();
    let inner=fixture.group(vec![a,b],"GroupAB");let inner_state=fixture.state();
    fixture.run("selection.set",json!({"ids":[inner]}));fixture.run("selection.enter",json!({}));
    let selection=fixture.run("selection.query",json!({}));let selected=selection["ids"].as_array().expect("object selection required");
    assert!(!selected.is_empty());assert!(selected.iter().all(|value|[a,b].contains(&id(value))));
    fixture.undo();fixture.undo();assert_eq!(fixture.state(),inner_state);
    let outer=fixture.group(vec![inner,c],"Outer");let outer_state=fixture.state();
    fixture.run("node.ungroup",json!({"node":outer,"all":true}));
    let session=&fixture.workspace.get(fixture.context).unwrap().session;
    assert_eq!(session.document.objects.iter().map(|object|object.id.0).collect::<BTreeSet<_>>(),[a,b,c].into_iter().collect());
    assert!(session.document.objects.iter().all(|object|!matches!(&object.kind,ObjectKind::Group)));
    fixture.undo();assert_eq!(fixture.state(),outer_state);
    fixture.undo();assert_eq!(fixture.state(),inner_state);
    fixture.undo();assert_eq!(fixture.state(),original);
    fixture.fixture_undo_only();
}

#[test]
fn group_02_lock_reply_and_hidden_tree_state_agree_and_c_is_unchanged() {
    let mut fixture=Fixture::new();let [a,b,c]=fixture.ids;let original=fixture.state();let c_before=serde_json::to_value(fixture.object(c)).unwrap();
    let reply=fixture.run("node.lock_state",json!({"nodes":[a],"state":"locked"}));assert_eq!(reply["locked"],json!(true));let locked=fixture.state();
    fixture.run("node.update",json!({"nodes":[b],"set":{"visible":false}}));
    let tree=fixture.tree();assert_flag(&tree,a,true,true);assert_flag(&tree,b,false,false);assert_flag(&tree,c,true,false);
    assert_eq!(serde_json::to_value(fixture.object(c)).unwrap(),c_before);
    assert!(!fixture.display().iter().any(|item|id(&item["object_id"])==b));
    fixture.undo();assert_eq!(fixture.state(),locked);
    fixture.undo();assert_eq!(fixture.state(),original);fixture.fixture_undo_only();
}

#[test]
fn lock_unlock_unlock_all_hide_show_show_all_each_have_one_exact_undo() {
    let mut fixture=Fixture::new();let [a,b,c]=fixture.ids;let mut states=vec![fixture.state()];
    let operations=[
        ("node.lock_state",json!({"nodes":[a],"state":"locked"}),[(true,true),(true,false),(true,false)]),
        ("node.lock_state",json!({"nodes":[a],"state":"unlocked"}),[(true,false),(true,false),(true,false)]),
        ("node.lock_state",json!({"nodes":[a,b],"state":"locked"}),[(true,true),(true,true),(true,false)]),
        ("node.unlock_all",json!({}),[(true,false),(true,false),(true,false)]),
        ("node.update",json!({"nodes":[b],"set":{"visible":false}}),[(true,false),(false,false),(true,false)]),
        ("node.update",json!({"nodes":[b],"set":{"visible":true}}),[(true,false),(true,false),(true,false)]),
        ("node.update",json!({"nodes":[a,c],"set":{"visible":false}}),[(false,false),(true,false),(false,false)]),
        ("node.show_all",json!({}),[(true,false),(true,false),(true,false)]),
    ];
    for (name,params,expected) in operations {
        let generation=fixture.generation();fixture.run(name,params);assert_eq!(fixture.generation(),generation+1);
        let tree=fixture.tree();for (node,(visible,locked)) in fixture.ids.into_iter().zip(expected) {assert_flag(&tree,node,visible,locked);}
        let displayed=fixture.display();
        let actual:BTreeSet<_>=displayed.iter().map(|item|id(&item["object_id"])).filter(|node|fixture.ids.contains(node)).collect();
        let visible:BTreeSet<_>=fixture.ids.into_iter().zip(expected).filter_map(|(node,(visible,_))|visible.then_some(node)).collect();assert_eq!(actual,visible);
        states.push(fixture.state());
    }
    for before in states[..states.len()-1].iter().rev() {fixture.undo();assert_eq!(&fixture.state(),before);}
    fixture.fixture_undo_only();
}

#[test]
fn locked_delete_needs_confirmation_is_atomic_and_yes_undo_restores() {
    let mut fixture=Fixture::new();let [a,_,_]=fixture.ids;let original=fixture.state();
    fixture.run("node.lock_state",json!({"nodes":[a],"state":"locked"}));let locked=fixture.state();let generation=fixture.generation();
    let error=fixture.call("node.delete",json!({"nodes":[a],"confirm_locked":false})).expect_err("locked deletion bypassed confirmation");assert_eq!(error.code,"LOCKED_NEEDS_CONFIRM");
    assert_eq!(fixture.state(),locked);assert_eq!(fixture.generation(),generation);
    fixture.run("node.delete",json!({"nodes":[a],"confirm_locked":true}));assert_eq!(fixture.generation(),generation+1);
    assert!(fixture.workspace.get(fixture.context).unwrap().session.document.get_object(ObjectId(a)).is_none());
    fixture.undo();assert_eq!(fixture.state(),locked);fixture.undo();assert_eq!(fixture.state(),original);fixture.fixture_undo_only();
}

#[test]
fn locked_delete_ui_no_keeps_state_and_yes_uses_shared_one_step_delete() {
    let mut fixture=Fixture::new();let [a,_,_]=fixture.ids;let original=fixture.state();
    fixture.run("node.lock_state",json!({"nodes":[a],"state":"locked"}));let locked=fixture.state();
    fixture.run("selection.set",json!({"ids":[a]}));let selected=fixture.state();let generation=fixture.generation();
    let error=fixture.call("ui.action.invoke",json!({"action":"delete","nodes":[a]})).expect_err("locked GUI delete did not request confirmation");assert_eq!(error.code,"LOCKED_NEEDS_CONFIRM");
    fixture.run("ui.action.invoke",json!({"action":"delete_confirm_no"}));assert_eq!(fixture.state(),selected);assert_eq!(fixture.generation(),generation);
    let error=fixture.call("ui.action.invoke",json!({"action":"delete","nodes":[a]})).expect_err("second locked delete did not request confirmation");assert_eq!(error.code,"LOCKED_NEEDS_CONFIRM");
    fixture.run("ui.action.invoke",json!({"action":"delete_confirm_yes"}));assert_eq!(fixture.generation(),generation+1);
    assert!(fixture.workspace.get(fixture.context).unwrap().session.document.get_object(ObjectId(a)).is_none());
    fixture.undo();assert_eq!(fixture.state(),selected);fixture.undo();assert_eq!(fixture.state(),locked);fixture.undo();assert_eq!(fixture.state(),original);fixture.fixture_undo_only();
}

#[test]
fn group_03_front_back_and_single_steps_preserve_identity_geometry_styles_and_render_order() {
    for (step,expected) in [("to_front",[0,2,1]),("to_back",[1,0,2]),("forward",[0,2,1]),("backward",[1,0,2])] {
        let mut fixture=Fixture::new();let [a,b,c]=fixture.ids;let original=fixture.state();let before:Vec<_>=fixture.ids.into_iter().map(|node|serde_json::to_value(fixture.object(node)).unwrap()).collect();
        let generation=fixture.generation();fixture.run("node.arrange",json!({"nodes":[format!("node:{b}")],"step":step}));assert_eq!(fixture.generation(),generation+1);
        let objects=&fixture.workspace.get(fixture.context).unwrap().session.document.objects;
        assert_eq!(objects.iter().filter(|object|object.parent_id.is_none()).map(|object|object.id.0).collect::<Vec<_>>(),expected.map(|index|fixture.ids[index]).to_vec());
        for (node,before) in fixture.ids.into_iter().zip(before) {assert_eq!(serde_json::to_value(fixture.object(node)).unwrap(),before);}
        let displayed=fixture.display();let render_order:Vec<_>=displayed.iter().map(|item|id(&item["object_id"])).filter(|node|[a,b,c].contains(node)).collect();
        assert_eq!(render_order,expected.map(|index|fixture.ids[index]).to_vec());
        let front=render_order.iter().rev().find(|&&node| {
            let bounds=displayed_bounds(&displayed,node);let x=bounds[0].as_f64().unwrap();let y=bounds[1].as_f64().unwrap();let w=bounds[2].as_f64().unwrap();let h=bounds[3].as_f64().unwrap();
            x<=25.0 && y<=25.0 && x+w>25.0 && y+h>25.0
        }).copied().expect("overlap has no rendered item");
        assert_eq!(front,if matches!(step,"to_front"|"forward") {b} else {a});
        fixture.undo();assert_eq!(fixture.state(),original);fixture.fixture_undo_only();
    }
}

#[test]
fn group_04_ui_action_routes_once_and_actual_layer_menu_query_is_read_only() {
    let mut fixture=Fixture::new();let [a,b,c]=fixture.ids;let original=fixture.state();let generation=fixture.generation();
    let reply=fixture.run("ui.action.invoke",json!({"action":"group","nodes":[a,b]}));let group=id(&reply["group"]);assert_eq!(fixture.generation(),generation+1);
    assert_eq!(fixture.object(a).parent_id,Some(ObjectId(group)));assert_eq!(fixture.object(b).parent_id,Some(ObjectId(group)));assert_eq!(fixture.object(c).parent_id,None);
    let before_query=fixture.state();let generation=fixture.generation();let reply=fixture.run("ui.menu.query",json!({"menu":"layer"}));
    let mut keys=Vec::new();strings(&reply,&mut keys);
    let ordered=["menu.layer.group","menu.layer.ungroup","menu.layer.ungroup_all","menu.layer.new_group","menu.layer.lock"];
    let positions:Vec<_>=ordered.iter().map(|key|keys.iter().position(|value|value==key).expect("actual Layer menu key missing")).collect();assert!(positions.windows(2).all(|pair|pair[0]<pair[1]));
    for key in ["menu.layer.unlock","menu.layer.unlock_all","menu.layer.hide","menu.layer.show","menu.layer.show_all"] {assert!(keys.iter().any(|value|value==key));}
    assert_eq!(fixture.state(),before_query);assert_eq!(fixture.generation(),generation);
    fixture.undo();assert_eq!(fixture.state(),original);fixture.fixture_undo_only();
}

}
