use crate::command::Session;
use crate::curve::Curve;
use crate::object::{Object,ObjectId,ObjectKind};
use crate::paint::{Paint,Colour};
use crate::raster_view::RasterView;
use crate::trace::{TraceSettings,TraceMode,trace_image};
use dtool_protocol::{Command,CommandError,CommandResult};
use serde_json::json;

const RESTORE:&str="selection.toCurves.restore";
pub fn can_execute(session:&Session,command:&Command)->Option<bool> {
    match command.name.as_str() {
        "selection.toCurves"=>Some(session.has_document && session.document.pixel_selection.as_ref().is_some_and(|mask| {
            (mask.width as usize).checked_mul(mask.height as usize)==Some(mask.coverage.len()) && mask.coverage.iter().any(|v|*v>=128)
        })),
        RESTORE=>Some(false),
        _=>None,
    }
}
pub fn run(session:&mut Session,command:&Command)->Option<CommandResult> {
    if command.name==RESTORE {return Some(Err(CommandError::invalid_params()));}
    if command.name!="selection.toCurves" {return None;}
    Some(convert(session,command))
}
pub fn apply(session:&mut Session,command:&Command)->Option<Result<(),CommandError>> {
    if command.name!=RESTORE {return None;}
    Some((|| {
        let objects:Vec<Object>=serde_json::from_value(command.params["objects"].clone()).map_err(|_|CommandError::invalid_params())?;
        let selection:crate::selection::Selection=serde_json::from_value(command.params["selection"].clone()).map_err(|_|CommandError::invalid_params())?;
        let next=command.params["next_object_id"].as_u64().ok_or_else(CommandError::invalid_params)?;
        let version=session.document.version.checked_add(1).ok_or_else(CommandError::invalid_params)?;
        session.document.objects=objects;session.document.next_object_id=next;session.selection=selection;session.document.version=version;Ok(())
    })())
}
fn convert(session:&mut Session,command:&Command)->CommandResult {
    if !session.has_document {return Err(CommandError::no_document());}
    if can_execute(session,command)!=Some(true) {return Err(CommandError::new("NOTHING_SELECTED"));}
    let mask=session.document.pixel_selection.as_ref().ok_or_else(CommandError::invalid_params)?;
    let size=mask.coverage.len().checked_mul(4).ok_or_else(CommandError::invalid_params)?;
    let mut rgba=Vec::new();rgba.try_reserve_exact(size).map_err(|_|CommandError::invalid_params())?;
    for coverage in &mask.coverage {let value=if *coverage>=128 {0} else {255};rgba.extend_from_slice(&[value,value,value,255]);}
    let settings=TraceSettings {mode:TraceMode::BlackWhite,threshold:128,tolerance:0.5,min_area_px:1,ignore_white:true,fills:true,strokes:false,..TraceSettings::default()};
    let paths=trace_image(&RasterView::new(mask.width,mask.height,&rgba),&settings);
    let mut path=kurbo::BezPath::new();let mut fill=None;
    for traced in paths {if fill.is_none() {fill=traced.fill;}for el in traced.path.elements() {path.push(*el);}}
    if path.elements().is_empty() {return Err(CommandError::new("NOTHING_SELECTED"));}
    let surface=crate::selection::PixelSurface::for_page(&session.document,mask.page_id).ok_or_else(CommandError::invalid_params)?;
    let [x,y,_,_]=surface.bounds.map(f64::from);
    let scale=1.0/f64::from(surface.scale);
    let path=crate::geometry::transform_bez_path(&path,x,y,scale,scale,0.0);
    let bounds=crate::geometry::bez_path_bounds(&path);
    if bounds.iter().any(|v|!v.is_finite()) {return Err(CommandError::invalid_params());}
    let local=crate::geometry::transform_bez_path(&path,-f64::from(bounds[0]),-f64::from(bounds[1]),1.0,1.0,0.0);
    let active=session.selection.ids.iter().find_map(|id|session.document.objects.iter().position(|o|o.id==*id)).or_else(||session.document.objects.len().checked_sub(1));
    let parent=active.filter(|&i| mask.page_id.is_none() || session.document.pages
        .get(crate::data_merge::layout::object_page_index(&session.document,session.document.objects[i].id))
        .is_some_and(|page|page.id==surface.page_id)).and_then(|i|session.document.objects[i].parent_id);
    let maximum=session.document.objects.iter().map(|o|o.id.0).max().unwrap_or(0);
    let id=session.document.next_object_id.max(maximum.checked_add(1).ok_or_else(CommandError::invalid_params)?);
    let next=id.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    let generation=session.generation.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    let version=session.document.version.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    let size=[bounds[2]-bounds[0],bounds[3]-bounds[1]];
    let object=Object {id:ObjectId(id),name:dtool_strings::text("selection.curve_name").to_owned(),pos:[bounds[0],bounds[1]],size,page_id:mask.page_id,parent_id:parent,kind:ObjectKind::Curve(Curve::from_kurbo(&local)),fill:fill.map(|c|Paint::Solid {colour:Colour::new(c[0],c[1],c[2],c[3])}).unwrap_or(Paint::None),..Object::default()};
    let undo=Command::with_params(RESTORE,json!({"objects":session.document.objects,"selection":session.selection,"next_object_id":session.document.next_object_id}));
    let mut objects=session.document.objects.clone();let mut insert=active.map(|i|i+1).unwrap_or(objects.len());
    if let Some(index)=active {
        let active_id=objects[index].id;
        while insert<objects.len() {
            let mut parent=objects[insert].parent_id;let mut descendant=false;
            for _ in 0..objects.len() {let Some(id)=parent else {break;};if id==active_id {descendant=true;break;}parent=objects.iter().find(|o|o.id==id).and_then(|o|o.parent_id);}
            if !descendant {break;}insert+=1;
        }
    }
    objects.insert(insert,object);
    let selection=crate::selection::Selection::from_ids(vec![ObjectId(id)]);
    let redo=Command::with_params(RESTORE,json!({"objects":objects,"selection":selection,"next_object_id":next}));
    session.document.objects=objects;session.document.next_object_id=next;session.selection=selection;session.generation=generation;session.document.version=version;
    session.undo.push(crate::undo::Change{name:command.name.clone(),undo,redo});
    Ok(json!({"object_id":id,"doc_version":generation}))
}
