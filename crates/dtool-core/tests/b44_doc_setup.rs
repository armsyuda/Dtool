use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, Paint};
use dtool_core::{Command, CommandError, Session};
use serde_json::{Value, json};

fn session() -> Session {
    let mut document = Document::new([720.0,360.0],Units::Points,72.0,1);
    document.objects.push(Object {
        id:ObjectId(7),pos:[72.0,36.0],size:[144.0,72.0],
        fill:Paint::Solid { colour:Colour::new(180,90,45,255) },
        ..Object::default()
    });
    document.next_object_id=8;
    Session::new(document)
}

fn command(session: &mut Session,name: &str,params: Value) -> Result<Value,CommandError> {
    session.run(&Command::with_params(name,params))
}

fn run(session: &mut Session,name: &str,params: Value) -> Value {
    command(session,name,params).unwrap_or_else(|error|panic!("{name} failed: {error:?}"))
}

fn query(session: &mut Session) -> Value {
    let value=run(session,"doc.query",json!({}));
    assert!(value["doc_version"].is_u64(),"doc.query must return a version");
    let snapshot=&value["snapshot"];
    assert!(snapshot.is_object(),"doc.query must return its full snapshot");
    assert_eq!(snapshot["pages"],serde_json::to_value(&session.document.pages).unwrap(),"empty/synthetic page snapshot is not acceptance evidence");
    assert_eq!(snapshot["objects"],serde_json::to_value(&session.document.objects).unwrap(),"empty/synthetic object snapshot is not acceptance evidence");
    value
}

fn content(query: &Value) -> Value {
    let mut snapshot=query["snapshot"].clone();
    snapshot.as_object_mut().expect("missing snapshot").remove("version");
    snapshot
}

fn near(actual: f64,expected: f64) {
    assert!((actual-expected).abs()<0.002,"{actual} != {expected}");
}

fn setup(units: &str,scale: f64,dpi: f64) -> Value {
    json!({
        "units":units,"dpi":dpi,"page_size":[720.0*scale,360.0*scale],"landscape":true,
        "margins":{"enabled":true,"left":18.0*scale,"right":24.0*scale,"top":18.0*scale,"bottom":24.0*scale,"linked":false},
        "bleed":{"enabled":true,"left":3.0*scale,"right":3.0*scale,"top":3.0*scale,"bottom":3.0*scale,"linked":true}
    })
}

fn geometry_points(session: &Session) -> Vec<(u64,[f32;2],[f32;2])> {
    let scale=match session.document.units {
        Units::Pt=>1.0,Units::Px=>72.0/session.document.dpi,Units::Mm=>72.0/25.4,Units::Cm=>72.0/2.54,Units::In=>72.0,
    };
    let mut result:Vec<_>=session.document.objects.iter().map(|object|(object.id.0,[object.pos[0]*scale,object.pos[1]*scale],[object.size[0]*scale,object.size[1]*scale])).collect();
    result.sort_by_key(|row|row.0);
    result
}

#[test]
fn size_units_dpi_orientation_margins_and_bleed_round_trip() {
    let mut session=session();
    let _=query(&mut session);
    run(&mut session,"doc.setup.apply",setup("points",1.0,144.0));
    let points=run(&mut session,"doc.setup.query",json!({}));
    near(points["page_size"][0].as_f64().expect("width"),720.0);
    near(points["page_size"][1].as_f64().expect("height"),360.0);
    near(points["dpi"].as_f64().expect("dpi"),144.0);
    assert_eq!(points["landscape"],json!(true));
    for (side,expected) in [("left",18.0),("right",24.0),("top",18.0),("bottom",24.0)] {
        near(points["margins"][side].as_f64().expect("margin"),expected);
        near(points["bleed"][side].as_f64().expect("bleed"),3.0);
    }
    run(&mut session,"doc.setup.apply",setup("millimeters",25.4/72.0,144.0));
    let millimeters=run(&mut session,"doc.setup.query",json!({}));
    near(millimeters["page_size"][0].as_f64().expect("mm width"),254.0);
    near(millimeters["margins"]["left"].as_f64().expect("mm margin"),6.35);
    near(millimeters["bleed"]["left"].as_f64().expect("mm bleed"),3.0*25.4/72.0);
    let _=query(&mut session);
    run(&mut session,"doc.setup.apply",setup("points",1.0,144.0));
    let restored=run(&mut session,"doc.setup.query",json!({}));
    assert_eq!(restored["units"],points["units"]);
    assert_eq!(restored["landscape"],points["landscape"]);
    near(restored["dpi"].as_f64().unwrap(),144.0);
    near(restored["page_size"][0].as_f64().unwrap(),720.0);
    near(restored["page_size"][1].as_f64().unwrap(),360.0);
    for (side,expected) in [("left",18.0),("right",24.0),("top",18.0),("bottom",24.0)] {
        near(restored["margins"][side].as_f64().unwrap(),expected);
        near(restored["bleed"][side].as_f64().unwrap(),3.0);
    }
    let _=query(&mut session);
}

#[test]
fn units_change_preserves_point_geometry_and_object_ids() {
    let mut session=session();
    let before=query(&mut session);
    let geometry=geometry_points(&session);
    run(&mut session,"doc.setup.apply",setup("millimeters",25.4/72.0,72.0));
    let after=query(&mut session);
    let changed=geometry_points(&session);
    assert_eq!(geometry.len(),changed.len());
    for (old,new) in geometry.iter().zip(&changed) {
        assert_eq!(old.0,new.0);
        for axis in 0..2 { near(old.1[axis] as f64,new.1[axis] as f64);near(old.2[axis] as f64,new.2[axis] as f64); }
    }
    assert_ne!(before["snapshot"]["units"],after["snapshot"]["units"]);
}

#[test]
fn invalid_width_ten_with_six_point_margins_is_atomic() {
    let mut session=session();
    let before=query(&mut session);
    let mut params=setup("points",1.0,72.0);
    params["page_size"]=json!([10.0,100.0]);
    params["margins"]=json!({"enabled":true,"left":6.0,"right":6.0,"top":0.0,"bottom":0.0,"linked":false});
    params["bleed"]=json!({"enabled":false,"left":0.0,"right":0.0,"top":0.0,"bottom":0.0,"linked":true});
    let error=command(&mut session,"doc.setup.apply",params).expect_err("invalid margins unexpectedly applied");
    assert_eq!(error.code,"INVALID_PARAMS");
    assert_eq!(query(&mut session),before);
    assert!(!session.undo.can_undo());
}

#[test]
fn one_apply_produces_one_undo_and_restores_the_snapshot() {
    let mut session=session();
    let before=query(&mut session);
    run(&mut session,"doc.setup.apply",setup("points",1.0,144.0));
    let after=query(&mut session);
    assert_ne!(content(&after),content(&before));
    assert_eq!(after["doc_version"].as_u64().unwrap(),before["doc_version"].as_u64().unwrap()+1);
    assert!(session.undo.can_undo());
    run(&mut session,"edit.undo",json!({}));
    assert_eq!(content(&query(&mut session)),content(&before));
    assert!(!session.undo.can_undo(),"one setup apply must not leave extra undo steps");
    assert!(session.undo.can_redo());
}

#[test]
fn preview_cancel_restores_exactly_without_version_or_undo() {
    let mut session=session();
    let before=query(&mut session);
    let preview=run(&mut session,"doc.setup.preview",setup("points",0.5,144.0));
    let token=preview["token"].as_u64().expect("preview must return a token");
    assert!(!session.undo.can_undo());
    assert_eq!(session.generation,before["doc_version"].as_u64().unwrap());
    run(&mut session,"doc.setup.cancel",json!({"token":token}));
    assert_eq!(query(&mut session),before);
    assert!(!session.undo.can_undo());
}

#[test]
fn assign_keeps_values_convert_changes_values_and_one_undo_restores() {
    let mut session=session();
    session.document.colour.profile="Unprofiled fixture".to_owned();
    let before=query(&mut session);
    let original_fill=serde_json::to_value(&session.document.objects[0].fill).unwrap();
    run(&mut session,"color.profile.assign",json!({"profile":"sRGB IEC61966-2.1"}));
    let assigned=query(&mut session);
    assert_ne!(content(&assigned),content(&before));
    assert_eq!(serde_json::to_value(&session.document.objects[0].fill).unwrap(),original_fill);
    assert_eq!(session.document.colour.profile,"sRGB IEC61966-2.1");
    let generation=session.generation;
    run(&mut session,"color.profile.convert",json!({
        "format":dtool_core::colour::ColourFormat::LAB_16.to_string(),"profile":"LAB D50",
        "intent":serde_json::to_value(dtool_core::colour::RenderingIntent::default()).unwrap(),"bpc":true
    }));
    let converted=query(&mut session);
    assert_ne!(content(&converted),content(&assigned));
    assert_ne!(serde_json::to_value(&session.document.objects[0].fill).unwrap(),original_fill,"profile conversion must transform colour values");
    assert_eq!(session.generation,generation+1);
    run(&mut session,"edit.undo",json!({}));
    assert_eq!(content(&query(&mut session)),content(&assigned));
    assert_eq!(serde_json::to_value(&session.document.objects[0].fill).unwrap(),original_fill);
}

#[test]
fn expired_preview_token_is_stale_and_leaves_state_unchanged() {
    let mut session=session();
    let before=query(&mut session);
    let preview=run(&mut session,"doc.setup.preview",setup("points",0.5,144.0));
    let token=preview["token"].as_u64().expect("preview token");
    run(&mut session,"doc.setup.cancel",json!({"token":token}));
    let mut params=setup("points",1.0,144.0);params["token"]=json!(token);
    let error=command(&mut session,"doc.setup.apply",params).expect_err("expired preview token was accepted");
    assert_eq!(error.code,"STALE_PREVIEW");
    assert_eq!(query(&mut session),before);
    assert!(!session.undo.can_undo());
}

#[test]
fn facing_arrange_start_reflow_origin_and_transparent_round_trip() {
    let mut session=session();
    let before=query(&mut session);
    run(&mut session,"doc.setup.apply",json!({
        "facing_pages":false,"arrange_vertical":true,"start_on_right":false,"reflow_pages":true,
        "page_origin":[36.0,18.0],"transparent_background":true
    }));
    let after=query(&mut session);
    assert_ne!(content(&after),content(&before));
    let setup=&after["snapshot"]["setup"];
    assert_eq!(setup["facing_pages"],json!(false));
    assert_eq!(setup["arrange_vertical"],json!(true));
    assert_eq!(setup["start_on_right"],json!(false));
    assert_eq!(setup["reflow_pages"],json!(true));
    assert_eq!(setup["transparent_background"],json!(true));
    assert_eq!(setup["page_origin"],json!([36.0,18.0]));
    let stored:Document=serde_json::from_value(after["snapshot"].clone()).expect("snapshot must load as a document");
    assert_eq!(serde_json::to_value(&stored.setup).unwrap(),setup.clone());
    run(&mut session,"edit.undo",json!({}));
    assert_eq!(content(&query(&mut session)),content(&before));
}

#[test]
fn width_change_preview_then_cancel_restores_without_undo() {
    let mut session=session();
    let before=query(&mut session);
    let mut params=setup("points",1.0,72.0);
    params["page_size"]=json!([800.0,360.0]);
    let preview=run(&mut session,"doc.setup.preview",params);
    let token=preview["token"].as_u64().expect("preview token");
    near(session.document.pages[0].size[0] as f64,800.0);
    run(&mut session,"doc.setup.cancel",json!({"token":token}));
    assert_eq!(query(&mut session),before);
    assert!(!session.undo.can_undo());
}