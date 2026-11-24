use std::io::Read;
use std::path::Path;

use dtool_core::{CommandError, ContextId, Workspace};

pub mod native;
pub mod recent;
pub mod image_import;
pub mod brush_file;
pub mod import_pdf;
mod import_pdf_map;
pub mod import_svg;
pub mod import_report;
pub mod import_eps;
pub mod import_other;
pub mod import_idml;
pub mod import_cad;
pub mod af;
pub mod addons;
pub mod selection_file;
pub mod import_commands;
pub mod place_current;

pub use native::{IoError, IoErrorKind, NATIVE_EXTENSION, open, save};
pub use recent::RecentList;
pub use import_pdf::{ImportFormat, ImportProfile, PageSel, ImportAs, ImportColour, FontReplacement, ImportOptions, admit, import_pdf};
pub use import_svg::{admit_svg, import_svg};
pub use import_report::{LossKind, ImportLoss, ImportReport, FidelityManifest, fidelity_check};
pub use import_eps::import_eps;
pub use import_other::{is_ait, is_psd, admit_other, open_other, import_other};
pub use import_idml::{IdmlPackage, StyleConflict, read_idml, import_idml};
pub use import_cad::{CadSelection, CadImportOptions, import_cad};
pub use af::{AfInspectReport, FeatureStatus, inspect_af, open_af};
pub use addons::{AddonKind, AddonPack, import_addon, export_addon, merge_addon};
pub use selection_file::{save_selection, load_selection, SELECTION_FILE_EXTENSION};
pub use import_commands::{install as install_import_commands, uninstall as uninstall_import_commands};

pub fn crate_ready() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterchangeFormat {
    Pdf,
    Svg,
    PdfCompatibleAi,
    Eps,
    Ait,
    Psd,
    Idml,
    Indd,
    Pub,
    Dwg,
    Dxf,
}

impl InterchangeFormat {
    pub const ALL: [InterchangeFormat; 11] = [
        InterchangeFormat::Pdf,
        InterchangeFormat::Svg,
        InterchangeFormat::PdfCompatibleAi,
        InterchangeFormat::Eps,
        InterchangeFormat::Ait,
        InterchangeFormat::Psd,
        InterchangeFormat::Idml,
        InterchangeFormat::Indd,
        InterchangeFormat::Pub,
        InterchangeFormat::Dwg,
        InterchangeFormat::Dxf,
    ];

    pub fn extension(self) -> &'static str {
        match self {
            InterchangeFormat::Pdf => "pdf",
            InterchangeFormat::Svg => "svg",
            InterchangeFormat::PdfCompatibleAi => "ai",
            InterchangeFormat::Eps => "eps",
            InterchangeFormat::Ait => "ait",
            InterchangeFormat::Psd => "psd",
            InterchangeFormat::Idml => "idml",
            InterchangeFormat::Indd => "indd",
            InterchangeFormat::Pub => "pub",
            InterchangeFormat::Dwg => "dwg",
            InterchangeFormat::Dxf => "dxf",
        }
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "pdf" => Some(InterchangeFormat::Pdf),
            "svg" | "svgz" => Some(InterchangeFormat::Svg),
            "ai" => Some(InterchangeFormat::PdfCompatibleAi),
            "eps" => Some(InterchangeFormat::Eps),
            "ait" => Some(InterchangeFormat::Ait),
            "psd" => Some(InterchangeFormat::Psd),
            "idml" => Some(InterchangeFormat::Idml),
            "indd" => Some(InterchangeFormat::Indd),
            "pub" => Some(InterchangeFormat::Pub),
            "dwg" => Some(InterchangeFormat::Dwg),
            "dxf" => Some(InterchangeFormat::Dxf),
            _ => None,
        }
    }
}

pub fn is_pdf_compatible(path:&Path)->bool{import_ai::admit_ai(path).is_ok_and(|profile|profile.supported)}

fn interchange_support(path: &Path, format: InterchangeFormat) -> CommandError {
    match format {
        InterchangeFormat::PdfCompatibleAi if !is_pdf_compatible(path) => CommandError {code:"UNSUPPORTED_PROFILE".into(),message_key:"error.ai.no_pdf".into()},
        InterchangeFormat::Ait => CommandError::not_implemented(),
        InterchangeFormat::Psd => CommandError {code:"UNSUPPORTED_PROFILE".into(),message_key:"error.format_not_supported".into()},
        _ => CommandError::not_implemented(),
    }
}

pub fn open_interchange(workspace: &mut Workspace, path: &Path) -> Result<ContextId, CommandError> {
    let ext = path.extension().and_then(|e| e.to_str()).map(|s| s.to_ascii_lowercase());
    match ext.as_deref() {
        Some("af") | Some("afdesign") | Some("afphoto") | Some("afpub") => {
            let (doc, _report) = open_af(path).map_err(|e| CommandError::new(e.code()))?;
            let title = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Untitled");
            let id = workspace.new_document(title);
            if let Some(ctx) = workspace.get_mut(id) {
                ctx.session = dtool_core::Session::new(doc);
                ctx.path = Some(path.to_path_buf());
            }
            Ok(id)
        }
        _ => match InterchangeFormat::from_path(path) {
            Some(format) => match interchange_import_command(path, format) {
                Some(name) => run_interchange_import(workspace, path, name),
                None => Err(interchange_support(path, format)),
            },
            None => workspace.open_path(path),
        },
    }
}

fn interchange_import_command(path: &Path, format: InterchangeFormat) -> Option<&'static str> {
    use dtool_core::import_hook::{IMPORT_AI, IMPORT_EPS, IMPORT_PDF, IMPORT_SVG};
    match format {
        InterchangeFormat::Pdf => Some(IMPORT_PDF),
        InterchangeFormat::Svg => Some(IMPORT_SVG),
        InterchangeFormat::PdfCompatibleAi if is_pdf_compatible(path) => Some(IMPORT_AI),
        InterchangeFormat::Eps => Some(IMPORT_EPS),
        _ => None,
    }
}

fn run_interchange_import(workspace: &mut Workspace, path: &Path, name: &str) -> Result<ContextId, CommandError> {
    let command = dtool_core::Command::with_params(name, serde_json::json!({ "path": path }));
    match import_commands::handle(workspace, &command) {
        Some((Some(id), Ok(_))) => Ok(id),
        Some((_, Err(error))) => Err(error),
        _ => Err(CommandError::not_implemented()),
    }
}

pub fn import_interchange(workspace: &mut Workspace, path: &Path) -> Result<(), CommandError> {
    import_into_current(workspace, path).map(|_| ())
}

pub fn import_into_current(workspace: &mut Workspace, path: &Path) -> dtool_core::CommandResult {
    let target = workspace.active().ok_or_else(CommandError::no_document)?;
    let format = InterchangeFormat::from_path(path).ok_or_else(CommandError::invalid_params)?;
    let name = interchange_import_command(path, format).ok_or_else(|| interchange_support(path, format))?;
    let mut scratch = Workspace::new(workspace.services());
    let imported = run_interchange_import(&mut scratch, path, name)?;
    let document = scratch.get(imported).map(|context| context.session.document.clone()).ok_or_else(CommandError::not_found)?;
    let context = workspace.get_mut(target).ok_or_else(CommandError::no_document)?;
    place_current::place_document(&mut context.session, &document)
}

pub mod import_ai;
pub use import_ai::admit_ai;
