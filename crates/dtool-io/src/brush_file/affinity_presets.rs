use std::collections::HashMap;
use std::io::{Cursor, Read};

use dtool_core::brush::{BrushImage, BrushTexture, BrushTextureMode, BrushTip, BrushWetEdges, Controller, ControllerKind, MAX_SUB_BRUSHES, ScatterAxis, SubBrush, SubBrushBlend, SubBrushColorBlend, SubBrushPlacement, TipKind, WetEdgeCurve, WetEdgeScope};
use dtool_core::commands::pixel_presets::ExternalBrushes;
use flate2::read::ZlibDecoder;

use super::{
    Input, MAX_LIBRARY_PIXELS, MAX_SAMPLE_PIXELS, MAX_TIPS, after, find, float_field, label, tip,
};

const MAX_CATALOG: usize = 4 * 1024 * 1024;
const MAX_TILES: usize = 4096;
const MAX_DECODED_PIXELS: usize = 48 * 1024 * 1024;
const TILE_SIDE: usize = 256;
const TILE_PIXELS: usize = TILE_SIDE * TILE_SIDE;
const OPACITY: &[u8] = b"\x09capO";
const SPLINE: &[u8] = b"\x31nlpS";

pub(super) fn decode(file: &[u8]) -> Option<ExternalBrushes> {
    if file.get(..4)? != b"\0\xffKA" || file.get(8..16)? != b"rArB#Inf" {
        return None;
    }
    let directory = read_u64(file.get(16..24)?)?;
    if file.get(directory..)?.starts_with(b"#FT4") {
        let archive = Archive::open(file, directory)?;
        let catalog = archive.read("Brushes.dat", MAX_CATALOG)?;
        return read_catalog(&catalog, Some(&archive));
    }
    if file.get(72..76)? != b"#Fil" {
        return Some(no_brushes());
    }
    let catalog = zstd_entry(file.get(76..)?, MAX_CATALOG)?;
    read_catalog(&catalog, None)
}

fn no_brushes() -> ExternalBrushes {
    ExternalBrushes {
        tips: Vec::new(),
        skipped: 0,
        limited: 0,
    }
}

#[derive(Clone, Copy)]
struct Entry {
    offset: usize,
    size: usize,
    packed: usize,
    checksum: u32,
    compression: u8,
    packed_checksum: u32,
}

struct Archive<'a> {
    data: &'a [u8],
    entries: HashMap<&'a str, Entry>,
}

impl<'a> Archive<'a> {
    fn open(file: &'a [u8], directory: usize) -> Option<Self> {
        let fat = file.get(directory..)?;
        if read_u64(fat.get(4..12)?)? != 0 {
            return None;
        }
        let count = le_u32(fat.get(44..48)?)? as usize;
        if count > MAX_TILES + 1 {
            return None;
        }
        let mut input = Input(fat.get(59..)?);
        let mut entries = HashMap::with_capacity(count);
        for _ in 0..count {
            input.take(4)?;
            if input.take(1)? != [0] {
                return None;
            }
            let offset = read_u64(input.take(8)?)?;
            let size = read_u64(input.take(8)?)?;
            let packed = read_u64(input.take(8)?)?;
            let checksum = le_u32(input.take(4)?)?;
            let [compression] = input.array()?;
            input.take(4)?;
            let packed_checksum = le_u32(input.take(4)?)?;
            let length = usize::from(u16::from_le_bytes(input.array()?));
            if length == 0 || length > 128 {
                return None;
            }
            let name = std::str::from_utf8(input.take(length)?).ok()?;
            let entry = Entry {
                offset,
                size,
                packed,
                checksum,
                compression,
                packed_checksum,
            };
            if entries.insert(name, entry).is_some() {
                return None;
            }
        }
        Some(Self {
            data: file.get(..directory)?,
            entries,
        })
    }

    fn read(&self, name: &str, limit: usize) -> Option<Vec<u8>> {
        let entry = self.entries.get(name)?;
        if entry.size > limit {
            return None;
        }
        let mut block = Input(self.data.get(entry.offset..)?);
        if block.take(4)? != b"#Fil" {
            return None;
        }
        let packed = block.take(entry.packed)?;
        if crc32fast::hash(packed) != entry.packed_checksum {
            return None;
        }
        let mut bytes = match entry.compression {
            0 => {
                if packed.len() != entry.size {
                    return None;
                }
                packed.to_vec()
            }
            1 | 0x41 => bounded_read(ZlibDecoder::new(packed), entry.size)?,
            2 | 0x42 => zstd_entry(packed, entry.size)?,
            _ => return None,
        };
        if bytes.len() != entry.size {
            return None;
        }
        if entry.compression & 0x40 != 0 {
            let mut value = 0u8;
            for byte in &mut bytes {
                value = value.wrapping_add(*byte);
                *byte = value;
            }
        }
        (crc32fast::hash(&bytes) == entry.checksum).then_some(bytes)
    }
}

fn bounded_read(reader: impl Read, limit: usize) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit as u64 + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() <= limit).then_some(bytes)
}

fn zstd_entry(packed: &[u8], limit: usize) -> Option<Vec<u8>> {
    let mut decoder = zstd::stream::read::Decoder::new(Cursor::new(packed))
        .ok()?
        .single_frame();
    decoder.window_log_max(23).ok()?;
    bounded_read(decoder, limit)
}

fn read_u64(bytes: &[u8]) -> Option<usize> {
    usize::try_from(u64::from_le_bytes(bytes.try_into().ok()?)).ok()
}

fn le_u32(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

fn le_after(bytes: &[u8], marker: &[u8]) -> Option<usize> {
    Input(after(bytes, marker)?).le32()
}

fn read_catalog(catalog: &[u8], archive: Option<&Archive<'_>>) -> Option<ExternalBrushes> {
    if !catalog.starts_with(b"\0\xffKS\x02\0VurB") {
        return None;
    }
    let mut info = Input(after(catalog, b"\x2bNCPB")?);
    let category = info.utf8()?;
    let category = label(&format!("Affinity {category}"), "Affinity");
    let mut catalog = Input(after(info.0, b"\xb1VurB")?);
    let count = catalog.le32()?;
    if count > MAX_TIPS {
        return None;
    }
    let table_at = catalog.0.windows(5).rposition(|w| w == b"\xabNurB")?;
    let mut table = Input(&catalog.0[table_at + 5..]);
    let name_bytes = table.le32()?;
    if table.le32()? != count {
        return None;
    }
    let mut name_data = Input(table.take(name_bytes)?);
    if table.0 != [0] && !table.0.is_empty() {
        return None;
    }
    let names: Vec<_> = (0..count).map(|_| name_data.utf8()).collect::<Option<_>>()?;
    if !name_data.0.is_empty() {
        return None;
    }
    let records = &catalog.0[..table_at];
    let curves = pressure_curves(records);

    if find(records, b"\xb1TpiT").is_none() {
        return round_brushes(records, &names, &category, &curves);
    }
    let Some(archive) = archive else {
        return Some(no_brushes());
    };
    textured_brushes(records, &names, &category, archive, &curves)
}

fn field_offsets<'a>(bytes: &'a [u8], marker: &'a [u8]) -> impl Iterator<Item = usize> + 'a {
    bytes
        .windows(marker.len())
        .enumerate()
        .filter_map(move |(index, value)| (value == marker).then_some(index))
}

fn settings(
    name: &str, category: &str, record: &[u8], kind: TipKind, curves: &HashMap<usize, WetEdgeCurve>,
) -> Option<BrushTip> {
    let base = |field: &[u8]| float_field(after(record, field)?, b"\x09VnyD");
    let mut brush = tip(
        label(name, "Brush"),
        category,
        kind,
        base(b"\x32eziS")?,
        float_field(record, b"\x09capS")?,
    );
    brush.opacity = float_field(record, OPACITY)?.clamp(0.0, 1.0);
    brush.accumulation = base(b"\x32nccA").map(|value| value.clamp(0.0, 1.0));
    brush.flow = base(b"\x32wolF")?.clamp(0.0, 1.0);
    brush.hardness = base(b"\x32draH")?.clamp(0.0, 1.0);
    brush.wet_edges = wet_edges(record);
    brush.dynamics.size = pressure_controller(record, b"\x32eziS", curves).unwrap_or_default();
    brush.dynamics.flow = pressure_controller(record, b"\x32wolF", curves).unwrap_or_default();
    if let Some((turns, amount, controller)) = dynamic_fields(record, b"\x32lgnA") {
        // Angle values in the Affinity dynamics record are fractions of a turn.
        brush.rotation = (turns.rem_euclid(1.0) * 360.0 + 180.0).rem_euclid(360.0) - 180.0;
        if controller == 9 {
            brush.dynamics.direction_rotation = amount.clamp(0.0, 1.0);
        }
    }
    for (axis, field) in brush.dynamics.scatter.iter_mut().zip([b"\x32XacS", b"\x32YacS"]) {
        if let Some((_, amount, controller @ (1 | 2))) = dynamic_fields(record, field)
            && amount > 0.0
        {
            *axis = ScatterAxis {
                amount: amount.min(ScatterAxis::MAX_AMOUNT),
                pressure: controller == 2,
            };
        }
    }
    Some(brush)
}

fn dynamic_fields(record: &[u8], marker: &[u8]) -> Option<(f32, f32, usize)> {
    let (base, amount, controller, _) = dynamic_record(record, marker)?;
    Some((base, amount, controller))
}

fn dynamic_record<'a>(record: &'a [u8], marker: &[u8]) -> Option<(f32, f32, usize, &'a [u8])> {
    let mut value = Input(after(record, marker)?.strip_prefix(b"\x01yDrB\0\0\x09VnyD")?);
    let base = f32::from_le_bytes(value.array()?);
    let mut variance = Input(value.0.strip_prefix(b"\x09aVyD")?);
    let amount = f32::from_le_bytes(variance.array()?);
    let mut controller_field = Input(variance.0.strip_prefix(b"\x2aCnyD")?);
    let controller = controller_field.le32()?;
    (base.is_finite() && amount.is_finite()).then_some((base, amount, controller, controller_field.0))
}

fn pressure_curves(records: &[u8]) -> HashMap<usize, WetEdgeCurve> {
    field_offsets(records, b"\x31nlpS\x01").filter_map(|offset| {
        let mut definition = Input(&records[offset + SPLINE.len() + 1..]);
        let id = definition.le32()?;
        let data = definition.0.strip_prefix(b"\x01nlpS\x07")?;
        Some((id, spline_curve(data)?))
    }).collect()
}

fn pressure_controller(
    record: &[u8], marker: &[u8], curves: &HashMap<usize, WetEdgeCurve>,
) -> Option<Controller> {
    let (_, amount, 2, tail) = dynamic_record(record, marker)? else { return None; };
    if !(0.0 < amount && amount <= 1.0) {
        return None;
    }
    let tail = tail.strip_prefix(b"\x2aMVyD")?.get(4..)?.strip_prefix(SPLINE)?;
    let [1 | 2, reference @ ..] = tail else { return None; };
    let id = Input(reference).le32()?;
    let curve = curves.get(&id)?;
    // Represent the imported response using the editable piecewise-linear controller.
    let inputs = if curve.second_derivatives.is_empty() && curve.points.len() <= 14 {
        let mut inputs: Vec<_> = curve.points.iter().map(|point| point[0]).collect();
        if inputs[0] > 0.0 { inputs.insert(0, 0.0); }
        if inputs.last() != Some(&1.0) { inputs.push(1.0); }
        inputs
    } else {
        (0..16).map(|index| index as f32 / 15.0).collect()
    };
    let curve = inputs.into_iter().map(|x| [x, 1.0 - amount * (1.0 - curve.sample(x))]).collect();
    Some(Controller { kind: ControllerKind::Pressure, curve, jitter: 0.0, requires_pressure: true })
}

fn wet_edges(record: &[u8]) -> BrushWetEdges {
    let enabled = match (after(record, b"\x29EWAS"), after(record, b"\x29EteW")) {
        (Some([1, ..]), Some([0, ..])) => Some(false),
        (Some([1, ..]), Some([1, ..])) => Some(true),
        _ => None,
    };
    let curve = after(record, b"\x31SEeW").and_then(wet_edge_curve);
    BrushWetEdges { enabled, curve, ..Default::default() }
}

fn wet_edge_curve(bytes: &[u8]) -> Option<WetEdgeCurve> {
    let [1, data @ ..] = bytes else { return None; };
    spline_curve(&data[find(data, b"\x20tnC")?..])
}

fn spline_curve(data: &[u8]) -> Option<WetEdgeCurve> {
    let mut field = Input(data.strip_prefix(b"\x20tnC")?);
    let count = field.le32()?;
    if !(2..=64).contains(&count) {
        return None;
    }
    let mut samples = Input(field.0.strip_prefix(b"\x8aslaV")?);
    if samples.le32()? != 3 * count {
        return None;
    }
    let values = (0..3 * count).map(|_| {
        Some(f64::from_le_bytes(samples.array()?) as f32)
    }).collect::<Option<Vec<_>>>()?;
    let points = (0..count).map(|i| [values[i], values[count + i]]).collect();
    let linear = match samples.0.strip_prefix(b"\x29rniL")?.first()? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let curve = WetEdgeCurve {
        points,
        second_derivatives: if linear { Vec::new() } else { values[2 * count..].to_vec() },
    };
    curve.valid().then_some(curve)
}

fn round_brushes(
    records: &[u8], names: &[&str], category: &str, curves: &HashMap<usize, WetEdgeCurve>,
) -> Option<ExternalBrushes> {
    let offsets: Vec<_> = field_offsets(records, OPACITY)
        .chain([records.len()])
        .collect();
    if offsets.len() != names.len() + 1 {
        return Some(ExternalBrushes {
            skipped: names.len(),
            ..no_brushes()
        });
    }
    let mut result = no_brushes();
    for (name, pair) in names.iter().zip(offsets.windows(2)) {
        if round_header_start(&records[..pair[0]]).is_some() {
            result.tips.push(settings(
                name,
                category,
                &records[pair[0]..pair[1]],
                TipKind::Round,
                curves,
            )?);
        } else {
            result.skipped += 1;
        }
    }
    Some(result)
}

fn round_header_start(before_opacity: &[u8]) -> Option<usize> {
    [b"rBRR".as_slice(), b"rBRR\x01\0\0\0urBR\0\0\0\x02"].into_iter()
        .find_map(|header| before_opacity.strip_suffix(header).map(<[u8]>::len))
}

fn textured_brushes(
    records: &[u8],
    names: &[&str],
    category: &str,
    archive: &Archive<'_>,
    curves: &HashMap<usize, WetEdgeCurve>,
) -> Option<ExternalBrushes> {
    let bitmaps = field_offsets(records, b"BRBI")
        .filter(|&index| index > 0 && records[index - 1] <= 1)
        .map(|index| (index, false));
    let round = field_offsets(records, OPACITY)
        .filter_map(|index| round_header_start(&records[..index]).map(|start| (start, true)));
    let mut offsets: Vec<_> = bitmaps.chain(round)
        .take(MAX_TIPS * 2 + 1)
        .collect();
    if offsets.len() > MAX_TIPS * 2 {
        return None;
    }
    offsets.sort_unstable_by_key(|&(start, _)| start);
    let ends = offsets.iter().skip(1).map(|&(start, _)| start).chain([records.len()]);
    let mut result = no_brushes();
    let mut labels = names.iter().copied();
    let mut nested = 0usize;
    let mut remaining_direct = 0;
    let mut parent = None;
    let mut images = HashMap::new();
    let mut decoded_pixels = 0;
    let mut stored_pixels = 0;
    for (&(start, round_header), end) in offsets.iter().zip(ends) {
        let record = &records[start..end];
        let top_level = nested == 0;
        let child_count = le_after(record, b"\xb1BbuS")?;
        let direct_child = !top_level && nested == remaining_direct;
        if top_level {
            parent = None;
            remaining_direct = child_count;
        } else if direct_child {
            remaining_direct -= 1;
        }
        nested = nested.saturating_sub(1).checked_add(child_count)?;
        if nested > MAX_TIPS {
            return None;
        }
        let offset = find(record, OPACITY)?;
        let header = &record[..offset];
        let nozzle_field = after(header, b"\xb1TpiT");
        let round = round_header || (!top_level && Input(nozzle_field?).le32()? == 0);
        let data = if round {
            Vec::new()
        } else {
            nozzles(nozzle_field?, archive, &mut images, &mut decoded_pixels)?
        };
        let name = if top_level { labels.next()? } else { "Sub brush" };
        let controls = &record[offset..];
        let sub_options = if direct_child && child_count == 0 && parent.is_some() {
            sub_brush_options(controls)
        } else {
            None
        };
        if !top_level && sub_options.is_none() {
            continue;
        }
        if !round && data.is_empty() {
            if top_level { result.skipped += 1; }
            continue;
        }
        let texture = paper_texture(controls, archive, &mut decoded_pixels)?;
        stored_pixels += data
            .iter()
            .chain(texture.iter().map(|texture| &texture.image))
            .map(|image| (image.size[0] * image.size[1]) as usize)
            .sum::<usize>();
        if stored_pixels > MAX_LIBRARY_PIXELS {
            return None;
        }
        let kind = if round { TipKind::Round } else { TipKind::Intensity };
        let mut brush = settings(name, category, controls, kind, curves)?;
        let mut stamps = data.into_iter();
        brush.image = stamps.next();
        brush.additional_images = stamps.collect();
        brush.texture = texture;
        if top_level {
            if child_count > 0 {
                brush.wet_edges.scope = WetEdgeScope::WholeStroke;
            }
            parent = Some(result.tips.len());
            result.tips.push(brush);
            result.limited += 1;
        } else if let (Some(index), Some(options)) = (parent, sub_options) {
            let children = &mut result.tips[index].sub_brushes;
            if children.len() < MAX_SUB_BRUSHES {
                children.push(SubBrush {
                    tip: brush,
                    enabled: true,
                    sync_size: options.sync_size,
                    sync_spacing: options.sync_spacing,
                    placement: options.placement,
                    blending: options.blending,
                    color_blend: options.color_blend,
                });
            }
        }
    }
    if nested != 0 || labels.next().is_some() {
        return None;
    }
    Some(result)
}

struct SubBrushOptions {
    placement: SubBrushPlacement,
    blending: SubBrushBlend,
    color_blend: SubBrushColorBlend,
    sync_size: bool,
    sync_spacing: bool,
}

fn sub_brush_options(record: &[u8]) -> Option<SubBrushOptions> {
    let options = after(record, b"\xb1BbuS\0\0\0\0")?;
    let mut fields = Input(options.strip_prefix(b"\x2adoMS")?);
    let blending = match fields.le32()? {
        0 => SubBrushBlend::Blend,
        1 => SubBrushBlend::Erase,
        _ => return None,
    };
    let mut fields = Input(fields.0.strip_prefix(b"\x2adMSS")?);
    let placement = match fields.le32()? {
        0 => SubBrushPlacement::AtNozzle,
        1 => SubBrushPlacement::BehindNozzle,
        2 => SubBrushPlacement::StrokeEnd,
        _ => return None,
    };
    let mut flags = Input(fields.0.strip_prefix(b"\x29zSSS")?);
    let [sync_size] = flags.array()?;
    let [sync_spacing] = Input(flags.0.strip_prefix(b"\x29pSSS")?).array()?;
    let color_blend = if after(record, b"\x2aoMlB").is_some_and(|fields| {
        fields.starts_with(b"\x15\0\0\0\x29MBAS\x01")
    }) {
        SubBrushColorBlend::Average
    } else {
        SubBrushColorBlend::Normal
    };
    (sync_size <= 1 && sync_spacing <= 1).then_some(SubBrushOptions {
        placement,
        blending,
        color_blend,
        sync_size: sync_size != 0,
        sync_spacing: sync_spacing != 0,
    })
}

fn nozzles(
    field: &[u8],
    archive: &Archive<'_>,
    images: &mut HashMap<u32, BrushImage>,
    decoded_pixels: &mut usize,
) -> Option<Vec<BrushImage>> {
    let mut input = Input(field);
    let count = input.le32()?;
    if !(1..=8).contains(&count) {
        return Some(Vec::new());
    }
    if count == 1 && input.0.first() == Some(&2) {
        input.take(1)?;
        let id = le_u32(input.take(4)?)?;
        return Some(images.get(&id).cloned().into_iter().collect());
    }

    let mut result = Vec::new();
    let offsets: Vec<_> = field_offsets(input.0, b"mByD")
        .chain([input.0.len()])
        .collect();
    if offsets.len() != count + 1 {
        return Some(Vec::new());
    }
    for pair in offsets.windows(2) {
        let at = pair[0];
        if at < 6 || input.0[at - 6] != 1 || input.0[at - 1] > 1 {
            return None;
        }
        let id = le_u32(&input.0[at - 5..at - 1])?;
        let bitmap = &input.0[at + 4..pair[1]];
        let Some(image) = read_bitmap(bitmap, archive, decoded_pixels)? else {
            return Some(Vec::new());
        };
        images.insert(id, image.clone());
        result.push(image);
    }
    Some(result)
}

fn read_bitmap(
    record: &[u8],
    archive: &Archive<'_>,
    decoded_pixels: &mut usize,
) -> Option<Option<BrushImage>> {
    read_bitmap_level(record, archive, decoded_pixels, 0)
}

fn paper_texture(
    record: &[u8],
    archive: &Archive<'_>,
    decoded_pixels: &mut usize,
) -> Option<Option<BrushTexture>> {
    let Some(field) = after(record, b"\x31TksM") else { return Some(None); };
    let (bitmap, settings) = field.split_at_checked(find(field, b"\x2aMTkM")?)?;
    let mode = Input(settings.get(5..)?).le32()?;
    if !matches!(mode, 0..=2) || bitmap.first() != Some(&1) {
        return Some(None);
    }
    let scale = f64::from_le_bytes(Input(after(settings, b"\x0aSTkM")?).array()?) as f32;
    if !(0.01..=10.0).contains(&scale) {
        return Some(None);
    }
    let bitmap = bitmap.strip_prefix(&[1])?.get(4..)?.strip_prefix(b"\x01mByD")?;
    let source_size = [le_after(bitmap, b"\x07WpmB")?, le_after(bitmap, b"\x07HpmB")?];
    if source_size.iter().any(|side| !(1..=4096).contains(side)) {
        return Some(None);
    }
    // Use the stored mip nearest the brush image limit and keep the
    // original pixel dimensions for repeat spacing in the raster.
    let level = (source_size[0].max(source_size[1]).div_ceil(512)).next_power_of_two().trailing_zeros() as u8;
    let Some(image) = read_bitmap_level(bitmap, archive, decoded_pixels, level)? else {
        return Some(None);
    };
    Some(Some(BrushTexture {
        image,
        source_size: source_size.map(|side| side as u32),
        scale,
        enabled: mode != 0,
        inverted: false,
        mode: if mode == 2 { BrushTextureMode::Final } else { BrushTextureMode::Nozzle },
    }))
}

fn read_bitmap_level(
    record: &[u8],
    archive: &Archive<'_>,
    decoded_pixels: &mut usize,
    level: u8,
) -> Option<Option<BrushImage>> {
    if !after(record, b"\x2atmrF")?.starts_with(b"\x06\0\0\0") {
        return Some(None);
    }
    let width = le_after(record, b"\x07WpmB")?.div_ceil(1 << level);
    let height = le_after(record, b"\x07HpmB")?.div_ceil(1 << level);
    let area = width.checked_mul(height)?;
    if area == 0 || area > MAX_SAMPLE_PIXELS || width.max(height) > 4096 {
        return Some(None);
    }
    *decoded_pixels = decoded_pixels.checked_add(area)?;
    if *decoded_pixels > MAX_DECODED_PIXELS {
        return None;
    }

    let columns = width.div_ceil(TILE_SIDE);
    let count = columns * height.div_ceil(TILE_SIDE);
    // The base mip stores payloads only for populated cells. Its state
    // table preserves the positions of omitted transparent cells.
    let (index_marker, state_marker) = if level == 0 {
        (*b"\xb11xdI", *b"\x811atS")
    } else {
        ([0xb1, b'1', level, b'I', b'M'], [0x81, b'1', level, b'T', b'M'])
    };
    let entries = after(record, &index_marker)?;
    let state_at = find(entries, &state_marker)?;
    let mut states = Input(entries.get(state_at + state_marker.len()..)?);
    if states.le32()? != count {
        return None;
    }
    let states = states.take(count)?;
    let mut tiles = Input(entries.get(..state_at)?);
    let populated = tiles.le32()?;
    if states.iter().any(|state| !matches!(state, 0 | 4))
        || states.iter().filter(|&&state| state == 4).count() != populated
    {
        return None;
    }
    let mut rgba = vec![0; area.checked_mul(4)?];
    for (index, &state) in states.iter().enumerate() {
        if state == 0 {
            continue;
        }
        tiles = Input(after(tiles.0, b"\x33ataDItaD")?);
        let name = tiles.utf8()?;
        if tiles.take(1)? != [0] {
            return None;
        }
        let tile = archive.read(name, TILE_PIXELS + 22)?;
        let data = tile_data(&tile)?;
        let x_start = (index % columns) * TILE_SIDE;
        let y_start = (index / columns) * TILE_SIDE;
        let w = (width - x_start).min(TILE_SIDE);
        let h = (height - y_start).min(TILE_SIDE);
        for row in 0..h {
            let target = ((y_start + row) * width + x_start) * 4;
            for (pixel, &alpha) in rgba[target..target + w * 4]
                .chunks_exact_mut(4)
                .zip(&data[row * TILE_SIDE..row * TILE_SIDE + w])
            {
                pixel.copy_from_slice(&[255, 255, 255, alpha]);
            }
        }
    }
    if !tiles.0.is_empty() {
        return None;
    }
    if !rgba.chunks_exact(4).any(|pixel| pixel[3] != 0) {
        return Some(None);
    }
    Some(Some(BrushImage::from_rgba(
        [width as u32, height as u32],
        rgba,
    )?))
}

fn tile_data(tile: &[u8]) -> Option<&[u8]> {
    if tile.len() == TILE_PIXELS {
        return Some(tile);
    }
    let mut input = Input(tile.strip_prefix(b"\0\xffKS\x01\0ataD\x01\0\x2dItaD")?);
    if input.le32()? != TILE_PIXELS {
        return None;
    }
    let pixels = input.take(TILE_PIXELS)?;
    (input.0 == [0]).then_some(pixels)
}
