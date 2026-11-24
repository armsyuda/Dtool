use std::fs;
use std::path::{Path, PathBuf};
use dtool_core::document::Document;
use dtool_io::IoError;

#[derive(Clone, Debug)]
pub struct PackageOptions {
    pub images: bool,
    pub fonts: bool,
    pub linked_text: bool,
    pub css: bool,
}

#[derive(Clone, Debug)]
pub struct PackageReport {
    pub copied: Vec<PathBuf>,
    pub missing: Vec<String>,
    pub restricted: Vec<String>,
    pub kept: Vec<PathBuf>,
}

pub fn font_embedding_allowed(font_file: &Path) -> bool {
    if !font_file.exists() {
        return false;
    }
    let data = match fs::read(font_file) {
        Ok(d) => d,
        Err(_) => return false,
    };
    if data.len() < 4 {
        return true;
    }
    let tag = &data[0..4];
    if tag != b"OTTO" && tag != b"\x00\x01\x00\x00" {
        return true;
    }
    if data.len() < 12 {
        return true;
    }
    let num_tables = u16::from_be_bytes([data[4], data[5]]) as usize;
    let table_dir_start = 12;
    for i in 0..num_tables {
        let offset = table_dir_start + i * 16;
        if offset + 8 > data.len() {
            break;
        }
        let tag_bytes = &data[offset..offset + 4];
        if tag_bytes == b"OS/2" {
            let os2_offset = u32::from_be_bytes([data[offset + 8], data[offset + 9], data[offset + 10], data[offset + 11]]) as usize;
            if os2_offset + 10 > data.len() {
                return true;
            }
            let fs_type = u16::from_be_bytes([data[os2_offset + 8], data[os2_offset + 9]]);
            return (fs_type & 0x0004) == 0;
        }
    }
    true
}

pub fn save_package(doc: &Document, folder: &Path, opts: &PackageOptions) -> Result<PackageReport, IoError> {
    fs::create_dir_all(folder).map_err(|e| IoError::new(e.to_string()))?;
    let links_dir = folder.join("Links");
    let fonts_dir = folder.join("Fonts");

    if opts.images || opts.linked_text || opts.css {
        fs::create_dir_all(&links_dir).map_err(|e| IoError::new(e.to_string()))?;
    }
    if opts.fonts {
        fs::create_dir_all(&fonts_dir).map_err(|e| IoError::new(e.to_string()))?;
    }

    let mut report = PackageReport {
        copied: Vec::new(),
        missing: Vec::new(),
        restricted: Vec::new(),
        kept: Vec::new(),
    };

    let doc_dir = folder.parent().unwrap_or(Path::new("."));
    let rows = dtool_core::resources::resource_rows(doc, Some(doc_dir));

    for row in &rows {
        if !opts.images && !opts.linked_text && !opts.css {
            break;
        }

        let is_image = row.name.ends_with(".png") || row.name.ends_with(".jpg") || row.name.ends_with(".jpeg") || row.name.ends_with(".webp");
        let is_text = row.name.ends_with(".txt") || row.name.ends_with(".md");
        let is_css = row.name.ends_with(".css");

        if !is_image && !is_text && !is_css {
            continue;
        }

        if (is_image && opts.images) || (is_text && opts.linked_text) || (is_css && opts.css) {
            let src_path = Path::new(&row.name);
            if src_path.exists() {
                let dest_name = src_path.file_name().unwrap_or(std::ffi::OsStr::new("unknown")).to_string_lossy().to_string();
                let dest_path = links_dir.join(&dest_name);
                fs::copy(src_path, &dest_path).map_err(|e| IoError::new(e.to_string()))?;
                report.copied.push(dest_path);
            } else {
                report.missing.push(row.name.clone());
            }
        }
    }

    if opts.fonts {
        for font_name in &doc.metadata.keywords {
            let font_path = Path::new(font_name);
            if font_path.exists() {
                if !font_embedding_allowed(font_path) {
                    report.restricted.push(font_name.clone());
                    continue;
                }
                let dest_name = font_path.file_name().unwrap_or(std::ffi::OsStr::new("unknown")).to_string_lossy().to_string();
                let dest_path = fonts_dir.join(&dest_name);
                fs::copy(font_path, &dest_path).map_err(|e| IoError::new(e.to_string()))?;
                report.copied.push(dest_path);
            } else {
                report.missing.push(font_name.clone());
            }
        }
    }

    let doc_file = folder.join("document.dtool");
    let serialized = serde_json::to_string_pretty(doc).map_err(|e| IoError::new(e.to_string()))?;
    fs::write(&doc_file, serialized).map_err(|e| IoError::new(e.to_string()))?;

    Ok(report)
}

pub fn open_package(folder: &Path) -> Result<Document, IoError> {
    let doc_file = folder.join("document.dtool");
    if !doc_file.exists() {
        return Err(IoError::new(format!("Package file not found: {:?}", doc_file)));
    }
    let content = fs::read_to_string(&doc_file).map_err(|e| IoError::new(e.to_string()))?;
    let mut doc: Document = serde_json::from_str(&content).map_err(|e| IoError::new(e.to_string()))?;

    let links_dir = folder.join("Links");
    if links_dir.exists() {
        for entry in fs::read_dir(&links_dir).map_err(|e| IoError::new(e.to_string()))? {
            let entry = entry.map_err(|e| IoError::new(e.to_string()))?;
            let path = entry.path();
            if path.is_file() {
                let file_name = path.file_name().unwrap_or(std::ffi::OsStr::new("")).to_string_lossy();
                let rel_path = format!("Links/{}", file_name);

                for resource in &mut doc.resources {
                    if !resource.embedded && resource.path.is_some() {
                        let current_path = resource.path.as_ref().unwrap();
                        if current_path.file_name() == Some(std::ffi::OsStr::new(&file_name)) {
                            resource.path = Some(PathBuf::from(&rel_path));
                            resource.name = file_name.to_string();
                            resource.status = dtool_core::resources::ResourceStatus::Linked;
                        }
                    }
                }
            }
        }
    }

    let fonts_dir = folder.join("Fonts");
    if fonts_dir.exists() {
        for entry in fs::read_dir(&fonts_dir).map_err(|e| IoError::new(e.to_string()))? {
            let entry = entry.map_err(|e| IoError::new(e.to_string()))?;
            let path = entry.path();
            if path.is_file() {
                let file_name = path.file_name().unwrap_or(std::ffi::OsStr::new("")).to_string_lossy();
                let rel_path = format!("Fonts/{}", file_name);

                if !doc.metadata.keywords.iter().any(|k| k == &rel_path) {
                    doc.metadata.keywords.push(rel_path);
                }
            }
        }
    }

    Ok(doc)
}

pub fn resave_package(doc: &Document, folder: &Path) -> Result<PackageReport, IoError> {
    let links_dir = folder.join("Links");
    fs::create_dir_all(&links_dir).map_err(|e| IoError::new(e.to_string()))?;

    let mut report = PackageReport {
        copied: Vec::new(),
        missing: Vec::new(),
        restricted: Vec::new(),
        kept: Vec::new(),
    };

    let doc_dir = folder.parent().unwrap_or(Path::new("."));
    let rows = dtool_core::resources::resource_rows(doc, Some(doc_dir));

    for row in &rows {
        if row.status == dtool_core::resources::ResourceStatus::Linked || row.status == dtool_core::resources::ResourceStatus::Modified {
            let is_image = row.name.ends_with(".png") || row.name.ends_with(".jpg") || row.name.ends_with(".jpeg") || row.name.ends_with(".webp");
            if !is_image {
                continue;
            }

            let src_path = Path::new(&row.name);
            if src_path.exists() {
                let dest_name = src_path.file_name().unwrap_or(std::ffi::OsStr::new("unknown")).to_string_lossy().to_string();
                let dest_path = links_dir.join(&dest_name);

                if !dest_path.exists() {
                    fs::copy(src_path, &dest_path).map_err(|e| IoError::new(e.to_string()))?;
                    report.copied.push(dest_path.clone());
                } else {
                    report.kept.push(dest_path.clone());
                }
            } else {
                report.missing.push(row.name.clone());
            }
        }
    }

    if links_dir.exists() {
        for entry in fs::read_dir(&links_dir).map_err(|e| IoError::new(e.to_string()))? {
            let entry = entry.map_err(|e| IoError::new(e.to_string()))?;
            let path = entry.path();
            if path.is_file() {
                let file_name = path.file_name().unwrap_or(std::ffi::OsStr::new("")).to_string_lossy().to_string();
                let rel_path = format!("Links/{}", file_name);

                let still_used = doc.resources.iter().any(|r| {
                    if r.embedded {
                        return false;
                    }
                    match &r.path {
                        Some(p) => p.to_string_lossy() == rel_path || p.file_name() == Some(std::ffi::OsStr::new(&file_name)),
                        None => false,
                    }
                });

                if !still_used && !report.copied.iter().any(|c| c == &path) {
                    report.kept.push(path);
                }
            }
        }
    }

    let doc_file = folder.join("document.dtool");
    let serialized = serde_json::to_string_pretty(doc).map_err(|e| IoError::new(e.to_string()))?;
    fs::write(&doc_file, serialized).map_err(|e| IoError::new(e.to_string()))?;

    Ok(report)
}
