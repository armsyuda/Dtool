use std::fs::File;
use std::io::Read;
use std::path::Path;

use dtool_core::document::{Document, Units};
use dtool_core::paint::Paint;
use lopdf::{Document as PdfDocument, LoadOptions};

use crate::import_pdf_map::{PdfMapper, page_geometry};
use crate::import_report::ImportReport;
use crate::{IoError, IoErrorKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    Pdf,
    Ai,
    Svg,
}

#[derive(Debug, Clone)]
pub struct ImportProfile {
    pub format: ImportFormat,
    pub version: String,
    pub encrypted: bool,
    pub pages: u32,
    pub supported: bool,
    pub reason_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageSel {
    All,
    Range(u32, u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportAs {
    Pages,
    Artboards,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportColour {
    Rgb,
    Cmyk,
    Grey,
}

#[derive(Debug, Clone)]
pub struct FontReplacement {
    pub missing: String,
    pub family: String,
    pub style: String,
}

#[derive(Debug, Clone)]
pub struct ImportOptions {
    pub pages: PageSel,
    pub import_as: ImportAs,
    pub dpi: u32,
    pub colour: ImportColour,
    pub favour_editable_text: bool,
    pub outline_embedded_text: bool,
    pub group_text_lines: bool,
    pub password: Option<String>,
    pub fonts: Vec<FontReplacement>,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            pages: PageSel::All,
            import_as: ImportAs::Pages,
            dpi: 300,
            colour: ImportColour::Rgb,
            favour_editable_text: false,
            outline_embedded_text: false,
            group_text_lines: false,
            password: None,
            fonts: Vec::new(),
        }
    }
}

pub fn admit(path: &Path, format: ImportFormat) -> Result<ImportProfile, IoError> {
    match format {
        ImportFormat::Pdf => admit_pdf(path),
        _ => Err(IoError::new(IoErrorKind::UnsupportedProfile)),
    }
}

fn admit_pdf(path: &Path) -> Result<ImportProfile, IoError> {
    let mut file = File::open(path).map_err(|err| IoError::from_io(&err))?;

    let mut header = [0u8; 5];
    if file.read_exact(&mut header).is_err() || &header != b"%PDF-" {
        return Err(IoError::new(IoErrorKind::UnsupportedProfile));
    }

    let version = read_pdf_version(path)?;

    let doc = match PdfDocument::load_with_options(
        path,
        LoadOptions::with_max_decompressed_size(64 * 1024 * 1024),
    ) {
        Ok(d) => d,
        Err(_) => return Err(IoError::new(IoErrorKind::UnsupportedProfile)),
    };

    if doc.is_encrypted() {
        return Ok(ImportProfile {
            format: ImportFormat::Pdf,
            version,
            encrypted: true,
            pages: 0,
            supported: false,
            reason_key: Some("PASSWORD_REQUIRED".to_string()),
        });
    }

    let page_count = doc.get_pages().len() as u32;

    if page_count == 0 {
        return Err(IoError::new(IoErrorKind::UnsupportedProfile));
    }

    Ok(ImportProfile {
        format: ImportFormat::Pdf,
        version,
        encrypted: false,
        pages: page_count,
        supported: true,
        reason_key: None,
    })
}

fn read_pdf_version(path: &Path) -> Result<String, IoError> {
    let mut file = File::open(path).map_err(|_| IoError::new(IoErrorKind::InvalidFormat))?;
    let mut buf = [0u8; 16];
    if file.read_exact(&mut buf).is_err() {
        return Err(IoError::new(IoErrorKind::UnsupportedProfile));
    }

    let header_str = String::from_utf8_lossy(&buf);
    if !header_str.starts_with("%PDF-") {
        return Err(IoError::new(IoErrorKind::UnsupportedProfile));
    }

    let version_part = &header_str[5..];
    let end_idx = version_part
        .find(|c: char| c == '\n' || c == '\r')
        .unwrap_or(version_part.len());
    Ok(version_part[..end_idx].to_string())
}

pub fn import_pdf(path: &Path, opts: &ImportOptions) -> Result<(Document, ImportReport), IoError> {
    let profile = admit(path, ImportFormat::Pdf)?;

    if !profile.supported && profile.reason_key.as_deref() != Some("PASSWORD_REQUIRED") {
        return Err(IoError::new(IoErrorKind::UnsupportedProfile));
    }

    if profile.encrypted && opts.password.is_none() {
        return Err(IoError {
            kind: IoErrorKind::Permission,
            message_key: "error.password_required".into(),
        });
    }
    let doc = PdfDocument::load_with_options(
        path,
        LoadOptions {
            password: opts.password.clone(),
            max_decompressed_size: Some(64 * 1024 * 1024),
            ..LoadOptions::default()
        },
    )
    .map_err(|error| match error {
        lopdf::Error::InvalidPassword => IoError {
            kind: IoErrorKind::Permission,
            message_key: "error.wrong_password".into(),
        },
        lopdf::Error::IO(error) => IoError::from_io(&error),
        _ => IoError::new(IoErrorKind::UnsupportedProfile),
    })?;
    map_document(doc, opts)
}

pub fn import_pdf_bytes(bytes: &[u8], opts: &ImportOptions) -> Result<(Document, ImportReport), IoError> {
    if !bytes.starts_with(b"%PDF-") {
        return Err(IoError::new(IoErrorKind::InvalidFormat));
    }
    let doc = PdfDocument::load_mem_with_options(bytes, LoadOptions::with_max_decompressed_size(64 * 1024 * 1024))
        .map_err(|_| IoError::new(IoErrorKind::UnsupportedProfile))?;
    map_document(doc, opts)
}

fn map_document(doc: PdfDocument, opts: &ImportOptions) -> Result<(Document, ImportReport), IoError> {
    if doc.is_encrypted() {
        return Err(IoError::new(IoErrorKind::UnsupportedProfile));
    }

    let page_ids: Vec<lopdf::ObjectId> = doc.get_pages().into_values().collect();
    let page_count = page_ids.len() as u32;
    if page_count == 0 {
        return Err(IoError::new(IoErrorKind::UnsupportedProfile));
    }

    let selected_pages: Vec<u32> = match opts.pages {
        PageSel::All => (1..=page_count).collect(),
        PageSel::Range(start, end) => {
            if start == 0 || end > page_count || start > end {
                return Err(IoError::new(IoErrorKind::InvalidFormat));
            }
            (start..=end).collect()
        }
    };

    let dpi = if opts.dpi == 0 {
        300.0
    } else {
        opts.dpi as f32
    };
    let geometry = selected_pages
        .iter()
        .map(|&page| page_geometry(&doc, page_ids[(page - 1) as usize]))
        .collect::<Result<Vec<_>, _>>()?;
    let mut core_doc = Document::new(
        geometry[0].size(),
        Units::Pt,
        dpi,
        selected_pages.len() as u32,
    );
    core_doc.setup.facing_pages = false;
    for (page, geometry) in core_doc.pages.iter_mut().zip(&geometry) {
        page.size = geometry.size();
    }
    use dtool_core::colour::{ColourFormat, ColourPrefs, DocColour};
    let format = match opts.colour {
        ImportColour::Rgb => ColourFormat::RGBA_8,
        ImportColour::Cmyk => ColourFormat::CMYK_8,
        ImportColour::Grey => ColourFormat::GREY_8,
    };
    core_doc.colour = DocColour::for_format(format, &ColourPrefs::default());
    let mut report = ImportReport::default();
    let mut mapper = PdfMapper::new(&doc, &mut core_doc, &mut report, opts);
    let mut x = 0.0;
    for (index, (&page_num, geometry)) in selected_pages.iter().zip(geometry).enumerate() {
        let width = geometry.size()[0];
        mapper.map_page(page_ids[(page_num - 1) as usize], index, [x, 0.0], geometry)?;
        x += width;
        if !x.is_finite() {
            return Err(IoError::new(IoErrorKind::InvalidFormat));
        }
    }
    mapper.finish();
    if report.objects == 0 && report.texts == 0 && report.images == 0 && !report.is_acceptable() {
        return Err(IoError::new(IoErrorKind::UnsupportedProfile));
    }
    Ok((core_doc, report))
}

#[derive(Debug, Clone)]
pub struct FidelityManifest {
    pub objects: Vec<(String, [f32; 4])>,
    pub texts: Vec<String>,
    pub colours: Vec<String>,
}

pub fn fidelity_check(doc: &Document, expected: &FidelityManifest, tol: f32) -> Vec<String> {
    let mut mismatches = Vec::new();

    for (name, rect) in &expected.objects {
        let found = doc.objects.iter().find(|o| o.name == *name);
        if let Some(obj) = found {
            let got = [obj.pos[0], obj.pos[1], obj.size[0], obj.size[1]];
            let dx = got[0] - rect[0];
            let dy = got[1] - rect[1];
            let dw = got[2] - rect[2];
            let dh = got[3] - rect[3];
            if dx.abs() > tol || dy.abs() > tol || dw.abs() > tol || dh.abs() > tol {
                mismatches.push(format!(
                    "Object '{}' geometry mismatch: expected [{}, {}, {}, {}], got [{}, {}, {}, {}]",
                    name, rect[0], rect[1], rect[2], rect[3], got[0], got[1], got[2], got[3]
                ));
            }
        } else {
            mismatches.push(format!("Object '{}' not found", name));
        }
    }

    for text in &expected.texts {
        let found = doc.objects.iter().any(|o| {
            o.name == *text
                || o.text_content().map_or(false, |content| content == *text)
                || o.story_id()
                    .and_then(|id| {
                        doc.stories
                            .iter()
                            .find(|story| story.id == id)
                            .map(|story| story.text.as_str())
                            .or_else(|| {
                                doc.stories
                                    .get(id.0 as usize)
                                    .map(|story| story.text.as_str())
                            })
                    })
                    .map_or(false, |content| content == *text)
        });
        if !found {
            mismatches.push(format!("Text '{}' not found", text));
        }
    }

    for colour in &expected.colours {
        let found = doc
            .objects
            .iter()
            .any(|o| paint_matches(&o.fill, colour) || paint_matches(&o.stroke.paint, colour));
        if !found {
            mismatches.push(format!("Colour '{}' not found", colour));
        }
    }

    mismatches
}

fn paint_matches(paint: &Paint, colour: &str) -> bool {
    paint_keys(paint).iter().any(|key| {
        key.eq_ignore_ascii_case(colour) || strip_hash(key).eq_ignore_ascii_case(strip_hash(colour))
    })
}

fn strip_hash(value: &str) -> &str {
    value.strip_prefix('#').unwrap_or(value)
}

fn paint_keys(paint: &Paint) -> Vec<String> {
    match paint {
        Paint::Solid { colour } => {
            let mut keys = vec![
                format!(
                    "#{:02X}{:02X}{:02X}{:02X}",
                    colour.r, colour.g, colour.b, colour.a
                ),
                format!("#{:02X}{:02X}{:02X}", colour.r, colour.g, colour.b),
                format!(
                    "#{:02x}{:02x}{:02x}{:02x}",
                    colour.r, colour.g, colour.b, colour.a
                ),
                format!("#{:02x}{:02x}{:02x}", colour.r, colour.g, colour.b),
            ];
            if colour.model == dtool_core::colour::ColourModel::Cmyk {
                let v = colour.values.map(|v| v / 100.0);
                keys.push(format!("CMYK({}, {}, {}, {})", v[0], v[1], v[2], v[3]));
                keys.push(format!("CMYK({},{},{},{})", v[0], v[1], v[2], v[3]));
            }
            keys
        }
        Paint::None => Vec::new(),
        Paint::Gradient { .. } => vec!["gradient".to_string()],
        Paint::Hatch(_) => vec!["hatch".to_string()],
        Paint::Bitmap(_) => vec!["bitmap".to_string()],
        Paint::Pattern(_) => vec!["pattern".to_string()],
        Paint::Mesh(_) => vec!["mesh".to_string()],
        Paint::Diffusion(_) => vec!["diffusion".to_string()],
    }
}
