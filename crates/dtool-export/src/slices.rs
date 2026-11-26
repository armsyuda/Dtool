use std::collections::HashSet;
use std::path::{Path, PathBuf};

use dtool_core::document::Document;
use dtool_core::slice::{OutputSize, SliceTarget, expand_path_with_context};

use crate::raster::{self, ExportError, ExportJob, ExportOptions, JobReceipt, OverwritePolicy};
use crate::{ExportArea, ExportFormat};

pub fn plan(
    doc: &Document,
    target: &SliceTarget,
    directory: &Path,
    document_name: &str,
    document_file_name: &str,
) -> Result<ExportJob, ExportError> {
    let mut job = ExportJob::new();
    let mut paths = HashSet::new();

    for slice in doc.slices.iter().filter(|slice| slice.enabled) {
        for output in slice.outputs.iter().filter(|output| output.enabled) {
            let included = match target {
                SliceTarget::AllEnabled => true,
                SliceTarget::SelectedSlices => slice.export_selected,
                SliceTarget::SelectedOutput(slice_id, output_id) => slice.id == *slice_id && output.id == *output_id,
            };
            if !included { continue; }

            let invalid = || ExportError::InvalidArea(slice.name.clone());
            let format = raster::parse_format(&output.format)?;
            if !matches!(format, ExportFormat::Png | ExportFormat::Jpeg | ExportFormat::Tiff
                | ExportFormat::Webp | ExportFormat::Gif | ExportFormat::Svg | ExportFormat::Pdf)
            {
                return Err(ExportError::Unavailable(output.format.clone()));
            }
            let [left, top, right, bottom] = slice.resolved_bounds(doc).map_err(|_| invalid())?;
            let width = right - left;
            let height = bottom - top;
            let [pixel_width, pixel_height] = output.dimensions(slice, doc).map_err(|_| invalid())?;
            let base = ExportOptions { format, ..ExportOptions::default() };
            let mut options = match &output.options {
                Some(settings) => base.merge_json(settings)?,
                None => base,
            };
            options.format = format;
            options.area = ExportArea::Rect([left, top, width, height]);
            options.pages = None;
            options.width = None;
            options.height = None;
            options.dpi = None;
            options.scale = None;
            match output.size {
                OutputSize::Scale(scale) => options.scale = Some(scale),
                OutputSize::Width(width) => options.width = Some(width),
                OutputSize::Height(height) => options.height = Some(height),
                OutputSize::Square(side) => {
                    options.width = Some(side);
                    options.height = Some(side);
                }
            }
            if (u64::from(pixel_width) * u64::from(pixel_height)) > raster::MAX_EXPORT_PIXELS
                || pixel_width.max(pixel_height) > raster::MAX_EXPORT_SIDE
            {
                return Err(ExportError::Unavailable("size".into()));
            }
            if format == ExportFormat::Svg {
                options.width = Some(pixel_width);
                options.height = Some(pixel_height);
                options.scale = None;
            }
            options.validate()?;

            let explicit = output.path.as_deref().filter(|path| !path.trim().is_empty())
                .or_else(|| (!slice.path.trim().is_empty()).then_some(slice.path.as_str()));
            let template = explicit.unwrap_or(if output.suffix.is_empty() { "{slice_name}{scale_suffix}" } else { "{slice_name}" });
            let mut expanded = expand_path_with_context(
                template, slice, output, doc, Some(document_name), Some(document_file_name),
            ).map_err(|_| invalid())?;
            if !output.suffix.is_empty() {
                if output.suffix.contains(['/', '\\']) { return Err(invalid()); }
                let extension = expanded.rfind('.').ok_or_else(invalid)?;
                expanded.insert_str(extension, &output.suffix);
            }
            let relative = relative_output_path(&expanded)?;
            if !paths.insert(expanded.to_lowercase()) {
                return Err(ExportError::Collision(relative));
            }
            job.queue(&slice.name, directory.join(relative), options);
        }
    }
    for path in &paths {
        if path.match_indices('/').any(|(end, _)| paths.contains(&path[..end])) {
            return Err(ExportError::Collision(directory.join(path)));
        }
    }
    if job.list().is_empty() {
        return Err(ExportError::NotFound("slice.output".into()));
    }
    Ok(job)
}

fn relative_output_path(name: &str) -> Result<PathBuf, ExportError> {
    let mut path = PathBuf::new();
    for segment in name.split('/') {
        let basename = segment.split('.').next().unwrap_or_default().trim_end().to_ascii_lowercase();
        let device = matches!(basename.as_str(), "con" | "prn" | "aux" | "nul" | "conin$" | "conout$")
            || ["com", "lpt"].iter().any(|prefix| {
                basename.strip_prefix(prefix).is_some_and(|port| matches!(port, "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"))
            });
        if segment.is_empty() || matches!(segment, "." | "..") || segment.ends_with([' ', '.']) || device
            || segment.chars().any(|c| c.is_control() || "\\:<>\"|?*{}".contains(c))
        {
            return Err(ExportError::InvalidArea(name.into()));
        }
        path.push(segment);
    }
    Ok(path)
}

pub fn run(
    doc: &Document,
    target: &SliceTarget,
    directory: &Path,
    document_name: &str,
    document_file_name: &str,
) -> Result<JobReceipt, ExportError> {
    let root = directory.canonicalize().map_err(|error| ExportError::Io(error.to_string()))?;
    if !root.is_dir() {
        return Err(ExportError::InvalidArea(directory.display().to_string()));
    }
    let mut job = plan(doc, target, &root, document_name, document_file_name)?;
    for item in job.list() {
        let parent = item.path.parent().ok_or_else(|| ExportError::InvalidArea(item.path.display().to_string()))?;
        let mut existing = parent.ancestors();
        let nearest = existing.find(|directory| directory.symlink_metadata().is_ok())
            .ok_or_else(|| ExportError::InvalidArea(parent.display().to_string()))?;
        let nearest = nearest.canonicalize().map_err(|error| ExportError::Io(error.to_string()))?;
        if !nearest.starts_with(&root) || !nearest.is_dir() {
            return Err(ExportError::InvalidArea(parent.display().to_string()));
        }
    }
    for item in job.list() {
        if let Some(parent) = item.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| ExportError::Io(error.to_string()))?;
        }
    }
    Ok(job.run_document(doc, OverwritePolicy::Skip))
}
