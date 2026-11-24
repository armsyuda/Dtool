use crate::commands::{Command, CommandError, CommandResult};
use crate::command::Session;
use crate::document::Document;
use std::path::{Path,PathBuf};
use serde_json::{json,Value};

pub fn can_execute(cmd:&Command)->bool {matches!(cmd.name.as_str(),"doc.package.save"|"doc.package.open"|"doc.package.resave")}

pub fn run(cmd:&Command,session:&mut Session)->Result<CommandResult,CommandError> {
    if !can_execute(cmd) {return Err(CommandError::not_implemented());}
    let folder=cmd.params.get("folder").and_then(Value::as_str).filter(|v|!v.is_empty()).map(PathBuf::from).ok_or_else(CommandError::invalid_params)?;
    if cmd.name=="doc.package.open" {
        let folder=std::fs::canonicalize(&folder).map_err(io_error)?;
        let mut doc:Document=serde_json::from_slice(&std::fs::read(folder.join("document.dtool")).map_err(io_error)?).map_err(|_|CommandError::invalid_params())?;
        crate::native_io::resolve_image_links(&mut doc, &folder);
        let result=Ok(json!({"document":doc,"folder":folder}));
        apply(result.clone(),session)?;
        return Ok(result);
    }
    if !session.has_document {return Err(CommandError::no_document());}
    let boolean=|key:&str,default:bool|->Result<bool,CommandError> {match cmd.params.get(key) {None=>Ok(default),Some(Value::Bool(v))=>Ok(*v),_=>Err(CommandError::invalid_params())}};
    let images=boolean("images",true)?;
    let fonts=boolean("fonts",false)?;
    let zip=boolean("zip",false)?;
    let font_sources=if fonts {Some(font_sources(session)?)} else {None};
    std::fs::create_dir_all(&folder).map_err(io_error)?;
    let folder=std::fs::canonicalize(&folder).map_err(io_error)?;
    let mut manifest=read_manifest(&folder);
    let links=folder.join("Links");if images {std::fs::create_dir_all(&links).map_err(io_error)?;}
    let mut doc=session.document.clone();let mut copied=Vec::new();let mut missing=Vec::new();let mut kept=Vec::new();let mut used=std::collections::HashSet::new();let mut items=Vec::new();
    let mut owned_links:Vec<String>=Vec::new();
    for resource in &mut doc.resources {
        if resource.embedded {items.push(json!({"resource":resource.id.0,"name":resource.display_name(),"status":"embedded"}));continue;}
        if !images {items.push(json!({"resource":resource.id.0,"name":resource.display_name(),"status":"skipped"}));continue;}
        let Some(path)=resource.path.clone() else {missing.push(resource.name.clone());resource.status=crate::resources::ResourceStatus::Missing;items.push(json!({"resource":resource.id.0,"name":resource.display_name(),"status":"missing","path":Value::Null}));continue;};
        let source=if path.is_absolute() {path} else {folder.join(path)};
        let source=std::fs::canonicalize(&source).unwrap_or(source);
        let Some(name)=source.file_name() else {return Err(CommandError::invalid_params());};
        let file_name=format!("{}-{}",resource.id.0,name.to_string_lossy());
        let target=if source.parent()==Some(links.as_path()) {source.clone()} else {links.join(file_name)};
        used.insert(target.clone());
        if !source.is_file() {missing.push(source.to_string_lossy().into_owned());resource.status=crate::resources::ResourceStatus::Missing;items.push(json!({"resource":resource.id.0,"name":resource.display_name(),"status":"missing","path":source}));continue;}
        let target_name=target.file_name().ok_or_else(CommandError::invalid_params)?.to_string_lossy().into_owned();
        let status=if source!=target {std::fs::copy(&source,&target).map_err(io_error)?;copied.push(target.clone());owned_links.push(target_name.clone());"copied"} else {if manifest.links.contains(&target_name) {owned_links.push(target_name.clone());}kept.push(target.clone());"kept"};
        items.push(json!({"resource":resource.id.0,"name":resource.display_name(),"status":status,"path":target}));
        resource.path=Some(PathBuf::from("Links").join(&target_name));
        resource.status=crate::resources::ResourceStatus::Linked;
    }
    let mut removed=Vec::new();
    if images {
        removed.extend(prune(&links,&manifest.links,&used)?);
        owned_links.sort();owned_links.dedup();
        manifest.links=owned_links;
    }
    if links.is_dir() {for entry in std::fs::read_dir(&links).map_err(io_error)? {let entry=entry.map_err(io_error)?;if entry.file_type().map_err(io_error)?.is_file() && !used.contains(&entry.path()) {kept.push(entry.path());}}}
    let font_report=match font_sources {
        Some(sources)=>{
            let (report,owned,font_removed)=package_fonts(&folder,&sources,&manifest.fonts)?;
            removed.extend(font_removed);
            manifest.fonts=owned;
            Value::Array(report)
        }
        None=>Value::String("skipped".into()),
    };
    let fonts_missing:Vec<Value>=font_report.as_array().map(|items|items.iter().filter(|item|item["status"]=="missing").map(|item|item["family"].clone()).collect()).unwrap_or_default();
    let body=serde_json::to_vec_pretty(&doc).map_err(|_|CommandError::invalid_params())?;
    write_document(&folder.join("document.dtool"),&body)?;
    write_manifest(&folder,&manifest)?;
    let archive=if zip {Value::String(write_zip(&folder)?.to_string_lossy().into_owned())} else {Value::Null};
    Ok(Ok(json!({"copied":copied,"missing":missing,"restricted":[],"kept":kept,"removed":removed,"items":items,"fonts":font_report,"fonts_missing":fonts_missing,"zip":archive,"folder":folder})))
}

pub const MANIFEST_FILE:&str=".dtool_package.json";
pub const FONTS_DIR:&str="Fonts";
pub const FONT_FILES:&str="font.files";

#[derive(Debug,Default,Clone,serde::Serialize,serde::Deserialize)]
pub struct PackageManifest {
    #[serde(default)]
    pub links:Vec<String>,
    #[serde(default)]
    pub fonts:Vec<String>,
}

pub fn read_manifest(folder:&Path)->PackageManifest {
    std::fs::read(folder.join(MANIFEST_FILE)).ok().and_then(|bytes|serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn write_manifest(folder:&Path,manifest:&PackageManifest)->Result<(),CommandError> {
    let body=serde_json::to_vec_pretty(manifest).map_err(|_|CommandError::invalid_params())?;
    write_document(&folder.join(MANIFEST_FILE),&body)
}

fn plain_name(name:&str)->bool {
    !name.is_empty() && name!="." && name!=".." && !name.contains(['/','\\',':']) && Path::new(name).file_name().is_some_and(|file|file==name)
}

fn prune(dir:&Path,owned:&[String],used:&std::collections::HashSet<PathBuf>)->Result<Vec<PathBuf>,CommandError> {
    let mut removed=Vec::new();
    for name in owned {
        if !plain_name(name) {continue;}
        let path=dir.join(name);
        if used.contains(&path) {continue;}
        let Ok(meta)=std::fs::symlink_metadata(&path) else {continue;};
        if !meta.file_type().is_file() {continue;}
        std::fs::remove_file(&path).map_err(io_error)?;
        removed.push(path);
    }
    Ok(removed)
}

fn font_faces(doc:&Document)->Vec<(String,String)> {
    let mut faces:Vec<(String,String)>=Vec::new();
    for story in &doc.stories {
        for range in &story.ranges {
            let family=range.get("family").and_then(Value::as_str).unwrap_or("").to_string();
            let style=range.get("style").and_then(Value::as_str).unwrap_or("").to_string();
            if !family.is_empty() && !faces.contains(&(family.clone(),style.clone())) {faces.push((family,style));}
        }
    }
    faces.sort();
    faces
}

#[derive(Debug,Clone)]
struct FontSource {
    family:String,
    style:String,
    paths:Vec<PathBuf>,
}

fn unavailable()->CommandError {CommandError {code:"PACKAGE_FONT_SOURCES_UNAVAILABLE".into(),message_key:"error.package_font_sources".into()}}

fn font_sources(session:&mut Session)->Result<Vec<FontSource>,CommandError> {
    if !crate::commands::text_host::installed() {return Err(unavailable());}
    let faces=font_faces(&session.document);
    if faces.is_empty() {return Ok(Vec::new());}
    let request=Command::with_params(FONT_FILES,json!({"faces":faces.iter().map(|(family,style)|json!({"family":family,"style":style})).collect::<Vec<_>>()}));
    let reply=match crate::commands::text_host::run(session,&request) {Some(Ok(reply))=>reply,_=>return Err(unavailable())};
    let files=reply.get("files").and_then(Value::as_array).ok_or_else(unavailable)?;
    Ok(faces.into_iter().map(|(family,style)|{
        let paths=files.iter().find(|file|file["family"].as_str()==Some(family.as_str()) && file["style"].as_str().unwrap_or("")==style)
            .and_then(|file|file["paths"].as_array())
            .map(|paths|paths.iter().filter_map(Value::as_str).filter(|path|!path.is_empty()).map(PathBuf::from).collect())
            .unwrap_or_default();
        FontSource {family,style,paths}
    }).collect())
}

fn package_fonts(folder:&Path,sources:&[FontSource],previous:&[String])->Result<(Vec<Value>,Vec<String>,Vec<PathBuf>),CommandError> {
    let dir=folder.join(FONTS_DIR);
    let mut names:std::collections::HashMap<PathBuf,String>=std::collections::HashMap::new();
    let mut taken:std::collections::HashSet<String>=std::collections::HashSet::new();
    let mut used=std::collections::HashSet::new();
    let mut owned=Vec::new();
    let mut report=Vec::new();
    for source in sources {
        let present:Vec<&PathBuf>=source.paths.iter().filter(|path|path.is_file()).collect();
        if present.is_empty() {
            report.push(json!({"family":source.family,"style":source.style,"status":"missing","files":[]}));
            continue;
        }
        std::fs::create_dir_all(&dir).map_err(io_error)?;
        let mut files=Vec::new();
        for path in present {
            let name=match names.get(path) {
                Some(name)=>name.clone(),
                None=>{
                    let base=path.file_name().map(|name|name.to_string_lossy().into_owned()).filter(|name|plain_name(name)).unwrap_or_else(||"font".into());
                    let mut name=base.clone();
                    let mut index=1;
                    while taken.contains(&name.to_lowercase()) {name=format!("{index}-{base}");index+=1;}
                    taken.insert(name.to_lowercase());
                    let target=dir.join(&name);
                    std::fs::copy(path,&target).map_err(io_error)?;
                    names.insert(path.clone(),name.clone());
                    owned.push(name.clone());
                    used.insert(target);
                    name
                }
            };
            files.push(PathBuf::from(FONTS_DIR).join(name));
        }
        report.push(json!({"family":source.family,"style":source.style,"status":"copied","files":files}));
    }
    let removed=prune(&dir,previous,&used)?;
    owned.sort();
    Ok((report,owned,removed))
}

pub fn zip_path(folder:&Path)->PathBuf {
    let name=folder.file_name().map(|name|name.to_string_lossy().into_owned()).unwrap_or_else(||"package".into());
    folder.with_file_name(format!("{name}.zip"))
}

fn collect_files(root:&Path,dir:&Path,out:&mut Vec<(String,PathBuf)>)->Result<(),CommandError> {
    let mut entries:Vec<_>=std::fs::read_dir(dir).map_err(io_error)?.collect::<Result<Vec<_>,_>>().map_err(io_error)?;
    entries.sort_by_key(|entry|entry.file_name());
    for entry in entries {
        let path=entry.path();
        let kind=entry.file_type().map_err(io_error)?;
        if kind.is_dir() {collect_files(root,&path,out)?;continue;}
        if !kind.is_file() {continue;}
        let name=entry.file_name().to_string_lossy().into_owned();
        if name==MANIFEST_FILE || name.contains(".tmp-") {continue;}
        let relative=path.strip_prefix(root).map_err(|_|CommandError::invalid_params())?;
        let key=relative.components().map(|part|part.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
        out.push((key,path));
    }
    Ok(())
}

fn write_zip(folder:&Path)->Result<PathBuf,CommandError> {
    use std::io::Write;
    let mut files=Vec::new();
    collect_files(folder,folder,&mut files)?;
    let target=zip_path(folder);
    let temporary=target.with_extension(format!("zip.tmp-{}",std::process::id()));
    let result=(||{
        let file=std::fs::File::create(&temporary).map_err(io_error)?;
        let mut writer=zip::ZipWriter::new(file);
        let options=zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name,path) in &files {
            writer.start_file(name.as_str(),options).map_err(|_|zip_error())?;
            writer.write_all(&std::fs::read(path).map_err(io_error)?).map_err(io_error)?;
        }
        writer.finish().map_err(|_|zip_error())?.sync_all().map_err(io_error)?;
        std::fs::rename(&temporary,&target).map_err(io_error)
    })();
    if result.is_err() {let _=std::fs::remove_file(&temporary);}
    result.map(|_|target)
}

fn zip_error()->CommandError {CommandError {code:"PACKAGE_ZIP".into(),message_key:"error.package_zip".into()}}
pub fn apply(result:CommandResult,session:&mut Session)->Result<(),CommandError> {
    let value=result?;
    if let Some(value)=value.get("document") {
        let document:Document=serde_json::from_value(value.clone()).map_err(|_|CommandError::invalid_params())?;
        let generation=session.generation.checked_add(1).ok_or_else(CommandError::invalid_params)?;
        session.document=document;session.generation=generation;session.has_document=true;
        session.undo=crate::undo::UndoStack::new();session.selection=crate::selection::Selection::default();
    }
    Ok(())
}
fn io_error(_:std::io::Error)->CommandError {CommandError {code:"PACKAGE_IO".into(),message_key:"error.package_io".into()}}
pub fn write_document(path:&Path,body:&[u8])->Result<(),CommandError> {
    use std::io::Write;
    static SEQUENCE:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
    let id=SEQUENCE.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
    let temporary=path.with_extension(format!("tmp-{}-{}",std::process::id(),id));
    let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(io_error)?;
    let result=(||{file.write_all(body).map_err(io_error)?;file.sync_all().map_err(io_error)?;drop(file);std::fs::rename(&temporary,path).map_err(io_error)})();
    if result.is_err() {let _=std::fs::remove_file(&temporary);}result
}
