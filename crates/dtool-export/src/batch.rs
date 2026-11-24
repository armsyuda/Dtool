#[path = "batch_store.rs"]
pub mod batch_store;
use self::batch_store::BatchStore;
use crate::ExportFormat;
use crate::ExportControl;
use std::{path::PathBuf,sync::{atomic::{AtomicU64,Ordering},Arc,Condvar,Mutex,OnceLock},collections::{HashMap,HashSet}};
use serde::{Serialize,Deserialize};
use dtool_core::{Command, CommandError, CommandResult};

#[derive(Debug,Clone,Copy,PartialEq,Eq,Hash,Serialize,Deserialize)]
pub struct BatchInputId(pub u64);
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct BatchInput {pub id:BatchInputId,pub path:PathBuf,pub order:u32,pub size:u64,pub modified:u64}
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct BatchAction {pub command:String,pub params:serde_json::Value}
pub const BATCH_ACTIONS:&[&str]=&["document.resize","document.adjustments"];

#[derive(Debug,Clone,Serialize,Deserialize)]
pub enum Destination {Original,Folder(PathBuf)}
#[derive(Debug,Clone,Copy,PartialEq,Eq,Hash,Serialize,Deserialize)]
pub enum OverwritePolicy {Skip,Replace,Fail}
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct BatchOutput {
    pub format:ExportFormat,
    pub width:Option<u32>,
    pub height:Option<u32>,
    pub keep_aspect:bool,
    pub dest:Destination,
    pub name:String,
    pub overwrite:OverwritePolicy,
    #[serde(default)]
    pub webp_lossy:bool,
}

#[derive(Debug,Clone,Default,Serialize,Deserialize)]
pub struct BatchManifest {pub inputs:Vec<BatchInput>,pub actions:Vec<BatchAction>,pub outputs:Vec<BatchOutput>,pub parallel:bool}
#[derive(Debug,Clone,PartialEq,Eq,Hash,Serialize,Deserialize)]
pub struct BatchHandle(pub String);
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct BatchProblem {pub input:Option<BatchInputId>,pub code:String,pub message_key:String}

#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)]
pub enum BatchStatus {Prepared,Running,Done,Cancelled,Failed}
#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)]
pub enum FileStatus {Waiting,Working,Done,Partial,Failed,Cancelled,Untouched}
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct OutputReceipt {pub path:PathBuf,pub bytes:u64,pub complete:bool}
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct FileReceipt {
    pub input:BatchInputId,
    pub status:FileStatus,
    pub outputs:Vec<OutputReceipt>,
    pub error:Option<String>,
    #[serde(default)]
    pub warnings:Vec<String>,
}
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct BatchSnapshot {pub handle:BatchHandle,pub status:BatchStatus,pub done_units:u64,pub total_units:u64,pub files:Vec<FileReceipt>}


pub use crate::raster::{E06_OFFERED_FORMATS, is_e06_held, is_e06_offered};


pub fn e06_label_key(format:ExportFormat)->&'static str{
    match format{
        ExportFormat::Png=>"batch.format_png",
        ExportFormat::Jpeg=>"batch.format_jpeg",
        ExportFormat::Webp=>"batch.format_webp",
        ExportFormat::Tiff=>"batch.format_tiff",
        ExportFormat::Gif=>"batch.format_gif",
        ExportFormat::Pdf=>"batch.format_pdf",
        ExportFormat::Svg=>"batch.format_svg",
        ExportFormat::Eps=>"batch.format_eps",
        ExportFormat::Wmf=>"batch.format_wmf",
    }
}

pub fn e06_format_label(format:ExportFormat)->&'static str{
    match format{
        ExportFormat::Png=>"PNG",
        ExportFormat::Jpeg=>"JPEG",
        ExportFormat::Webp=>"WebP",
        ExportFormat::Tiff=>"TIFF",
        ExportFormat::Gif=>"GIF",
        ExportFormat::Pdf=>"PDF",
        ExportFormat::Svg=>"SVG",
        ExportFormat::Eps=>"EPS",
        ExportFormat::Wmf=>"WMF",
    }
}

pub fn output_skip_reason(output:&BatchOutput)->Option<&'static str>{
    if is_e06_held(output.format){
        Some("FORMAT_HELD")
    }else{
        None
    }
}

fn decode_input(bytes:&[u8])->Result<image::RgbaImage,CommandError>{
    image::load_from_memory(bytes).map(|decoded|decoded.to_rgba8()).map_err(|_|CommandError::new("INPUT_DECODE_FAILED"))
}

fn resize_params(params:&serde_json::Value)->Option<(u32,u32)>{
    let width=params.get("width").and_then(|v|v.as_u64())? as u32;
    let height=params.get("height").and_then(|v|v.as_u64())? as u32;
    if width==0||height==0{None}else{Some((width,height))}
}

pub const ADJUSTMENT_KEYS:&[&str]=&["brightness","contrast","saturation","gamma"];

#[derive(Debug,Clone,Copy,PartialEq)]
pub struct Adjustments{pub brightness:f32,pub contrast:f32,pub saturation:f32,pub gamma:f32}

pub fn adjustment_params(params:&serde_json::Value)->Option<Adjustments>{
    let object=params.as_object()?;
    if object.is_empty()||object.keys().any(|key|!ADJUSTMENT_KEYS.contains(&key.as_str())){return None;}
    let read=|key:&str,default:f32,min:f32,max:f32|->Option<f32>{
        match object.get(key){
            None=>Some(default),
            Some(value)=>{let v=value.as_f64()? as f32;if v.is_finite()&&v>=min&&v<=max{Some(v)}else{None}}
        }
    };
    Some(Adjustments{brightness:read("brightness",0.0,-1.0,1.0)?,contrast:read("contrast",0.0,-1.0,1.0)?,saturation:read("saturation",0.0,-1.0,1.0)?,gamma:read("gamma",1.0,0.1,10.0)?})
}

pub fn apply_adjustments(image:&mut image::RgbaImage,adjust:Adjustments){
    let contrast=1.0+adjust.contrast;
    let saturation=1.0+adjust.saturation;
    for pixel in image.pixels_mut(){
        let mut rgb=[pixel[0] as f32/255.0,pixel[1] as f32/255.0,pixel[2] as f32/255.0];
        for c in rgb.iter_mut(){
            *c=((*c-0.5)*contrast+0.5+adjust.brightness).clamp(0.0,1.0);
        }
        let grey=0.2126*rgb[0]+0.7152*rgb[1]+0.0722*rgb[2];
        for (i,c) in rgb.iter().enumerate(){
            let v=(grey+(*c-grey)*saturation).clamp(0.0,1.0).powf(1.0/adjust.gamma);
            pixel[i]=(v*255.0).round().clamp(0.0,255.0) as u8;
        }
    }
}

fn apply_actions(manifest:&BatchManifest,mut image:image::RgbaImage)->image::RgbaImage{
    for action in &manifest.actions {
        match action.command.as_str(){
            "document.resize"=>{
                if let Some((width,height))=resize_params(&action.params){
                    image=image::imageops::resize(&image,width,height,image::imageops::FilterType::Triangle);
                }
            }
            "document.adjustments"=>{
                if let Some(adjust)=adjustment_params(&action.params){apply_adjustments(&mut image,adjust);}
            }
            _=>{}
        }
    }
    image
}

fn has_pixel_actions(manifest:&BatchManifest)->bool{
    manifest.actions.iter().any(|action|(action.command=="document.resize"&&resize_params(&action.params).is_some())||(action.command=="document.adjustments"&&adjustment_params(&action.params).is_some()))
}

fn output_size(output:&BatchOutput,width:u32,height:u32)->Option<(u32,u32)>{
    let (w,h)=match (output.width.filter(|v|*v>0),output.height.filter(|v|*v>0)){
        (None,None)=>return None,
        (Some(w),Some(h))=>{
            if output.keep_aspect&&width>0&&height>0{
                let scale=(w as f64/width as f64).min(h as f64/height as f64);
                (((width as f64*scale).round() as u32).max(1),((height as f64*scale).round() as u32).max(1))
            }else{(w,h)}
        }
        (Some(w),None)=>(w,((height as f64*w as f64/width.max(1) as f64).round() as u32).max(1)),
        (None,Some(h))=>(((width as f64*h as f64/height.max(1) as f64).round() as u32).max(1),h),
    };
    if w==width&&h==height{None}else{Some((w,h))}
}

fn png_bytes(image:&image::RgbaImage)->Result<Vec<u8>,CommandError>{
    let mut buffer=std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image.clone()).write_to(&mut buffer,image::ImageFormat::Png).map_err(|_|CommandError::new("ENCODE_FAILED"))?;
    Ok(buffer.into_inner())
}

fn base64(bytes:&[u8])->String{
    const TABLE:&[u8;64]=b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out=String::with_capacity(bytes.len().div_ceil(3)*4);
    for chunk in bytes.chunks(3){
        let b=[chunk[0],*chunk.get(1).unwrap_or(&0),*chunk.get(2).unwrap_or(&0)];
        let n=((b[0] as u32)<<16)|((b[1] as u32)<<8)|b[2] as u32;
        out.push(TABLE[(n>>18) as usize&63] as char);
        out.push(TABLE[(n>>12) as usize&63] as char);
        out.push(if chunk.len()>1{TABLE[(n>>6) as usize&63] as char}else{'='});
        out.push(if chunk.len()>2{TABLE[n as usize&63] as char}else{'='});
    }
    out
}

fn svg_bytes(image:&image::RgbaImage)->Result<Vec<u8>,CommandError>{
    let (w,h)=image.dimensions();
    let data=base64(&png_bytes(image)?);
    Ok(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\"><image width=\"{w}\" height=\"{h}\" href=\"data:image/png;base64,{data}\"/></svg>\n").into_bytes())
}

fn pdf_bytes(image:&image::RgbaImage)->Vec<u8>{
    let (w,h)=image.dimensions();
    let rgb=crate::flatten_rgba(image.as_raw(),[255,255,255]);
    let content=format!("q {w} 0 0 {h} 0 0 cm /Im0 Do Q\n");
    let mut out:Vec<u8>=b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets=Vec::new();
    let mut object=|out:&mut Vec<u8>,head:String,stream:Option<&[u8]>|{
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{}",offsets.len(),head).as_bytes());
        if let Some(data)=stream{
            out.extend_from_slice(b"\nstream\n");
            out.extend_from_slice(data);
            out.extend_from_slice(b"\nendstream");
        }
        out.extend_from_slice(b"\nendobj\n");
    };
    object(&mut out,"<< /Type /Catalog /Pages 2 0 R >>".into(),None);
    object(&mut out,"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),None);
    object(&mut out,format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] /Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>"),None);
    object(&mut out,format!("<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length {} >>",rgb.len()),Some(&rgb));
    object(&mut out,format!("<< /Length {} >>",content.len()),Some(content.as_bytes()));
    let xref=out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n",offsets.len()+1).as_bytes());
    for offset in &offsets{out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());}
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",offsets.len()+1).as_bytes());
    out
}

fn eps_bytes(image:&image::RgbaImage)->Vec<u8>{
    let (w,h)=image.dimensions();
    let rgb=crate::flatten_rgba(image.as_raw(),[255,255,255]);
    let mut out=format!("%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 {w} {h}\n%%HiResBoundingBox: 0 0 {w} {h}\n%%Pages: 1\n%%EndComments\n%%Page: 1 1\ngsave\n/picstr {} string def\n{w} {h} scale\n{w} {h} 8 [{w} 0 0 -{h} 0 {h}] {{currentfile picstr readhexstring pop}} false 3 colorimage\n",w as usize*3);
    const HEX:&[u8;16]=b"0123456789abcdef";
    for (i,byte) in rgb.iter().enumerate(){
        out.push(HEX[(byte>>4) as usize] as char);
        out.push(HEX[(byte&15) as usize] as char);
        if i%36==35{out.push('\n');}
    }
    out.push_str("\ngrestore\nshowpage\n%%Trailer\n%%EOF\n");
    out.into_bytes()
}

fn encode_output(output:&BatchOutput,manifest:&BatchManifest,input_bytes:&[u8])->Result<Vec<u8>,CommandError>{
    let is_png=input_bytes.starts_with(&[137,80,78,71,13,10,26,10]);
    let decoded=decode_input(input_bytes)?;
    let (width,height)=decoded.dimensions();
    let resize=output_size(output,width,height);
    if output.format==ExportFormat::Png&&is_png&&!has_pixel_actions(manifest)&&resize.is_none(){
        return Ok(input_bytes.to_vec());
    }
    let mut image=apply_actions(manifest,decoded);
    let (width,height)=image.dimensions();
    if let Some((w,h))=output_size(output,width,height){
        image=image::imageops::resize(&image,w,h,image::imageops::FilterType::Triangle);
    }
    match output.format{
        ExportFormat::Png=>png_bytes(&image),
        ExportFormat::Jpeg=>crate::jpeg::encode_image_bytes_to_jpeg(&png_bytes(&image)?,90,false),
        ExportFormat::Webp=>{
            let _=crate::webp_lossy::install();
            let (w,h)=image.dimensions();
            crate::webp::encode_rgba(w,h,image.as_raw(),!output.webp_lossy,90).map_err(|error|error.to_command_error())
        }
        ExportFormat::Svg=>svg_bytes(&image),
        ExportFormat::Pdf=>Ok(pdf_bytes(&image)),
        ExportFormat::Eps=>Ok(eps_bytes(&image)),
        _=>Err(CommandError::new("FORMAT_HELD")),
    }
}

static CONTROLS:OnceLock<Mutex<HashMap<BatchHandle,ExportControl>>>=OnceLock::new();

fn controls()->&'static Mutex<HashMap<BatchHandle,ExportControl>>{
    CONTROLS.get_or_init(||Mutex::new(HashMap::new()))
}

pub fn export_control(handle:&BatchHandle)->Option<ExportControl>{
    controls().lock().ok()?.get(handle).cloned()
}

fn bind_control(handle:&BatchHandle)->ExportControl{
    let ctrl=ExportControl::new();
    if let Ok(mut map)=controls().lock(){
        map.insert(handle.clone(),ctrl.clone());
    }
    ctrl
}

static INPUT_SEQUENCE:AtomicU64=AtomicU64::new(1);
static HANDLE_SEQUENCE:AtomicU64=AtomicU64::new(1);
static PREPARED:OnceLock<Mutex<HashMap<BatchHandle,BatchManifest>>>=OnceLock::new();
static SNAPSHOTS:OnceLock<Mutex<HashMap<BatchHandle,BatchSnapshot>>>=OnceLock::new();
static CANCEL_FLAGS:OnceLock<Mutex<HashMap<BatchHandle,bool>>>=OnceLock::new();


static START_GATE: Mutex<()> = Mutex::new(());
static ACTIVE: OnceLock<Mutex<HashSet<BatchHandle>>> = OnceLock::new();
struct ActiveJob(BatchHandle);
impl Drop for ActiveJob {
    fn drop(&mut self) { if let Ok(mut active) = ACTIVE.get_or_init(||Mutex::new(HashSet::new())).lock() { active.remove(&self.0); } }
}
pub fn is_running(handle: &BatchHandle) -> bool {
    ACTIVE.get_or_init(||Mutex::new(HashSet::new())).lock().map(|active|active.contains(handle)).unwrap_or(false)
}


#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="lowercase")]
pub enum BatchBoundary { File, Encode }

struct BoundaryCtrl {
    arm: Option<(BatchBoundary, Option<BatchInputId>)>,
    parked: Option<BatchBoundary>,
    released: bool,
}

static BOUNDARIES: OnceLock<Mutex<HashMap<BatchHandle, Arc<(Mutex<BoundaryCtrl>, Condvar)>>>> = OnceLock::new();

fn boundary_entry(handle: &BatchHandle) -> Arc<(Mutex<BoundaryCtrl>, Condvar)> {
    let mut map = BOUNDARIES.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap_or_else(|poison| poison.into_inner());
    map.entry(handle.clone()).or_insert_with(|| Arc::new((Mutex::new(BoundaryCtrl { arm: None, parked: None, released: false }), Condvar::new()))).clone()
}

pub fn arm_boundary(handle: &BatchHandle, boundary: BatchBoundary, input: Option<BatchInputId>) {
    let entry = boundary_entry(handle);
    let mut ctrl = entry.0.lock().unwrap_or_else(|poison| poison.into_inner());
    ctrl.arm = Some((boundary, input));
    ctrl.parked = None;
    ctrl.released = false;
}

pub fn current_boundary(handle: &BatchHandle) -> Option<BatchBoundary> {
    let map = BOUNDARIES.get()?.lock().ok()?;
    let entry = map.get(handle)?;
    entry.0.lock().ok()?.parked
}

pub fn wait_until_parked(handle: &BatchHandle) -> Option<BatchBoundary> {
    let entry = boundary_entry(handle);
    let mut ctrl = entry.0.lock().unwrap_or_else(|poison| poison.into_inner());
    loop {
        if let Some(parked) = ctrl.parked {
            return Some(parked);
        }
        if ctrl.released || is_cancelled(handle) {
            return ctrl.parked;
        }
        if !is_running(handle) {
            return ctrl.parked;
        }
        ctrl = entry.1.wait(ctrl).unwrap_or_else(|poison| poison.into_inner());
    }
}

pub fn release_boundary(handle: &BatchHandle) {
    let entry = boundary_entry(handle);
    let mut ctrl = entry.0.lock().unwrap_or_else(|poison| poison.into_inner());
    ctrl.released = true;
    entry.1.notify_all();
}

fn wake_boundary(handle: &BatchHandle) {
    if let Some(map) = BOUNDARIES.get().and_then(|value| value.lock().ok()) {
        if let Some(entry) = map.get(handle) {
            entry.1.notify_all();
        }
    }
}

fn park_if_armed(handle: &BatchHandle, boundary: BatchBoundary, input: BatchInputId) {
    let entry = boundary_entry(handle);
    let mut ctrl = entry.0.lock().unwrap_or_else(|poison| poison.into_inner());
    let armed = match ctrl.arm {
        Some((kind, None)) if kind == boundary => true,
        Some((kind, Some(id))) if kind == boundary && id == input => true,
        _ => false,
    };
    if !armed {
        return;
    }
    ctrl.arm = None;
    ctrl.parked = Some(boundary);
    entry.1.notify_all();
    while !ctrl.released && !is_cancelled(handle) {
        ctrl = entry.1.wait(ctrl).unwrap_or_else(|poison| poison.into_inner());
    }
    ctrl.parked = None;
    entry.1.notify_all();
}



fn snapshots() -> &'static Mutex<HashMap<BatchHandle, BatchSnapshot>> {
    SNAPSHOTS.get_or_init(|| Mutex::new(BatchStore::default().load_all().unwrap_or_default().into_iter().map(|snapshot| (snapshot.handle.clone(), snapshot)).collect()))
}

fn publish(snapshot: BatchSnapshot) -> Result<(), CommandError> {
    let mut map = snapshots().lock().map_err(|_| CommandError::new("EDITOR_BUSY"))?;
    map.insert(snapshot.handle.clone(), snapshot.clone());
    BatchStore::default().save(&snapshot)
}

fn publish_progress(manifest: &BatchManifest, handle: &BatchHandle, files: &[FileReceipt], done_units: u64, total_units: u64) -> Result<(), CommandError> {
    let cancelled = is_cancelled(handle);
    let mut receipts = files.to_vec();
    receipts.extend(manifest.inputs.iter().skip(files.len()).map(|input| FileReceipt { input: input.id, status: if cancelled { FileStatus::Untouched } else { FileStatus::Waiting }, outputs: Vec::new(), error: None, warnings: Vec::new() }));
    publish(BatchSnapshot { handle: handle.clone(), status: if cancelled { BatchStatus::Cancelled } else { BatchStatus::Running }, done_units, total_units, files: receipts })
}

fn problem(id:Option<BatchInputId>,code:&str)->BatchProblem{BatchProblem{input:id,code:code.into(),message_key:format!("error.{}",code.to_lowercase())}}
fn new_id()->Result<BatchInputId,BatchProblem>{INPUT_SEQUENCE.fetch_update(Ordering::SeqCst,Ordering::Relaxed,|v|v.checked_add(1)).map(BatchInputId).map_err(|_|problem(None,"INPUT_ID_EXHAUSTED"))}
fn stat(id:BatchInputId,path:PathBuf,order:u32)->Result<BatchInput,BatchProblem>{
    let metadata=std::fs::metadata(&path).map_err(|_|problem(Some(id),"INPUT_NOT_FOUND"))?;
    if !metadata.is_file(){return Err(problem(Some(id),"INPUT_NOT_FILE"));}
    let modified=metadata.modified().ok().and_then(|time|time.duration_since(std::time::UNIX_EPOCH).ok()).map(|time|time.as_millis().min(u64::MAX as u128) as u64).unwrap_or(0);
    Ok(BatchInput{id,path,order,size:metadata.len(),modified})
}
fn validate_action(action:&BatchAction)->Option<BatchProblem>{
    if !BATCH_ACTIONS.contains(&action.command.as_str()){return Some(problem(None,"ACTION_NOT_ALLOWED"));}
    match action.command.as_str(){
        "document.resize"=>{
            let width=action.params.get("width").and_then(|v|v.as_u64());
            let height=action.params.get("height").and_then(|v|v.as_u64());
            if width.is_none()||height.is_none(){return Some(problem(None,"ACTION_PARAMS_INVALID"));}
        }
        "document.adjustments"=>{
            if adjustment_params(&action.params).is_none(){return Some(problem(None,"ACTION_PARAMS_INVALID"));}
        }
        _=>return Some(problem(None,"ACTION_NOT_ALLOWED")),
    }
    None
}

fn is_reserved_name(name:&str)->bool{
    let upper=name.to_uppercase();
    if upper=="CON"||upper=="PRN"||upper=="AUX"||upper=="NUL"{return true;}
    for i in 1..=9 {if upper==format!("COM{}",i)||upper==format!("LPT{}",i){return true;}}
    false
}

fn has_bad_chars(name:&str)->bool{
    name.chars().any(|c|matches!(c,'<'|'>'|':'|'"'|'/'|'\\'|'|'|'?'))
}

fn expand_name(template:&str,input:&BatchInput,format:&ExportFormat,width:Option<u32>,height:Option<u32>)->String{
    let stem=input.path.file_stem().map(|s|s.to_string_lossy().to_string()).unwrap_or_default();
    let ext=match format { ExportFormat::Png=>"png",ExportFormat::Jpeg=>"jpg",ExportFormat::Webp=>"webp",ExportFormat::Tiff=>"tiff",ExportFormat::Gif=>"gif",ExportFormat::Pdf=>"pdf",ExportFormat::Svg=>"svg",ExportFormat::Eps=>"eps",ExportFormat::Wmf=>"wmf" };
    let w=width.map(|v|v.to_string()).unwrap_or_else(||"0".to_string());
    let h=height.map(|v|v.to_string()).unwrap_or_else(||"0".to_string());
    template.replace("{name}",&stem).replace("{format}",ext).replace("{width}",&w).replace("{height}",&h)
}

fn validate_output(output:&BatchOutput,input:&BatchInput)->Option<BatchProblem>{
    let name=expand_name(&output.name,input,&output.format,output.width,output.height);
    if is_reserved_name(&name){return Some(problem(Some(input.id),"RESERVED_NAME"));}
    if has_bad_chars(&name){return Some(problem(Some(input.id),"BAD_CHARS"));}
    None
}

fn check_collisions(manifest:&BatchManifest)->Vec<BatchProblem>{
    let mut problems=Vec::new();
    let mut seen:HashMap<String,Option<BatchInputId>>=HashMap::new();
    for output in &manifest.outputs {
        for input in &manifest.inputs {
            let name=expand_name(&output.name,input,&output.format,output.width,output.height);
            let key=name.to_lowercase();
            if let Some(existing)=seen.get(&key){
                problems.push(problem(Some(input.id),"NAME_COLLISION"));
            } else {
                seen.insert(key,Some(input.id));
            }
        }
    }
    problems
}

fn check_existing_files(manifest:&BatchManifest)->Vec<BatchProblem>{
    let mut problems=Vec::new();
    for output in &manifest.outputs {
        for input in &manifest.inputs {
            let name=expand_name(&output.name,input,&output.format,output.width,output.height);
            let path=match &output.dest {
                Destination::Original=>{
                    let dir=input.path.parent().unwrap_or(std::path::Path::new("."));
                    dir.join(&name)
                }
                Destination::Folder(folder)=>folder.join(&name),
            };
            if std::fs::metadata(&path).is_ok(){
                match output.overwrite {
                    OverwritePolicy::Fail=>problems.push(problem(Some(input.id),"FILE_EXISTS")),
                    _=>{}
                }
            }
        }
    }
    problems
}

impl BatchManifest {
    pub fn add(&mut self,path:PathBuf)->Result<BatchInputId,BatchProblem>{
        let high=self.inputs.iter().map(|input|input.id.0).max().unwrap_or(0);
        let next=high.checked_add(1).ok_or_else(||problem(None,"INPUT_ID_EXHAUSTED"))?;INPUT_SEQUENCE.fetch_max(next,Ordering::SeqCst);
        let id=new_id()?;let order=u32::try_from(self.inputs.len()).map_err(|_|problem(Some(id),"INPUT_LIMIT"))?;
        self.inputs.push(stat(id,path,order)?);Ok(id)
    }
    pub fn remove(&mut self,id:BatchInputId)->bool{
        let length=self.inputs.len();self.inputs.retain(|input|input.id!=id);self.renumber();length!=self.inputs.len()
    }
    pub fn reorder(&mut self,id:BatchInputId,to:usize)->bool{
        if to>=self.inputs.len(){return false;}let Some(from)=self.inputs.iter().position(|input|input.id==id)else{return false;};
        let input=self.inputs.remove(from);self.inputs.insert(to,input);self.renumber();true
    }
    fn renumber(&mut self){for (order,input) in self.inputs.iter_mut().enumerate(){input.order=order as u32;}}
}

pub fn validate_manifest(manifest:&BatchManifest)->Vec<BatchProblem>{
    let mut result=Vec::new();let mut ids=HashSet::new();
    for input in &manifest.inputs {
        if input.id.0==0||!ids.insert(input.id){result.push(problem(Some(input.id),"INPUT_ID_CONFLICT"));}
        if let Err(problem)=stat(input.id,input.path.clone(),input.order){result.push(problem);}
    }
    for action in &manifest.actions {
        if let Some(problem)=validate_action(action){result.push(problem);}
    }
    for output in &manifest.outputs {
        for input in &manifest.inputs {
            if let Some(problem)=validate_output(output,input){result.push(problem);}
        }
    }
    if manifest.outputs.is_empty(){result.push(problem(None,"NO_OUTPUT"));}
    result.extend(check_collisions(manifest));
    result.extend(check_existing_files(manifest));
    result
}

pub fn prepare(mut manifest:BatchManifest)->Result<BatchHandle,CommandError>{
    let problems=validate_manifest(&manifest);if let Some(problem)=problems.first(){return Err(CommandError{code:problem.code.clone(),message_key:problem.message_key.clone()});}
    for (order,input) in manifest.inputs.iter_mut().enumerate(){*input=stat(input.id,input.path.clone(),order as u32).map_err(|problem|CommandError{code:problem.code,message_key:problem.message_key})?;}
    let sequence=HANDLE_SEQUENCE.fetch_update(Ordering::SeqCst,Ordering::Relaxed,|v|v.checked_add(1)).map_err(|_|CommandError::invalid_params())?;
    let timestamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|CommandError::invalid_params())?.as_nanos();
    let handle=BatchHandle(format!("batch-{}-{}-{}",timestamp,std::process::id(),sequence));
    PREPARED.get_or_init(||Mutex::new(HashMap::new())).lock().map_err(|_|CommandError::new("EDITOR_BUSY"))?.insert(handle.clone(),manifest);Ok(handle)
}
pub fn prepared_inputs(handle:&BatchHandle)->Option<BatchManifest>{PREPARED.get()?.lock().ok()?.get(handle).cloned()}

pub fn validate(handle:&BatchHandle)->Vec<BatchProblem>{
    let manifest=prepared_inputs(handle);
    match manifest {
        Some(m)=>validate_manifest(&m),
        None=>vec![problem(None,"HANDLE_NOT_FOUND")],
    }
}

fn measure_work(manifest:&BatchManifest)->u64{
    (manifest.inputs.len() as u64).saturating_mul(manifest.outputs.len() as u64)
}

fn is_cancelled(handle:&BatchHandle)->bool{
    if export_control(handle).is_some_and(|ctrl|ctrl.is_cancelled()){return true;}
    CANCEL_FLAGS.get_or_init(||Mutex::new(HashMap::new())).lock().ok().and_then(|m|m.get(handle).copied()).unwrap_or(false)
}

fn run_batch(manifest:&BatchManifest,handle:&BatchHandle)->Result<BatchSnapshot,CommandError>{
    let mut files=Vec::new();
    let total_units=measure_work(manifest);
    let mut done_units=0u64;
    
    for input in &manifest.inputs {
        if is_cancelled(handle) {
            files.push(FileReceipt{input:input.id,status:FileStatus::Untouched,outputs:Vec::new(),error:None,warnings:Vec::new()});
            publish_progress(manifest,handle,&files,done_units,total_units)?;
            continue;
        }

        let mut outputs=Vec::new();
        let mut file_status=FileStatus::Working;
        let mut error_message=None;
        let mut warnings=Vec::new();
        let mut working=files.clone();
        working.push(FileReceipt{input:input.id,status:FileStatus::Working,outputs:Vec::new(),error:None,warnings:Vec::new()});
        publish_progress(manifest,handle,&working,done_units,total_units)?;
        park_if_armed(handle,BatchBoundary::File,input.id);
        if is_cancelled(handle) {
            files.push(FileReceipt{input:input.id,status:FileStatus::Untouched,outputs:Vec::new(),error:None,warnings:Vec::new()});
            publish_progress(manifest,handle,&files,done_units,total_units)?;
            continue;
        }

        for output in &manifest.outputs {
            if is_cancelled(handle) {
                file_status=FileStatus::Cancelled;
                break;
            }

            if let Some(reason)=output_skip_reason(output){
                warnings.push(reason.to_string());
                done_units+=1;
                continue;
            }

            let name=expand_name(&output.name,input,&output.format,output.width,output.height);
            let target_path=match &output.dest {
                Destination::Original=>{
                    let dir=input.path.parent().unwrap_or(std::path::Path::new("."));
                    dir.join(&name)
                }
                Destination::Folder(folder)=>folder.join(&name),
            };
            
            let tmp_path=target_path.with_extension(format!("{}.tmp",target_path.extension().map(|e|e.to_string_lossy()).unwrap_or_default()));
            
            if std::fs::metadata(&target_path).is_ok() {
                match output.overwrite {
                    OverwritePolicy::Skip=>{
                        outputs.push(OutputReceipt{path:target_path,bytes:0,complete:true});
                        done_units+=1;
                        continue;
                    }
                    OverwritePolicy::Fail=>{
                        file_status=FileStatus::Failed;
                        error_message=Some("FILE_EXISTS".to_string());
                        break;
                    }
                    OverwritePolicy::Replace=>{}
                }
            }

            let input_bytes=match std::fs::read(&input.path) {
                Ok(b)=>b,
                Err(e)=>{
                    file_status=FileStatus::Failed;
                    error_message=Some(format!("INPUT_READ_FAILED: {}",e));
                    break;
                }
            };

            let payload=match encode_output(output,manifest,&input_bytes) {
                Ok(buf)=>buf,
                Err(e)=>{
                    file_status=FileStatus::Failed;
                    error_message=Some(format!("ENCODE_FAILED: {}",e.code));
                    break;
                }
            };

            if std::fs::write(&tmp_path,&payload).is_err() {
                let _=std::fs::remove_file(&tmp_path);
                file_status=FileStatus::Failed;
                error_message=Some("OUTPUT_WRITE_FAILED".to_string());
                break;
            }

            let bytes=payload.len() as u64;

            park_if_armed(handle,BatchBoundary::Encode,input.id);
            if is_cancelled(handle) {
                let _=std::fs::remove_file(&tmp_path);
                file_status=FileStatus::Cancelled;
                break;
            }
            
            if target_path.exists() {
                if std::fs::remove_file(&target_path).is_err() {
                    let _=std::fs::remove_file(&tmp_path);
                    file_status=FileStatus::Failed;
                    error_message=Some("REPLACE_FAILED".to_string());
                    break;
                }
            }
            
            if std::fs::rename(&tmp_path,&target_path).is_err() {
                let _=std::fs::remove_file(&tmp_path);
                file_status=FileStatus::Failed;
                error_message=Some("RENAME_FAILED".to_string());
                break;
            }
            
            outputs.push(OutputReceipt{path:target_path,bytes,complete:true});
            done_units+=1;
        }
        
        if file_status==FileStatus::Working {
            let completed_count=outputs.iter().filter(|o|o.complete).count();
            if completed_count==manifest.outputs.len() {
                file_status=FileStatus::Done;
            } else if completed_count>0||!warnings.is_empty() {
                file_status=FileStatus::Partial;
            } else {
                file_status=FileStatus::Failed;
            }
        } else if file_status==FileStatus::Failed && outputs.iter().any(|o|o.complete) {
            file_status=FileStatus::Partial;
        }
        
        files.push(FileReceipt{input:input.id,status:file_status,outputs,error:error_message,warnings});
        publish_progress(manifest,handle,&files,done_units,total_units)?;
    }
    
    let status=if is_cancelled(handle) {BatchStatus::Cancelled} else {BatchStatus::Done};
    Ok(BatchSnapshot{handle:handle.clone(),status,done_units,total_units,files})
}

pub fn start(handle: &BatchHandle) -> Result<BatchSnapshot, CommandError> {
    let manifest = {
        let _gate = START_GATE.lock().map_err(|_| CommandError::new("EDITOR_BUSY"))?;
        if let Some(snapshot) = snapshots().lock().map_err(|_| CommandError::new("EDITOR_BUSY"))?.get(handle).cloned() { return Ok(snapshot); }
        if let Some(snapshot) = BatchStore::default().load(handle)? {
            snapshots().lock().map_err(|_| CommandError::new("EDITOR_BUSY"))?.insert(handle.clone(), snapshot.clone());
            return Ok(snapshot);
        }
        let manifest = prepared_inputs(handle).ok_or_else(CommandError::not_found)?;
        let problems = validate_manifest(&manifest);
        if let Some(problem) = problems.first() { return Err(CommandError { code: problem.code.clone(), message_key: problem.message_key.clone() }); }
        CANCEL_FLAGS.get_or_init(||Mutex::new(HashMap::new())).lock().map_err(|_|CommandError::new("EDITOR_BUSY"))?.insert(handle.clone(),false);
        let _=bind_control(handle);
        ACTIVE.get_or_init(||Mutex::new(HashSet::new())).lock().map_err(|_|CommandError::new("EDITOR_BUSY"))?.insert(handle.clone());
        publish_progress(&manifest,handle,&[],0,measure_work(&manifest))?;
        manifest
    };
    let _active = ActiveJob(handle.clone());
    match run_batch(&manifest,handle) {
        Ok(snapshot) => { publish(snapshot.clone())?; Ok(snapshot) }
        Err(error) => {
            let snapshot = snapshots().lock().ok().and_then(|map|map.get(handle).cloned());
            if let Some(mut snapshot) = snapshot { snapshot.status = BatchStatus::Failed; let _ = publish(snapshot); }
            Err(error)
        }
    }
}

pub fn query(handle: &BatchHandle) -> BatchSnapshot {
    snapshots().lock().ok().and_then(|map|map.get(handle).cloned())
        .or_else(||BatchStore::default().load(handle).ok().flatten())
        .unwrap_or_else(||BatchSnapshot { handle:handle.clone(),status:BatchStatus::Prepared,done_units:0,total_units:0,files:Vec::new() })
}

pub fn cancel(handle: &BatchHandle) -> BatchSnapshot {
    if let Some(ctrl)=export_control(handle){
        ctrl.cancel.store(true,Ordering::Relaxed);
    }else{
        bind_control(handle).cancel.store(true,Ordering::Relaxed);
    }
    if !is_running(handle) { return query(handle); }
    CANCEL_FLAGS.get_or_init(||Mutex::new(HashMap::new())).lock().ok().map(|mut flags|flags.insert(handle.clone(),true));
    wake_boundary(handle);
    let Some(mut map) = snapshots().lock().ok() else { return query(handle); };
    let Some(snapshot) = map.get_mut(handle) else { drop(map); return query(handle); };
    if snapshot.status != BatchStatus::Running && snapshot.status != BatchStatus::Cancelled { return snapshot.clone(); }
    snapshot.status = BatchStatus::Cancelled;
    for file in &mut snapshot.files {
        if file.status == FileStatus::Working { file.status = FileStatus::Cancelled; }
        else if file.status == FileStatus::Waiting { file.status = FileStatus::Untouched; }
    }
    let _ = BatchStore::default().save(snapshot);
    let snapshot = snapshot.clone();
    drop(map);
    wake_boundary(handle);
    snapshot
}

fn handle_from_command(command: &Command) -> Result<BatchHandle, CommandError> {
    let value = command.params.get("handle").ok_or_else(CommandError::invalid_params)?;
    let text = value.as_str().ok_or_else(CommandError::invalid_params)?;
    if text.is_empty() {
        return Err(CommandError::invalid_params());
    }
    Ok(BatchHandle(text.to_string()))
}

fn arm_from_command(command: &Command, handle: &BatchHandle) {
    let Some(value) = command.params.get("pause_at") else { return; };
    let Some(name) = value.as_str() else { return; };
    let boundary = match name {
        "file" => BatchBoundary::File,
        "encode" => BatchBoundary::Encode,
        _ => return,
    };
    let input = command.params.get("pause_input").and_then(|value| value.as_u64()).map(BatchInputId);
    arm_boundary(handle, boundary, input);
}

fn snapshot_value(snapshot: &BatchSnapshot, handle: &BatchHandle) -> CommandResult {
    let mut value = serde_json::to_value(snapshot).map_err(|_| CommandError::new("INTERNAL_ERROR"))?;
    if let Some(paused) = current_boundary(handle) {
        if let Some(object) = value.as_object_mut() {
            object.insert("paused_at".into(), serde_json::to_value(paused).unwrap_or(serde_json::Value::Null));
        }
    }
    Ok(value)
}

fn handle_workspace_command(command: &Command) -> CommandResult {
    match command.name.as_str() {
        "export.batch.prepare" => {
            let manifest = command.params.get("manifest").cloned().ok_or_else(CommandError::invalid_params)?;
            let manifest: BatchManifest = serde_json::from_value(manifest).map_err(|_| CommandError::invalid_params())?;
            let handle = prepare(manifest)?;
            Ok(serde_json::json!({ "handle": handle.0 }))
        }
        "export.batch.validate" => {
            let handle = handle_from_command(command)?;
            Ok(serde_json::json!({ "problems": validate(&handle) }))
        }
        "export.batch.start" => {
            let handle = handle_from_command(command)?;
            arm_from_command(command, &handle);
            let snapshot = start(&handle)?;
            snapshot_value(&snapshot, &handle)
        }
        "export.batch.query" => {
            let handle = handle_from_command(command)?;
            let snapshot = query(&handle);
            snapshot_value(&snapshot, &handle)
        }
        "export.batch.cancel" => {
            let handle = handle_from_command(command)?;
            let snapshot = cancel(&handle);
            snapshot_value(&snapshot, &handle)
        }
        _ => Err(CommandError::not_implemented()),
    }
}

pub fn install_workspace_bridge() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        dtool_core::commands::export_batch::register(handle_workspace_command);
    });
}

#[used]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".init_array"))]
#[cfg_attr(target_os = "macos", unsafe(link_section = "__DATA,__mod_init_func"))]
#[cfg_attr(target_os = "windows", unsafe(link_section = ".CRT$XCU"))]
static INSTALL_WORKSPACE_BATCH_BRIDGE: unsafe extern "C" fn() = {
    unsafe extern "C" fn init() {
        install_workspace_bridge();
    }
    init
};
