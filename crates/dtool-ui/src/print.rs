use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use dtool_core::ObjectId;
use dtool_protocol::{CommandError, CommandResult};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
mod vector;
#[cfg(windows)]
mod sheet;
#[cfg(windows)]
mod profiles;
#[cfg(windows)]
pub(crate) mod dialog;

#[derive(Clone, Debug)]
pub enum PrintRange {
    All,
    Current,
    Pages(Vec<u32>),
    Artboards(Vec<ObjectId>),
    Selection,
}

#[derive(Clone, Debug)]
pub enum FitType {
    ActualSize,
    ScaleToFit,
    Scale,
}

#[derive(Clone, Debug)]
pub struct PdfMarks {
    pub crop_marks: bool,
    pub registration_marks: bool,
    pub color_bars: bool,
    pub page_info: bool,
}

impl Default for PdfMarks {
    fn default() -> Self {
        Self {
            crop_marks: false,
            registration_marks: false,
            color_bars: false,
            page_info: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PrintSettings {
    pub profile: String,
    pub printer: Option<String>,
    pub copies: u32,
    pub collate: bool,
    pub range: PrintRange,
    pub fit: FitType,
    pub scale: f32,
    pub include: String,
    pub reverse: bool,
    pub landscape: bool,
    pub bleed: bool,
    pub marks: PdfMarks,
    pub rasterize_dpi: u32,
    pub clip_transparency: bool,
    pub rasterize_page: bool,
    pub to_file: Option<PathBuf>,
}

impl Default for PrintSettings {
    fn default() -> Self {
        Self {
            profile: "Custom".to_string(),
            printer: None,
            copies: 1,
            collate: true,
            range: PrintRange::All,
            fit: FitType::Scale,
            scale: 100.0,
            include: "All Sheets".to_string(),
            reverse: false,
            landscape: false,
            bleed: false,
            marks: PdfMarks::default(),
            rasterize_dpi: 300,
            clip_transparency: true,
            rasterize_page: false,
            to_file: None,
        }
    }
}

pub trait PrintBackend {
    fn printers(&self) -> Vec<String>;
    fn send(&self, pdf: &PathBuf, s: &PrintSettings) -> Result<(), CommandError>;
}

pub struct FilePrintBackend;

impl PrintBackend for FilePrintBackend {
    fn printers(&self) -> Vec<String> {
        vec!["File".to_string()]
    }

    fn send(&self, pdf: &PathBuf, s: &PrintSettings) -> Result<(), CommandError> {
        if let Some(path) = &s.to_file {
            std::fs::copy(pdf, path).map_err(|_| CommandError::new("IO_ERROR"))?;
            Ok(())
        } else {
            Err(CommandError::invalid_params())
        }
    }
}

fn temp_print_path() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("dtool_print_{}_{}.pdf", std::process::id(), stamp))
}

pub fn run_print(settings: &PrintSettings) -> Result<CommandResult, CommandError> {
    let backend = FilePrintBackend;
    
    if settings.to_file.is_none() {
        return Err(CommandError::invalid_params());
    }

    let temp_pdf = temp_print_path();
    
    backend.send(&temp_pdf, settings)?;
    
    if let Some(path) = &settings.to_file {
        if let Err(e) = std::fs::remove_file(&temp_pdf) {
            eprintln!("Failed to remove temp file: {}", e);
        }
        
        return Ok(Ok(serde_json::json!({
            "path": path.display().to_string(),
            "status": "success"
        })));
    }

    Err(CommandError::invalid_params())
}
