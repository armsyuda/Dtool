use crate::command::Session;
use crate::curve::Curve;
use crate::object::{Object, ObjectId, ObjectKind};
use crate::paint::{Colour, Paint, StrokeStyle};
use crate::raster_view::RasterView;
use crate::trace::{TraceSettings, trace_image, validate_settings};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::json;
use kurbo::Shape;

pub const TRACE_RESTORE:&str="image.trace.restore";
const TRACE_UPDATE: &str = "image.trace.update";

#[derive(serde::Deserialize, serde::Serialize)]
struct TraceUpdate {
    source: ObjectId,
    visible: bool,
    insert_at: usize,
    next_object_id: u64,
    selection: Vec<ObjectId>,
    added: Vec<Object>,
    removed: Vec<ObjectId>,
}

impl TraceUpdate {
    fn apply(self, session: &mut Session) -> Result<(), CommandError> {
        let doc = &mut session.document;
        if doc.get_object(self.source).is_none() || self.insert_at > doc.objects.len()
            || self.added.iter().any(|object| doc.get_object(object.id).is_some())
        {
            return Err(CommandError::invalid_params());
        }
        if self.removed.is_empty() {
            doc.objects.splice(self.insert_at..self.insert_at, self.added);
        } else {
            let removed: std::collections::HashSet<_> = self.removed.into_iter().collect();
            doc.objects.retain(|object| !removed.contains(&object.id));
        }
        doc.find_object_mut(self.source.0).unwrap().pickable = self.visible;
        doc.next_object_id = self.next_object_id;
        session.selection.ids = self.selection;
        Ok(())
    }
}
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
    if command.name == TRACE_UPDATE {
        return Some(serde_json::from_value::<TraceUpdate>(command.params.clone())
            .map_err(|_| CommandError::invalid_params()).and_then(|update| update.apply(session)));
    }
    if command.name!=TRACE_RESTORE {return None;}
    Some((|| {
        let objects:Vec<Object>=serde_json::from_value(command.params["objects"].clone()).map_err(|_|CommandError::invalid_params())?;
        let next=command.params["next_object_id"].as_u64().ok_or_else(CommandError::invalid_params)?;
        session.document.objects=objects;session.document.next_object_id=next;
        if let Some(selection) = command.params.get("selection") {
            session.selection.ids = serde_json::from_value(selection.clone()).map_err(|_| CommandError::invalid_params())?;
        }
        Ok(())
    })())
}
fn execute(session:&mut Session,command:&Command)->CommandResult {
    if !session.has_document {return Err(CommandError::no_document());}
    let id=node(command).or_else(|error| selected_source(session).ok_or(error))?;
    let source=session.document.get_object(id).ok_or_else(||CommandError::new("NO_PIXEL_LAYER"))?;
    let settings:TraceSettings=if let Some(value)=command.params.get("settings") {serde_json::from_value(value.clone()).map_err(|_|CommandError::invalid_params())?} else {TraceSettings::default()};
    if !validate_settings(&settings) {return Err(CommandError::invalid_params());}
    if command.params.get("preset").is_some() {return Err(CommandError::invalid_params());}
    let ([width, height], rgba) = crate::raster_source::rgba(&session.document, source)
        .ok_or_else(|| CommandError::new("NO_PIXEL_LAYER"))?;
    let width = u32::try_from(width).map_err(|_| CommandError::invalid_params())?;
    let height = u32::try_from(height).map_err(|_| CommandError::invalid_params())?;
    if width==0 || height==0 || source.size.iter().any(|v|!v.is_finite()||*v==0.0) || source.pos.iter().any(|v|!v.is_finite()) || !source.rotation_deg.is_finite() || !source.shear_deg.is_finite() {return Err(CommandError::invalid_params());}
    let paths=trace_image(&RasterView::new(width,height,&rgba),&settings);
    if command.name=="image.trace.preview" {return Ok(json!({"path_count":paths.len()}));}
    apply_paths(session, id, [width, height], paths, false, settings.auto_group)
}

pub fn apply_paths(
    session: &mut Session, id: ObjectId, [width, height]: [u32; 2], paths: Vec<crate::trace::TracedPath>, hide_source: bool, auto_group: bool,
) -> CommandResult {
    if !session.has_document { return Err(CommandError::no_document()); }
    let source = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
    if width == 0 || height == 0 || source.size.iter().any(|value| !value.is_finite() || *value == 0.0) {
        return Err(CommandError::invalid_params());
    }
    if paths.is_empty() {return Ok(json!({"group_id":null,"path_count":0}));}
    let count=u64::try_from(paths.len()).map_err(|_|CommandError::invalid_params())?;
    let max_id=session.document.objects.iter().map(|object|object.id.0).max().unwrap_or(0);
    let first=session.document.next_object_id.max(max_id.checked_add(1).ok_or_else(CommandError::invalid_params)?);
    let clusters = if auto_group { nested_clusters(&paths) } else { vec![None; paths.len()] };
    let cluster_roots: std::collections::BTreeSet<_> = clusters.iter().flatten().copied().collect();
    let next=first.checked_add(count).and_then(|n|n.checked_add(1 + cluster_roots.len() as u64)).ok_or_else(CommandError::invalid_params)?;
    let generation=session.generation.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    let version=session.document.version.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    let mut additions=Vec::with_capacity(paths.len()+1);
    let group=ObjectId(first);
    additions.push(Object {id:group,name:dtool_strings::text("trace.group_name").to_owned(),kind:ObjectKind::Group,parent_id:source.parent_id,..Object::default()});
    let group_ids: std::collections::BTreeMap<_, _> = cluster_roots.into_iter().enumerate()
        .map(|(index, member)| (member, ObjectId(first + count + 1 + index as u64))).collect();
    let scale = [f64::from(source.size[0])/f64::from(width),f64::from(source.size[1])/f64::from(height)];
    for (index,traced) in paths.into_iter().enumerate() {
        if let Some(&cluster) = group_ids.get(&index) {
            additions.push(Object { id: cluster, name: dtool_strings::text("trace.nested_name").into(),
                kind: ObjectKind::Group, parent_id: Some(group), ..Default::default() });
        }
        let scaled=crate::geometry::transform_bez_path(&traced.path,0.0,0.0,scale[0],scale[1],0.0);
        let fill=traced.fill_paint(scale);
        let mut stroke=StrokeStyle::default();
        if let Some((colour, stroke_width)) = traced.stroke {
            stroke.paint = Paint::Solid {colour: Colour::new(colour[0], colour[1], colour[2], colour[3])};
            let x_scale = source.size[0].abs() / width as f32;
            let y_scale = source.size[1].abs() / height as f32;
            stroke.width = stroke_width * x_scale.sqrt() * y_scale.sqrt();
        }
        let parent = clusters[index].and_then(|root| group_ids.get(&root).copied()).unwrap_or(group);
        let mut object = Object {
            id:ObjectId(first+1+index as u64),pos:source.pos,size:source.size,rotation_deg:source.rotation_deg,shear_deg:source.shear_deg,
            opacity:source.opacity,fill,stroke,kind:ObjectKind::Curve(Curve::from_kurbo(&scaled).with_source(id.0)),parent_id:Some(parent),..Object::default()
        };
        if let Some(kind) = traced.shape
            && source.rotation_deg == 0.0 && source.shear_deg == 0.0 && scale.iter().all(|scale| *scale > 0.0)
        {
            let bounds = scaled.bounding_box();
            let size = [bounds.width() as f32, bounds.height() as f32];
            if let Ok(shape) = serde_json::from_value::<crate::shape::ShapeRecipe>(json!({"kind": kind, "size": size})) {
                object.pos = [source.pos[0] + bounds.x0 as f32, source.pos[1] + bounds.y0 as f32];
                object.size = size;
                object.kind = ObjectKind::Shape;
                object.shape = Some(shape);
            }
        }
        additions.push(object);
    }
    let objects = &session.document.objects;
    let source_index = objects.iter().position(|object|object.id==id).unwrap();
    let descendants = session.document.subtree_ids(id);
    let insert_at = source_index + 1 + objects[source_index + 1..].iter()
        .take_while(|object| descendants.contains(&object.id)).count();
    let undo = TraceUpdate { source: id, visible: source.pickable, insert_at,
        next_object_id: session.document.next_object_id, selection: session.selection.ids.clone(),
        added: Vec::new(), removed: additions.iter().map(|object| object.id).collect() };
    let update = TraceUpdate { source: id, visible: source.pickable && !hide_source, insert_at,
        next_object_id: next, selection: if hide_source { vec![group] } else { session.selection.ids.clone() },
        added: additions, removed: Vec::new() };
    let redo = Command::with_params(TRACE_UPDATE, json!(update));
    let undo = Command::with_params(TRACE_UPDATE, json!(undo));
    update.apply(session)?;
    session.undo.push(crate::undo::Change {name:"image.trace".into(),undo,redo});
    session.generation=generation;session.document.version=version;
    Ok(json!({"group_id":group.0,"path_count":count,"doc_version":session.generation}))
}

fn nested_clusters(paths: &[crate::trace::TracedPath]) -> Vec<Option<usize>> {
    let bounds: Vec<_> = paths.iter().map(|path| path.path.bounding_box()).collect();
    let mut groups = vec![None; paths.len()];
    let mut cursor = 0;
    while cursor + 1 < paths.len() {
        let container = bounds[cursor];
        let mut end = cursor + 1;
        while end < paths.len() {
            let inner = bounds[end];
            if inner.x0 <= container.x0 || inner.y0 <= container.y0
                || inner.x1 >= container.x1 || inner.y1 >= container.y1
                || paths[cursor].path.winding(inner.center()) == 0
            { break; }
            end += 1;
        }
        if end > cursor + 1 { groups[cursor..end].fill(Some(cursor)); }
        cursor = end;
    }
    groups
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
