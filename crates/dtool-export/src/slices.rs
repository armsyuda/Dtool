use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use dtool_core::document::Document;
use dtool_core::slice::{INCLUDE_CONTAINER, OutputSize, SliceSource, SliceTarget, expand_path_with_context};
use dtool_core::{ObjectId, artboard};

use crate::raster::{self, ExportError, ExportJob, ExportJobItem, ExportOptions, JobReceipt, OverwriteDecision, OverwritePolicy};
use crate::{ExportArea, ExportFormat};

pub fn plan(
    doc: &Document,
    target: &SliceTarget,
    directory: &Path,
    document_name: &str,
    document_file_name: &str,
) -> Result<ExportJob, ExportError> {
    Ok(plan_with_sources(doc, target, directory, document_name, document_file_name)?.0)
}

fn plan_with_sources(
    doc: &Document,
    target: &SliceTarget,
    directory: &Path,
    document_name: &str,
    document_file_name: &str,
) -> Result<(ExportJob, HashMap<u64, ObjectId>), ExportError> {
    let mut sources = HashMap::new();
    let mut job = ExportJob::new();
    let mut paths = HashSet::new();

    for slice in doc.slices.iter().filter(|slice| slice.enabled) {
        let source = match slice.source {
            SliceSource::Layer(id) => Some(id),
            _ => None,
        };
        let mut cropped = None;
        for output in slice.outputs.iter().filter(|output| output.enabled) {
            let included = match target {
                SliceTarget::AllEnabled => true,
                SliceTarget::LayerQueue => source.is_some(),
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
            let mut settings = output.options.clone();
            let include = settings.as_mut().and_then(serde_json::Value::as_object_mut)
                .and_then(|settings| settings.remove(INCLUDE_CONTAINER))
                .and_then(|value| value.as_bool()).unwrap_or(false);
            let bounds = match source {
                Some(id) if include => layer_bounds(doc, id, true).ok_or_else(invalid)?,
                Some(id) => *cropped.get_or_insert_with(|| layer_bounds(doc, id, false))
                    .as_ref().ok_or_else(invalid)?,
                None => slice.resolved_bounds(doc).map_err(|_| invalid())?,
            };
            let [left, top, right, bottom] = bounds;
            let [pixel_width, pixel_height] = output.dimensions_for_bounds(bounds, doc).map_err(|_| invalid())?;
            let base = ExportOptions { format, ..ExportOptions::default() };
            let mut options = match settings.as_ref() {
                Some(settings) => base.merge_json(settings)?,
                None => base,
            };
            options.format = format;
            options.area = ExportArea::Rect([left, top, right - left, bottom - top]);
            options.pages = None;
            options.width = None;
            options.height = None;
            options.dpi = None;
            options.scale = None;
            if source.is_some() {
                options.width = Some(pixel_width);
                options.height = Some(pixel_height);
            } else {
                match output.size {
                    OutputSize::Scale(scale) => options.scale = Some(scale),
                    OutputSize::Width(width) => options.width = Some(width),
                    OutputSize::Height(height) => options.height = Some(height),
                    OutputSize::Square(side) => {
                        options.width = Some(side);
                        options.height = Some(side);
                    }
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
            let relative = if source.is_some() && explicit.is_none() {
                let name: String = slice.name.chars().map(|c| {
                    if c.is_control() || "/\\:<>\"|?*{}".contains(c) { '_' } else { c }
                }).take(100).collect();
                let name = name.trim_matches([' ', '.']);
                let suffix = match output.size {
                    OutputSize::Scale(scale) if scale != 1.0 => format!("@{scale}x"),
                    OutputSize::Scale(_) => String::new(),
                    _ => format!("@{pixel_width}x{pixel_height}"),
                };
                let extension = match output.format.as_str() { "jpeg" => "jpg", value => value };
                let base = format!("{name}{suffix}{}", output.suffix);
                let base = if name.is_empty() || relative_output_path(&format!("{base}.{extension}")).is_err() {
                    format!("Layer-{}{suffix}", slice.id.0)
                } else {
                    base
                };
                let mut candidate = format!("{base}.{extension}");
                let mut variant = 2;
                while !paths.insert(candidate.to_lowercase()) {
                    candidate = format!("{base}-{variant}.{extension}");
                    variant += 1;
                }
                relative_output_path(&candidate)?
            } else {
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
                relative
            };
            let item = job.queue(&slice.name, directory.join(relative), options);
            if let Some(id) = source {
                sources.insert(item, id);
            }
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
    Ok((job, sources))
}

fn layer_bounds(doc: &Document, id: ObjectId, include: bool) -> Option<[f32; 4]> {
    let object = doc.get_object(id)?;
    let [x, y, w, h] = if artboard::is_artboard(object) {
        artboard::artboard_bounds(doc, id).ok()?
    } else if include {
        if let Some(board) = artboard::owner(doc, id) {
            artboard::artboard_bounds(doc, board.id).ok()?
        } else {
            let (x, y, w, h) = crate::page_region(doc, dtool_core::data_merge::layout::object_page_index(doc, id))?;
            [x, y, w, h]
        }
    } else {
        let isolated = isolated_layer(doc, id);
        if let Some(region) = crate::selection_output_region(&isolated, &[id]) {
            alpha_bounds(&isolated, region)
        } else {
            let [x, y] = object.pos;
            let side = pixels_per_unit(doc).recip();
            [x, y, side, side]
        }
    };
    Some([x, y, x + w, y + h])
}

fn isolated_layer(doc: &Document, id: ObjectId) -> Document {
    let mut isolated = crate::only_objects(doc, &[id]);
    if !doc.get_object(id).is_some_and(artboard::is_artboard) {
        isolated.setup.transparent_background = true;
    }
    isolated
}

fn pixels_per_unit(doc: &Document) -> f32 {
    dtool_core::doc_setup::unit_factor(doc.units, doc.dpi, dtool_core::document::Units::Px, doc.dpi)
}

fn alpha_bounds(doc: &Document, (x, y, w, h): (f32, f32, f32, f32)) -> [f32; 4] {
    let factor = pixels_per_unit(doc);
    let native_width = (w * factor).ceil().max(1.0);
    let native_height = (h * factor).ceil().max(1.0);
    // Scan at native resolution within the exporter limits. Rendering the
    // measurement in tiles keeps memory independent of the layer dimensions.
    let measurement_scale = ((raster::MAX_EXPORT_PIXELS as f32) / (native_width * native_height)).sqrt()
        .min(raster::MAX_EXPORT_SIDE as f32 / native_width.max(native_height)).min(1.0);
    let width = (native_width * measurement_scale).floor().max(1.0) as usize;
    let height = (native_height * measurement_scale).floor().max(1.0) as usize;
    let (dx, dy) = (w / width as f32, h / height as f32);
    let mut visible: Option<[usize; 4]> = None;
    let tile_side = 2048;
    for row in (0..height).step_by(tile_side) {
        let tile_height = tile_side.min(height - row);
        for column in (0..width).step_by(tile_side) {
            let tile_width = tile_side.min(width - column);
            let region = (x + column as f32 * dx, y + row as f32 * dy,
                tile_width as f32 * dx, tile_height as f32 * dy);
            let area = ExportArea::Rect([region.0, region.1, region.2, region.3]);
            let pixels = raster::render_rgba_for_area(doc, &area, region, [tile_width, tile_height]);
            for (index, rgba) in pixels.chunks_exact(4).enumerate() {
                if rgba[3] != 0 {
                    let px = column + index % tile_width;
                    let py = row + index / tile_width;
                    let bounds = visible.get_or_insert([px, py, px + 1, py + 1]);
                    bounds[0] = bounds[0].min(px);
                    bounds[1] = bounds[1].min(py);
                    bounds[2] = bounds[2].max(px + 1);
                    bounds[3] = bounds[3].max(py + 1);
                }
            }
        }
    }
    let Some([left, top, right, bottom]) = visible else {
        return [x, y, factor.recip(), factor.recip()];
    };
    [x + left as f32 * dx, y + top as f32 * dy, (right - left) as f32 * dx, (bottom - top) as f32 * dy]
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
    run_job(doc, target, directory, document_name, document_file_name, OverwritePolicy::Skip, |_| OverwriteDecision::Skip)
}

pub fn run_confirming(
    doc: &Document,
    target: &SliceTarget,
    directory: &Path,
    document_name: &str,
    document_file_name: &str,
    decide: impl FnMut(&ExportJobItem) -> OverwriteDecision,
) -> Result<JobReceipt, ExportError> {
    run_job(doc, target, directory, document_name, document_file_name, OverwritePolicy::Ask, decide)
}

fn run_job(
    doc: &Document,
    target: &SliceTarget,
    directory: &Path,
    document_name: &str,
    document_file_name: &str,
    policy: OverwritePolicy,
    decide: impl FnMut(&ExportJobItem) -> OverwriteDecision,
) -> Result<JobReceipt, ExportError> {
    let root = directory.canonicalize().map_err(|error| ExportError::Io(error.to_string()))?;
    if !root.is_dir() {
        return Err(ExportError::InvalidArea(directory.display().to_string()));
    }
    let (mut job, sources) = plan_with_sources(doc, target, &root, document_name, document_file_name)?;
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
    Ok(job.run_with(policy, |item| {
        if let Some(&id) = sources.get(&item.id) {
            raster::encode_output(&isolated_layer(doc, id), &item.options, &item.path)
        } else {
            raster::encode_output(doc, &item.options, &item.path)
        }
    }, decide, || false))
}
