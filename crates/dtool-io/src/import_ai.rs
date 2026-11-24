use std::{io::Read,path::Path};
use crate::{IoError,IoErrorKind};
use crate::import_pdf::{import_pdf,ImportAs,ImportFormat,ImportOptions,ImportProfile};
use crate::import_report::{ImportReport,LossKind};

fn unsupported()->IoError{IoError{kind:IoErrorKind::UnsupportedProfile,message_key:"error.ai.no_pdf".into()}}

pub fn admit_ai(path:&Path)->Result<ImportProfile,IoError>{
    let mut file=std::fs::File::open(path).map_err(|error|IoError::from_io(&error))?;
    let mut header=[0u8;8];
    let count=file.read(&mut header).map_err(|error|IoError::from_io(&error))?;
    if count<8 || &header[..5]!=b"%PDF-"{return Err(unsupported());}
    let pdf=lopdf::Document::load(path).map_err(|_|unsupported())?;
    let root=pdf.trailer.get(b"Root").and_then(|value|value.as_reference()).ok().and_then(|id|pdf.get_object(id).ok()).and_then(|object|object.as_dict().ok()).ok_or_else(unsupported)?;
    if root.get(b"Type").and_then(|value|value.as_name()).ok()!=Some(b"Catalog".as_slice()){return Err(unsupported());}
    let encrypted=pdf.is_encrypted();
    let pages=if encrypted{0}else{u32::try_from(pdf.get_pages().len()).map_err(|_|unsupported())?};
    Ok(ImportProfile{format:ImportFormat::Ai,version:pdf.version,encrypted,pages,supported:!encrypted,reason_key:encrypted.then(||"error.password_required".into())})
}

pub fn import_ai(path:&Path,opts:&ImportOptions)->Result<(dtool_core::Document,ImportReport),IoError>{
    let _profile=admit_ai(path)?;
    let mut mapped=opts.clone();
    mapped.import_as=ImportAs::Artboards;
    let (doc,mut report)=import_pdf(path,&mapped)?;

    for font in &opts.fonts {
        if !font.missing.is_empty() {
            report.add_loss(LossKind::Font,1,false,format!("Missing font: {}",font.missing));
        }
    }

    Ok((doc,report))
}
