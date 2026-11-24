use crate::command::Session;
use crate::curve::Curve;
use crate::object::{Object, ObjectId, ObjectKind};
use crate::paint::{Colour, Paint, StrokeStyle};
use crate::raster_view::RasterView;
use crate::trace::{TraceSettings, trace_image, validate_settings};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::json;

pub const TRACE_RESTORE:&str="image.trace.restore";
fn ours(command:&Command)->bool {matches!(command.name.as_str(),"image.trace"|"image.trace.preview")}
fn node(command:&Command)->Result<ObjectId,CommandError> {
    let value=&command.params["node"];
    let id=value.as_u64().or_else(||value.as_str().and_then(|text|text.strip_prefix("node:").unwrap_or(text).parse().ok())).ok_or_else(CommandError::invalid_params)?;
    Ok(ObjectId(id))
}
pub fn can_execute(session:&Session,command:&Command)->Option<bool> {
    if !ours(command) { return None; }
    Some(session.has_document && node(command).ok().or_else(|| selected_source(session))
        .and_then(|id| session.document.get_object(id))
        .is_some_and(|object| crate::raster_source::dimensions(&session.document, object).is_some()))
}

pub fn selected_source(session: &Session) -> Option<ObjectId> {
    let [id] = session.selection.ids.as_slice() else { return None };
    let object = session.document.get_object(*id)?;
    crate::raster_source::dimensions(&session.document, object).map(|_| *id)
}
pub fn run(session:&mut Session,command:&Command)->Option<CommandResult> {
    if !ours(command) {return None;}
    Some(execute(session,command))
}
pub fn apply(session:&mut Session,command:&Command)->Option<Result<(),CommandError>> {
    if command.name!=TRACE_RESTORE {return None;}
    Some((|| {
        let objects:Vec<Object>=serde_json::from_value(command.params["objects"].clone()).map_err(|_|CommandError::invalid_params())?;
        let next=command.params["next_object_id"].as_u64().ok_or_else(CommandError::invalid_params)?;
        session.document.objects=objects;session.document.next_object_id=next;Ok(())
    })())
}
fn execute(session:&mut Session,command:&Command)->CommandResult {
    if !session.has_document {return Err(CommandError::no_document());}
    let id=node(command)?;
    let source=session.document.objects.iter().find(|object|object.id==id).cloned().ok_or_else(||CommandError::new("NO_PIXEL_LAYER"))?;
    let settings:TraceSettings=if let Some(value)=command.params.get("settings") {serde_json::from_value(value.clone()).map_err(|_|CommandError::invalid_params())?} else {TraceSettings::default()};
    if !validate_settings(&settings) {return Err(CommandError::invalid_params());}
    if command.params.get("preset").is_some() {return Err(CommandError::invalid_params());}
    let ([width, height], rgba) = crate::raster_source::rgba(&session.document, &source)
        .ok_or_else(|| CommandError::new("NO_PIXEL_LAYER"))?;
    let width = u32::try_from(width).map_err(|_| CommandError::invalid_params())?;
    let height = u32::try_from(height).map_err(|_| CommandError::invalid_params())?;
    if width==0 || height==0 || source.size.iter().any(|v|!v.is_finite()||*v==0.0) || source.pos.iter().any(|v|!v.is_finite()) || !source.rotation_deg.is_finite() || !source.shear_deg.is_finite() {return Err(CommandError::invalid_params());}
    let paths=trace_image(&RasterView::new(width,height,&rgba),&settings);
    if command.name=="image.trace.preview" {return Ok(json!({"path_count":paths.len()}));}
    if paths.is_empty() {return Ok(json!({"group_id":null,"path_count":0}));}
    let count=u64::try_from(paths.len()).map_err(|_|CommandError::invalid_params())?;
    let max_id=session.document.objects.iter().map(|object|object.id.0).max().unwrap_or(0);
    let first=session.document.next_object_id.max(max_id.checked_add(1).ok_or_else(CommandError::invalid_params)?);
    let next=first.checked_add(count).and_then(|n|n.checked_add(1)).ok_or_else(CommandError::invalid_params)?;
    let generation=session.generation.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    let version=session.document.version.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    let mut additions=Vec::with_capacity(paths.len()+1);
    let group=ObjectId(first);
    additions.push(Object {id:group,name:dtool_strings::text("trace.group_name").to_owned(),kind:ObjectKind::Group,parent_id:source.parent_id,..Object::default()});
    for (index,traced) in paths.into_iter().enumerate() {
        let scaled=crate::geometry::transform_bez_path(&traced.path,0.0,0.0,f64::from(source.size[0])/f64::from(width),f64::from(source.size[1])/f64::from(height),0.0);
        let fill=traced.fill.map(|c|Paint::Solid {colour:Colour::new(c[0],c[1],c[2],c[3])}).unwrap_or(Paint::None);
        let mut stroke=StrokeStyle::default();
        if let Some((colour, stroke_width)) = traced.stroke {
            stroke.paint = Paint::Solid {colour: Colour::new(colour[0], colour[1], colour[2], colour[3])};
            let x_scale = source.size[0].abs() / width as f32;
            let y_scale = source.size[1].abs() / height as f32;
            stroke.width = stroke_width * x_scale.sqrt() * y_scale.sqrt();
        }
        additions.push(Object {
            id:ObjectId(first+1+index as u64),pos:source.pos,size:source.size,rotation_deg:source.rotation_deg,shear_deg:source.shear_deg,
            opacity:source.opacity,fill,stroke,kind:ObjectKind::Curve(Curve::from_kurbo(&scaled).with_source(id.0)),parent_id:Some(group),..Object::default()
        });
    }
    let undo=Command::with_params(TRACE_RESTORE,json!({"objects":session.document.objects,"next_object_id":session.document.next_object_id}));
    let mut objects=session.document.objects.clone();
    let mut insert=objects.iter().position(|object|object.id==id).unwrap()+1;
    while insert<objects.len() {
        let mut parent=objects[insert].parent_id;let mut descendant=false;
        for _ in 0..objects.len() {
            let Some(parent_id)=parent else {break;};if parent_id==id {descendant=true;break;}
            parent=objects.iter().find(|object|object.id==parent_id).and_then(|object|object.parent_id);
        }
        if !descendant {break;}insert+=1;
    }
    objects.splice(insert..insert,additions);
    let redo=Command::with_params(TRACE_RESTORE,json!({"objects":objects,"next_object_id":next}));
    session.document.objects=objects;session.document.next_object_id=next;
    session.undo.push(crate::undo::Change {name:command.name.clone(),undo,redo});
    session.generation=generation;session.document.version=version;
    Ok(json!({"group_id":group.0,"path_count":count,"doc_version":session.generation}))
}

#[cfg(test)]
mod contracts {
    use super::*;
    use crate::document::{Document,Units};
    use crate::pixel::PixelLayer;
    #[test]
    fn preview_is_unchanged_and_trace_has_one_exact_restore() {
        let mut document=Document::new([10.0,10.0],Units::Points,72.0,1);
        document.objects.push(Object {id:ObjectId(1),size:[10.0,10.0],..Object::default()});document.next_object_id=2;
        let mut rgba=vec![0;400];for pixel in rgba.chunks_exact_mut(4) {pixel[3]=255;}
        document.pixel_layers.insert(ObjectId(1),PixelLayer::from_rgba(10,10,&rgba));
        let mut session=Session::new(document);let before=serde_json::to_value(&session.document).unwrap();
        let preview=Command::with_params("image.trace.preview",json!({"node":1,"settings":{"ignore_white":true}}));
        assert_eq!(run(&mut session,&preview).unwrap().unwrap()["path_count"],json!(1));
        assert_eq!(serde_json::to_value(&session.document).unwrap(),before);assert!(!session.undo.can_undo());
        let command=Command::with_params("image.trace",preview.params);
        let reply=run(&mut session,&command).unwrap().unwrap();let group=reply["group_id"].as_u64().unwrap();
        assert_eq!(reply["path_count"],json!(1));assert_eq!(session.document.objects.len(),3);
        assert!(matches!(session.document.objects[1].kind,ObjectKind::Group));
        assert_eq!(session.document.objects[2].parent_id,Some(ObjectId(group)));
        let undo=session.undo.undo().expect("missing trace undo");apply(&mut session,&undo).unwrap().unwrap();
        assert_eq!(serde_json::to_value(&session.document.objects).unwrap(),before["objects"]);
        assert_eq!(session.document.next_object_id,before["next_object_id"].as_u64().unwrap());
        assert!(!session.undo.can_undo());
        let redo=session.undo.redo().expect("missing trace redo");apply(&mut session,&redo).unwrap().unwrap();
        assert_eq!(session.document.objects.len(),3);assert_eq!(session.document.objects[1].id,ObjectId(group));
    }
}
