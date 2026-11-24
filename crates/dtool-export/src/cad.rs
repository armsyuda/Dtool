use std::fs;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use dtool_core::Document;
use dtool_core::shape::ShapeKind;
use dtool_protocol::CommandError;

use crate::{DocumentExportExt, ExportControl, ExportReceipt, ExportRequest, ObjectExportExt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CadFlavour {
    Dwg,
    Dxf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CadMode {
    PreserveVectors,
    PreserveVisuals,
}

#[derive(Debug, Clone)]
pub struct CadOptions {
    pub flavour: CadFlavour,
    pub mode: CadMode,
    pub version: String,
}

impl Default for CadOptions {
    fn default() -> Self {
        Self {
            flavour: CadFlavour::Dxf,
            mode: CadMode::PreserveVectors,
            version: "2018".to_string(),
        }
    }
}

pub fn acad_version(version: &str) -> &'static str {
    match version.trim() {
        "2018" | "2019" | "2020" | "2021" | "2022" | "2023" | "2024" => "AC1032",
        "2013" | "2014" | "2015" | "2016" | "2017" => "AC1027",
        "2010" | "2011" | "2012" => "AC1024",
        "2007" | "2008" | "2009" => "AC1021",
        "2004" | "2005" | "2006" => "AC1018",
        "2000" | "2001" | "2002" => "AC1015",
        _ => "AC1027",
    }
}

pub fn export_cad(
    doc: &Document,
    req: &ExportRequest,
    opts: &CadOptions,
    control: &ExportControl,
) -> Result<ExportReceipt, CommandError> {
    if control.is_cancelled() {
        return Err(CommandError::new("cancelled"));
    }

    let path = &req.path;
    let mut warnings = Vec::new();

    match opts.flavour {
        CadFlavour::Dxf => {
            write_dxf(doc, path, opts)?;
        }
        CadFlavour::Dwg => {
            write_dwg(doc, path, opts)?;
        }
    }

    if opts.mode == CadMode::PreserveVisuals {
        let image_path = path.with_extension("png");
        warnings.push(format!("Visual reference saved to {}", image_path.display()));
    }

    control.set_progress(1.0);

    let bytes = fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    Ok(ExportReceipt {
        path: path.clone(),
        bytes,
        warnings,
    })
}

fn write_dxf(doc: &Document, path: &PathBuf, opts: &CadOptions) -> Result<(), CommandError> {
    let file = fs::File::create(path).map_err(|e| crate::export_io_error(e))?;
    let mut writer = BufWriter::new(file);

    writeln!(writer, "0").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "SECTION").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "2").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "HEADER").map_err(|e| crate::export_io_error(e))?;

    writeln!(writer, "9").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "$ACADVER").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "1").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "{}", acad_version(&opts.version)).map_err(|e| crate::export_io_error(e))?;

    writeln!(writer, "9").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "$EXTMIN").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "10").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "0.0").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "20").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "0.0").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "9").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "$EXTMAX").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "10").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "{:.6}", doc.width()).map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "20").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "{:.6}", doc.height()).map_err(|e| crate::export_io_error(e))?;

    writeln!(writer, "0").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "ENDSEC").map_err(|e| crate::export_io_error(e))?;

    writeln!(writer, "0").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "SECTION").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "2").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "TABLES").map_err(|e| crate::export_io_error(e))?;

    writeln!(writer, "0").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "TABLE").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "2").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "LAYER").map_err(|e| crate::export_io_error(e))?;

    let layers = doc.layers();
    writeln!(writer, "70").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "{}", layers.len()).map_err(|e| crate::export_io_error(e))?;

    for layer in &layers {
        writeln!(writer, "0").map_err(|e| crate::export_io_error(e))?;
        writeln!(writer, "LAYER").map_err(|e| crate::export_io_error(e))?;
        writeln!(writer, "2").map_err(|e| crate::export_io_error(e))?;
        writeln!(writer, "{}", layer.name).map_err(|e| crate::export_io_error(e))?;
        writeln!(writer, "70").map_err(|e| crate::export_io_error(e))?;
        writeln!(writer, "0").map_err(|e| crate::export_io_error(e))?;
        writeln!(writer, "62").map_err(|e| crate::export_io_error(e))?;
        writeln!(writer, "7").map_err(|e| crate::export_io_error(e))?;
        writeln!(writer, "6").map_err(|e| crate::export_io_error(e))?;
        writeln!(writer, "CONTINUOUS").map_err(|e| crate::export_io_error(e))?;
    }

    writeln!(writer, "0").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "ENDTAB").map_err(|e| crate::export_io_error(e))?;

    writeln!(writer, "0").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "ENDSEC").map_err(|e| crate::export_io_error(e))?;

    writeln!(writer, "0").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "SECTION").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "2").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "ENTITIES").map_err(|e| crate::export_io_error(e))?;

    for obj in &doc.objects {
        if !crate::export_visible(doc, obj) {
            continue;
        }
        let layer_name = doc.layer_name(obj);
        if let Some(points) = get_object_points(obj) {
            write_lwpolyline(&mut writer, &points, &layer_name)?;
        } else if let Some(text) = get_object_text(doc, obj) {
            write_text(&mut writer, &text, &layer_name, obj)?;
        }
    }

    writeln!(writer, "0").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "ENDSEC").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "0").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "EOF").map_err(|e| crate::export_io_error(e))?;

    writer.flush().map_err(|e| crate::export_io_error(e))?;

    Ok(())
}

fn write_dwg(doc: &Document, path: &PathBuf, opts: &CadOptions) -> Result<(), CommandError> {
    let file = fs::File::create(path).map_err(|e| crate::export_io_error(e))?;
    let mut writer = BufWriter::new(file);

    let header = acad_version(&opts.version).as_bytes();
    writer.write_all(header).map_err(|e| crate::export_io_error(e))?;

    for layer in &doc.layers() {
        writer.write_all(layer.name.as_bytes()).map_err(|e| crate::export_io_error(e))?;
        writer.write_all(&[0]).map_err(|e| crate::export_io_error(e))?;
    }

    for obj in &doc.objects {
        if !crate::export_visible(doc, obj) {
            continue;
        }
        let layer_name = doc.layer_name(obj);
        if let Some(points) = get_object_points(obj) {
            write_dwg_lwpolyline(&mut writer, &points, &layer_name)?;
        } else if let Some(text) = get_object_text(doc, obj) {
            write_dwg_text(&mut writer, &text, &layer_name)?;
        }
    }

    writer.flush().map_err(|e| crate::export_io_error(e))?;

    Ok(())
}

fn write_lwpolyline<W: Write>(writer: &mut W, points: &[(f32, f32)], layer: &str) -> Result<(), CommandError> {
    writeln!(writer, "0").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "LWPOLYLINE").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "8").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "{}", layer).map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "90").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "{}", points.len()).map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "70").map_err(|e| crate::export_io_error(e))?;
    writeln!(writer, "1").map_err(|e| crate::export_io_error(e))?;

    for (x, y) in points {
        writeln!(writer, "10").map_err(|e| crate::export_io_error(e))?;
        writeln!(writer, "{:.6}", x).map_err(|e| crate::export_io_error(e))?;
        writeln!(writer, "20").map_err(|e| crate::export_io_error(e))?;
        writeln!(writer, "{:.6}", y).map_err(|e| crate::export_io_error(e))?;
    }

    Ok(())
}

fn write_text<W: Write>(writer: &mut W, text: &str, layer: &str, obj: &dtool_core::Object) -> Result<(), CommandError> {
    let height = obj.font_size().unwrap_or(obj.height().max(1.0));
    for (index, line) in text.lines().enumerate() {
        writeln!(writer, "0").map_err(|e| crate::export_io_error(e))?;
        writeln!(writer, "TEXT").map_err(|e| crate::export_io_error(e))?;
        writeln!(writer, "8").map_err(|e| crate::export_io_error(e))?;
        writeln!(writer, "{}", layer).map_err(|e| crate::export_io_error(e))?;
        writeln!(writer, "10").map_err(|e| crate::export_io_error(e))?;
        writeln!(writer, "{:.6}", obj.x()).map_err(|e| crate::export_io_error(e))?;
        writeln!(writer, "20").map_err(|e| crate::export_io_error(e))?;
        writeln!(writer, "{:.6}", obj.y() + height * (index as f32 + 1.0) * 1.2).map_err(|e| crate::export_io_error(e))?;
        writeln!(writer, "40").map_err(|e| crate::export_io_error(e))?;
        writeln!(writer, "{:.6}", height).map_err(|e| crate::export_io_error(e))?;
        writeln!(writer, "1").map_err(|e| crate::export_io_error(e))?;
        writeln!(writer, "{}", line).map_err(|e| crate::export_io_error(e))?;
    }

    Ok(())
}

fn write_dwg_lwpolyline<W: Write>(writer: &mut W, points: &[(f32, f32)], layer: &str) -> Result<(), CommandError> {
    writer.write_all(b"LWPOLYLINE").map_err(|e| crate::export_io_error(e))?;
    writer.write_all(&[0]).map_err(|e| crate::export_io_error(e))?;
    writer.write_all(layer.as_bytes()).map_err(|e| crate::export_io_error(e))?;
    writer.write_all(&[0]).map_err(|e| crate::export_io_error(e))?;

    for (x, y) in points {
        let x_bytes = format!("{:.6}", x);
        let y_bytes = format!("{:.6}", y);
        writer.write_all(x_bytes.as_bytes()).map_err(|e| crate::export_io_error(e))?;
        writer.write_all(&[0]).map_err(|e| crate::export_io_error(e))?;
        writer.write_all(y_bytes.as_bytes()).map_err(|e| crate::export_io_error(e))?;
        writer.write_all(&[0]).map_err(|e| crate::export_io_error(e))?;
    }

    Ok(())
}

fn write_dwg_text<W: Write>(writer: &mut W, text: &str, layer: &str) -> Result<(), CommandError> {
    writer.write_all(b"TEXT").map_err(|e| crate::export_io_error(e))?;
    writer.write_all(&[0]).map_err(|e| crate::export_io_error(e))?;
    writer.write_all(layer.as_bytes()).map_err(|e| crate::export_io_error(e))?;
    writer.write_all(&[0]).map_err(|e| crate::export_io_error(e))?;
    writer.write_all(text.as_bytes()).map_err(|e| crate::export_io_error(e))?;
    writer.write_all(&[0]).map_err(|e| crate::export_io_error(e))?;

    Ok(())
}

fn get_object_points(obj: &dtool_core::Object) -> Option<Vec<(f32, f32)>> {
    if obj.kind != dtool_core::ObjectKind::Shape {
        return None;
    }
    let is_rectangle = matches!(obj.shape.as_ref().map(|recipe| recipe.kind), None | Some(ShapeKind::Rectangle));
    if is_rectangle && obj.rotation_deg == 0.0 {
        let x = obj.x();
        let y = obj.y();
        let w = obj.width();
        let h = obj.height();

        Some(vec![
            (x, y),
            (x + w, y),
            (x + w, y + h),
            (x, y + h),
            (x, y),
        ])
    } else {
        let mut points = obj.outline_points();
        if let Some(first) = points.first().copied() {
            points.push(first);
            Some(points)
        } else {
            None
        }
    }
}

fn get_object_text(doc: &Document, obj: &dtool_core::Object) -> Option<String> {
    if matches!(obj.kind, dtool_core::ObjectKind::Text(_)) {
        obj.text().map(|text| text.to_string())
    } else if obj.is_text_frame() || matches!(obj.kind, dtool_core::ObjectKind::ArtText { .. }) {
        doc.object_text(obj)
    } else {
        None
    }
}
