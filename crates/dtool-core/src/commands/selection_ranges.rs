use crate::command::Session;
use crate::document::Document;
use crate::object::{Object,ObjectId};
use crate::raster_view::RasterView;
use crate::selection::{PixelSelection,PixelSurface,SelectionOp};
use dtool_protocol::{Command,CommandError,CommandResult};
use serde_json::json;

const RESTORE:&str="selection.ranges.restore";
fn kind(command:&Command)->Option<&str> {
    for name in ["selection.hueRange","selection.tonalRange","selection.alphaRange"] {if command.name==name || command.name.strip_prefix(name).is_some_and(|s|s.starts_with('.')) {return Some(name);}}
    None
}
fn source_object<'a>(session:&'a Session,command:&Command)->Option<&'a Object> {
    if let Some(node)=command.params.get("node") {
        let id=node.as_u64().or_else(||node.as_str().and_then(|s|s.strip_prefix("node:").unwrap_or(s).parse().ok()))?;
        return session.document.get_object(ObjectId(id));
    }
    if let Some(id)=session.selection.ids.first() {return session.document.get_object(*id);}
    session.document.objects.iter().rev().find(|o|o.pickable && (session.document.pixel_layers.contains(o.id) || o.raster_data.is_some()))
}
fn readable(session:&Session,command:&Command)->bool {source_object(session,command).is_some_and(|o|o.pickable && (session.document.pixel_layers.contains(o.id) || (o.raster_data.is_some()&&o.raster_size.is_some())))}
pub fn can_execute(session:&Session,command:&Command)->Option<bool> {
    if command.name==RESTORE {return Some(false);}
    kind(command).map(|_|session.has_document && readable(session,command))
}
pub fn run(session:&mut Session,command:&Command)->Option<CommandResult> {
    if command.name==RESTORE {return Some(Err(CommandError::invalid_params()));}
    let mode=kind(command)?;
    Some((|| {
        if !session.has_document {return Err(CommandError::no_document());}
        let key=if mode=="selection.hueRange" {"hue"} else {"range"};
        let value=command.name.strip_prefix(mode).and_then(|s|s.strip_prefix('.')).or_else(||command.params.get(key).and_then(|v|v.as_str())).ok_or_else(CommandError::invalid_params)?;
        let valid=match mode {"selection.hueRange"=>["reds","yellows","greens","cyans","blues","magentas"].contains(&value),"selection.tonalRange"=>["highlights","midtones","shadows"].contains(&value),_=>["transparent","partial","opaque"].contains(&value)};
        if !valid {return Err(CommandError::invalid_params());}
        let op:SelectionOp=match command.params.get("op") {None=>SelectionOp::New,Some(value)=>serde_json::from_value(value.clone()).map_err(|_|CommandError::invalid_params())?};
        let source=source_object(session,command).ok_or_else(||CommandError::new("NO_PIXEL_LAYER"))?;
        let doc=&session.document;
        let index=crate::data_merge::layout::object_page_index(doc,source.id);
        let page_id=(index!=0).then(||doc.pages.get(index).map(|page|page.id)).flatten();
        let surface=PixelSurface::for_page(doc,page_id).ok_or_else(CommandError::no_document)?;
        let [width,height]=surface.size;
        let rgba=document_raster(doc,source,surface)?;
        let src=RasterView::new(width,height,&rgba);
        let mut mask=match mode {"selection.hueRange"=>crate::selection::hue_range_mask(&src,value),"selection.tonalRange"=>crate::selection::tonal_range_mask(&src,value),_=>crate::selection::alpha_range_mask(&src,value)};
        if let Some(previous)=&session.document.pixel_selection {
            if op!=SelectionOp::New {
                if !surface.matches(doc,previous) || previous.coverage.len()!=mask.coverage.len() {return Err(CommandError::invalid_params());}
                let mut combined=previous.clone();combined.combine(&mask,op);mask=combined;
            }
        } else if matches!(op,SelectionOp::Subtract|SelectionOp::Intersect) {mask=PixelSelection::empty(width,height);}
        mask.page_id=page_id;
        let generation=session.generation.checked_add(1).ok_or_else(CommandError::invalid_params)?;
        let undo=Command::with_params(RESTORE,json!({"selection":session.document.pixel_selection}));
        let redo=Command::with_params(RESTORE,json!({"selection":mask}));
        let reply=json!({"doc_version":generation,"selected":!mask.is_empty(),"area_px":mask.area(),"bounds":mask.bounds()});
        session.document.pixel_selection=Some(mask);session.generation=generation;
        session.undo.push(crate::undo::Change{name:mode.to_owned(),undo,redo});Ok(reply)
    })())
}
pub fn apply(session:&mut Session,command:&Command)->Option<Result<(),CommandError>> {
    if command.name!=RESTORE {return None;}
    Some((|| {
        let selection:Option<PixelSelection>=serde_json::from_value(command.params.get("selection").cloned().ok_or_else(CommandError::invalid_params)?).map_err(|_|CommandError::invalid_params())?;
        if selection.as_ref().is_some_and(|mask|(mask.width as usize).checked_mul(mask.height as usize)!=Some(mask.coverage.len())) {return Err(CommandError::invalid_params());}
        session.document.pixel_selection=selection;Ok(())
    })())
}
fn document_raster(doc:&Document,object:&Object,surface:PixelSurface)->Result<Vec<u8>,CommandError> {
    if !object.pickable {return Err(CommandError::new("NO_PIXEL_LAYER"));}
    let native=doc.pixel_layers.get(object.id);
    let legacy=object.raster_data.as_ref().zip(object.raster_size);
    let (source_w,source_h)=if let Some(layer)=native {(layer.width,layer.height)} else if let Some((_,size))=legacy {size} else {return Err(CommandError::new("NO_PIXEL_LAYER"));};
    if source_w==0 || source_h==0 {return Err(CommandError::invalid_params());}
    let source_len=(source_w as usize).checked_mul(source_h as usize).and_then(|v|v.checked_mul(4)).ok_or_else(CommandError::invalid_params)?;
    if legacy.is_some_and(|(data,_)|data.len()<source_len) && native.is_none() {return Err(CommandError::invalid_params());}
    let [width,height]=surface.size;
    let factor=f64::from(surface.scale);
    let [origin_x,origin_y,_,_]=surface.bounds.map(f64::from);
    if u64::from(width)*u64::from(height)>32*1024*1024 {return Err(CommandError::validation("PIXEL_SELECTION_TOO_LARGE"));}
    if !factor.is_finite() || factor<=0.0 || object.pos.iter().chain(object.size.iter()).any(|v|!v.is_finite()) || !object.rotation_deg.is_finite() || !object.shear_deg.is_finite() || !object.opacity.is_finite() {return Err(CommandError::invalid_params());}
    let size=[if object.size[0]==0.0 {f64::from(source_w)/factor} else {f64::from(object.size[0])},if object.size[1]==0.0 {f64::from(source_h)/factor} else {f64::from(object.size[1])}];
    let length=(width as usize).checked_mul(height as usize).and_then(|v|v.checked_mul(4)).ok_or_else(CommandError::invalid_params)?;
    let mut bytes=Vec::new();bytes.try_reserve_exact(length).map_err(|_|CommandError::invalid_params())?;bytes.resize(length,0);
    let (sin,cos)=f64::from(object.rotation_deg).to_radians().sin_cos();let shear=f64::from(object.shear_deg).to_radians().tan();
    for y in 0..height {for x in 0..width {
        let dx=(f64::from(x)+0.5)/factor+origin_x-f64::from(object.pos[0]);let dy=(f64::from(y)+0.5)/factor+origin_y-f64::from(object.pos[1]);
        let ly=-sin*dx+cos*dy;let lx=cos*dx+sin*dy-shear*ly;
        let u=lx/size[0]*f64::from(source_w);let v=ly/size[1]*f64::from(source_h);
        if !(0.0..f64::from(source_w)).contains(&u) || !(0.0..f64::from(source_h)).contains(&v) {continue;}
        let mut pixel=if let Some(layer)=native {layer.get(u as u32,v as u32)} else {
            let data=legacy.ok_or_else(CommandError::invalid_params)?.0;let at=((v as usize)*source_w as usize+u as usize)*4;[data[at],data[at+1],data[at+2],data[at+3]]
        };
        pixel[3]=(f32::from(pixel[3])*object.opacity.clamp(0.0,1.0)).round() as u8;
        let at=(y as usize*width as usize+x as usize)*4;bytes[at..at+4].copy_from_slice(&pixel);
    }}Ok(bytes)
}
