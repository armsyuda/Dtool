use std::path::{Path,PathBuf};

pub use dtool_core::text_link::{decode_text,read_text_file,link_status,text_link_rows,TextLink,TextLinkRow};

pub const TEXT_EXTENSIONS:&[&str]=&["txt","text","md","csv","tsv","rtf","docx"];

pub fn is_text_file(path:&Path)->bool {
    path.extension().and_then(|extension|extension.to_str()).is_some_and(|extension|TEXT_EXTENSIONS.iter().any(|known|known.eq_ignore_ascii_case(extension)))
}

pub fn text_files(paths:&[PathBuf])->Vec<PathBuf> {
    paths.iter().filter(|path|is_text_file(path)).cloned().collect()
}

pub fn join_files(paths:&[PathBuf])->Result<String,dtool_core::CommandError> {
    let parts:Result<Vec<String>,_>=text_files(paths).iter().map(|path|read_text_file(path)).collect();
    Ok(parts?.join("\n"))
}
