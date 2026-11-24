use dtool_core::CommandError;
use dtool_core::brush::{BrushImage, BrushTip, TipKind};
use dtool_core::commands::pixel_presets::{ExternalBrushes, register_external_importer};

mod affinity_presets;
mod photoshop_descriptor;

use photoshop_descriptor::PresetTip;

const MAX_TIPS: usize = 512;
const MAX_SAMPLE_PIXELS: usize = 16 * 1024 * 1024;
const MAX_LIBRARY_PIXELS: usize = 8 * 1024 * 1024;

pub fn install() {
    register_external_importer(decode);
}

fn decode(extension: &str, stem: &str, bytes: &[u8]) -> Result<ExternalBrushes, CommandError> {
    let imported = match extension {
        "abr" => photoshop(bytes, stem).map(|(tips, skipped)| ExternalBrushes {
            tips,
            skipped,
            limited: 0,
        }),
        "afbrushes" => affinity_presets::decode(bytes),
        _ => None,
    }
    .ok_or_else(|| CommandError::new("INVALID_FORMAT"))?;
    if imported.tips.is_empty() {
        return Err(CommandError::new("UNSUPPORTED_PROFILE"));
    }
    Ok(imported)
}

fn label(value: &str, fallback: &str) -> String {
    let clean: String = value
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect();
    if clean.trim().is_empty() {
        fallback.to_owned()
    } else {
        clean.trim().to_owned()
    }
}

fn tip(name: String, category: &str, kind: TipKind, size_px: f32, spacing: f32) -> BrushTip {
    BrushTip {
        id: String::new(),
        name,
        category: category.to_owned(),
        kind,
        size_px: size_px.clamp(1.0, 2048.0),
        spacing: spacing.clamp(0.01, 2.0),
        opacity: 1.0,
        accumulation: None,
        flow: 1.0,
        hardness: 1.0,
        roundness: 1.0,
        rotation: 0.0,
        dynamics: Default::default(),
        image: None,
        additional_images: Vec::new(),
        wet_edges: Default::default(),
        texture: None,
        sub_brushes: Vec::new(),
    }
}

struct Input<'a>(&'a [u8]);

impl<'a> Input<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, tail) = self.0.split_at_checked(n)?;
        self.0 = tail;
        Some(head)
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    fn be16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.array()?))
    }
    fn be32(&mut self) -> Option<usize> {
        Some(u32::from_be_bytes(self.array()?) as usize)
    }
    fn le32(&mut self) -> Option<usize> {
        Some(u32::from_le_bytes(self.array()?) as usize)
    }

    fn utf8(&mut self) -> Option<&'a str> {
        let length = self.le32()?;
        std::str::from_utf8(self.take(length)?).ok()
    }
}

fn photoshop(bytes: &[u8], stem: &str) -> Option<(Vec<BrushTip>, usize)> {
    let mut file = Input(bytes);
    let version = file.be16()?;
    let count = file.be16()?;
    let category = label(stem, "Photoshop");
    let mut tips = Vec::new();
    let mut skipped = 0;
    let mut stored = 0;
    let mut accept = |sample: Option<BrushTip>| -> Option<()> {
        if let Some(sample) = sample {
            if let Some(image) = &sample.image {
                let [w, h] = image.size;
                stored += (w * h) as usize;
            }
            if tips.len() == MAX_TIPS || stored > MAX_LIBRARY_PIXELS {
                return None;
            }
            tips.push(sample);
        } else {
            skipped += 1;
        }
        Some(())
    };
    let mut other_presets = 0;
    if matches!(version, 1 | 2) {
        for index in 1..=count {
            let kind = file.be16()?;
            let length = file.be32()?;
            let mut record = Input(file.take(length)?);
            if kind == 1 {
                accept(computed_tip(record, format!("Brush {index:03}"), &category))?;
                continue;
            }
            if kind != 2 {
                accept(None)?;
                continue;
            }
            record.take(4)?;
            let spacing = f32::from(record.be16()?) / 100.0;
            let name = if version == 2 {
                let length = record.be32()?.checked_mul(2)?;
                let units: Vec<_> = record
                    .take(length)?
                    .chunks_exact(2)
                    .take(129)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .take_while(|&c| c != 0)
                    .collect();
                label(
                    &String::from_utf16_lossy(&units),
                    &format!("Brush {index:03}"),
                )
            } else {
                format!("Brush {index:03}")
            };
            record.take(9)?;
            accept(sampled_tip(record, name, &category, spacing)?)?;
        }
    } else if matches!(version, 6 | 10) && matches!(count, 1 | 2) {
        let mut index = 0;
        let mut sections = Vec::new();
        let mut presets = Vec::new();
        while !file.0.is_empty() {
            if file.take(4)? != b"8BIM" {
                return None;
            }
            let tag = file.take(4)?;
            let length = file.be32()?;
            let data = file.take(length)?;
            match tag {
                b"samp" => sections.push(Input(data)),
                b"desc" if data.len() <= 4 * 1024 * 1024 => {
                    (presets, other_presets) =
                        photoshop_descriptor::presets(data).unwrap_or_default();
                }
                _ => {}
            }
        }
        for mut section in sections {
            while !section.0.is_empty() {
                let size = section.be32()?;
                let mut record = Input(section.take(size)?);
                section.take((4 - size % 4) % 4)?;
                let header = record.take(if count == 1 { 47 } else { 301 })?;
                let length = usize::from(header[0]);
                let id = header
                    .get(1..1 + length)
                    .and_then(|id| std::str::from_utf8(id).ok());
                index += 1;
                let Some(original) =
                    sampled_tip(record, format!("Brush {index:03}"), &category, 0.25)?
                else {
                    accept(None)?;
                    continue;
                };
                let mut found = false;
                for preset in presets
                    .iter()
                    .filter(|preset| matches!(&preset.tip, PresetTip::Sampled(sample) if Some(sample.as_str()) == id))
                {
                    let mut brush = original.clone();
                    brush.name = label(&preset.name, &brush.name);
                    if let Some(size) = preset.size {
                        brush.size_px = size.clamp(1.0, 2048.0);
                    }
                    if let Some(spacing) = preset.spacing {
                        brush.spacing = (spacing / 100.0).clamp(0.01, 2.0);
                    }
                    if let Some(angle) = preset.angle {
                        brush.rotation = (angle + 180.0).rem_euclid(360.0) - 180.0;
                    }
                    brush.dynamics = preset.dynamics.clone();
                    accept(Some(brush))?;
                    found = true;
                }
                if !found {
                    accept(Some(original))?;
                }
            }
        }
        // Keep existing sampled IDs stable when a library is imported again.
        for preset in presets {
            if let PresetTip::Computed { hardness, roundness } = preset.tip {
                index += 1;
                let name = label(&preset.name, &format!("Brush {index:03}"));
                let mut brush = tip(name, &category, TipKind::Round, preset.size?, 0.25);
                if let Some(spacing) = preset.spacing {
                    brush.spacing = (spacing / 100.0).clamp(0.01, 9.99);
                }
                if let Some(angle) = preset.angle {
                    brush.rotation = (angle + 180.0).rem_euclid(360.0) - 180.0;
                }
                brush.hardness = (hardness / 100.0).clamp(0.0, 1.0);
                brush.roundness = (roundness / 100.0).clamp(0.01, 1.0);
                brush.dynamics = preset.dynamics;
                accept(Some(brush))?;
            }
        }
    } else {
        return Some((Vec::new(), 0));
    }
    Some((tips, skipped + other_presets))
}

fn computed_tip(mut record: Input<'_>, name: String, category: &str) -> Option<BrushTip> {
    if record.0.len() != 14 { return None; }
    record.take(4)?;
    let spacing = record.be16()?;
    let diameter = record.be16()?;
    let roundness = record.be16()?;
    let angle = i16::from_be_bytes(record.array()?);
    let hardness = record.be16()?;
    if spacing > 999 || !(1..=999).contains(&diameter) || roundness > 100
        || !(-180..=180).contains(&angle) || hardness > 100
    {
        return None;
    }
    let mut brush = tip(name, category, TipKind::Round, f32::from(diameter), f32::from(spacing) / 100.0);
    brush.spacing = f32::from(spacing.max(1)) / 100.0;
    brush.roundness = f32::from(roundness.max(1)) / 100.0;
    brush.rotation = f32::from(angle);
    brush.hardness = f32::from(hardness) / 100.0;
    Some(brush)
}

fn sampled_tip(
    mut sample: Input<'_>,
    name: String,
    category: &str,
    spacing: f32,
) -> Option<Option<BrushTip>> {
    let top = i32::from_be_bytes(sample.array()?);
    let left = i32::from_be_bytes(sample.array()?);
    let bottom = i32::from_be_bytes(sample.array()?);
    let right = i32::from_be_bytes(sample.array()?);
    let width = usize::try_from(right.checked_sub(left)?).ok()?;
    let height = usize::try_from(bottom.checked_sub(top)?).ok()?;
    let area = width.checked_mul(height)?;
    if area == 0 {
        return None;
    }
    let depth = sample.be16()?;
    let [compression] = sample.array()?;
    if area > MAX_SAMPLE_PIXELS {
        return Some(None);
    }
    let mask = match (depth, compression) {
        (8, 0) => sample.take(area)?.to_vec(),
        (16, 0) => sample
            .take(area * 2)?
            .chunks_exact(2)
            .map(|p| p[1])
            .collect(),
        (8, 1) => unpack_rows(sample, width, height, area)?,
        _ => return Some(None),
    };
    if !mask.iter().any(|&alpha| alpha != 0) {
        return Some(None);
    }
    let mut preset = tip(
        name,
        category,
        TipKind::Intensity,
        width.max(height) as f32,
        spacing,
    );
    let rgba = mask
        .into_iter()
        .flat_map(|alpha| [255, 255, 255, alpha])
        .collect();
    preset.image = Some(BrushImage::from_rgba([width as u32, height as u32], rgba)?);
    Some(Some(preset))
}

fn unpack_rows(mut sample: Input<'_>, width: usize, height: usize, area: usize) -> Option<Vec<u8>> {
    let sizes = sample.take(height.checked_mul(2)?)?;
    let mut pixels = Vec::with_capacity(area);
    for length in sizes.chunks_exact(2) {
        let mut row = Input(sample.take(u16::from_be_bytes([length[0], length[1]]) as usize)?);
        let end = pixels.len() + width;
        while !row.0.is_empty() {
            let [control] = row.array()?;
            let control = control as i8;
            if control == i8::MIN {
                continue;
            }
            let count = if control < 0 {
                usize::from(control.unsigned_abs()) + 1
            } else {
                control as usize + 1
            };
            if pixels.len() + count > end {
                return None;
            }
            if control < 0 {
                let [value] = row.array()?;
                pixels.resize(pixels.len() + count, value);
            } else {
                pixels.extend_from_slice(row.take(count)?);
            }
        }
        if pixels.len() != end {
            return None;
        }
    }
    Some(pixels)
}

fn find(bytes: &[u8], marker: &[u8]) -> Option<usize> {
    bytes
        .windows(marker.len())
        .position(|window| window == marker)
}

fn after<'a>(bytes: &'a [u8], marker: &[u8]) -> Option<&'a [u8]> {
    Some(&bytes[find(bytes, marker)? + marker.len()..])
}

fn float_field(bytes: &[u8], marker: &[u8]) -> Option<f32> {
    let value = f32::from_le_bytes(after(bytes, marker)?.get(..4)?.try_into().ok()?);
    value.is_finite().then_some(value)
}
