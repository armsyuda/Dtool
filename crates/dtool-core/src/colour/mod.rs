pub mod model;
pub mod profiles;
pub mod convert;
pub mod ocio_engine;
pub mod swatch;
pub mod proof;

use std::path::PathBuf;
use dtool_protocol::CommandError;

pub use model::{ColourModel, BitDepth, ColourFormat, Colour, RenderingIntent, ConvertCtx, DocColour, ColourPrefs};
pub use profiles::{IccProfile, ProfileStore};
pub use convert::convert;
pub use ocio_engine::OcioEngine;
pub use swatch::Swatch;
pub use proof::{SoftProof, apply_soft_proof, is_active};

use std::sync::{OnceLock, RwLock};

pub struct ColourServices {
    pub profiles: ProfileStore,
    pub preferences: ColourPrefs,
}

pub fn services() -> &'static RwLock<ColourServices> {
    static SERVICES: OnceLock<RwLock<ColourServices>> = OnceLock::new();
    SERVICES.get_or_init(|| {
        let mut profiles = ProfileStore::builtin();
        profiles.load_system();
        RwLock::new(ColourServices {
            profiles,
            preferences: ColourPrefs::default(),
        })
    })
}

pub fn rgb_to_hsl(r: f32, g: f32, b: f32) -> [f32; 3] {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if (max - min).abs() < f32::EPSILON {
        return [0.0, 0.0, l * 100.0];
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r { (g - b) / d + if g < b { 6.0 } else { 0.0 } } else if max == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 };
    [h * 60.0, s * 100.0, l * 100.0]
}

pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    let (h, s, l) = (h.rem_euclid(360.0) / 360.0, (s / 100.0).clamp(0.0, 1.0), (l / 100.0).clamp(0.0, 1.0));
    if s <= 0.0 {
        return [l, l, l];
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let channel = |mut t: f32| {
        t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 { p + (q - p) * 6.0 * t } else if t < 0.5 { q } else if t < 2.0 / 3.0 { p + (q - p) * (2.0 / 3.0 - t) * 6.0 } else { p }
    };
    [channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0)]
}

pub(crate) fn rgb8_to_hsl(rgb: &[u8]) -> [f32; 3] {
    let [r, g, b] = [0, 1, 2].map(|index| f32::from(rgb[index]) / 255.0);
    rgb_to_hsl(r, g, b)
}

pub(crate) fn mix_chroma(rgb: &[u8], hue: f32, saturation: f32, amount: f32) -> [u8; 3] {
    let [original_hue, original_saturation, lightness] = rgb8_to_hsl(rgb);
    let from = if original_saturation <= f32::EPSILON { hue } else { original_hue };
    let to = if saturation <= f32::EPSILON { from } else { hue };
    let delta = (to - from + 180.0).rem_euclid(360.0) - 180.0;
    hsl_to_rgb(from + delta * amount, original_saturation + (saturation - original_saturation) * amount, lightness)
        .map(|value| (value * 255.0).round() as u8)
}

pub fn parse_intent(name: &str) -> Option<RenderingIntent> {
    match name.trim().to_ascii_lowercase().replace([' ', '-'], "_").as_str() {
        "perceptual" => Some(RenderingIntent::Perceptual),
        "relative" | "relative_colorimetric" | "relativecolorimetric" => Some(RenderingIntent::RelativeColorimetric),
        "saturation" => Some(RenderingIntent::Saturation),
        "absolute" | "absolute_colorimetric" | "absolutecolorimetric" => Some(RenderingIntent::AbsoluteColorimetric),
        _ => None,
    }
}

impl From<&crate::prefs::Prefs> for ColourPrefs {
    fn from(prefs: &crate::prefs::Prefs) -> Self {
        let base = ColourPrefs::default();
        let named = |value: &str, fallback: &str| if value.trim().is_empty() { fallback.to_string() } else { value.to_string() };
        ColourPrefs {
            rgb: named(&prefs.rgb_profile, &base.rgb),
            rgb32: named(&prefs.rgb32_profile, &base.rgb32),
            cmyk: named(&prefs.cmyk_profile, &base.cmyk),
            grey: named(&prefs.grey_profile, &base.grey),
            lab: named(&prefs.lab_profile, &base.lab),
            intent: parse_intent(&prefs.rendering_intent).unwrap_or(base.intent),
            bpc: prefs.black_point_compensation,
            convert_on_open: prefs.convert_to_working,
            warn_on_convert: prefs.convert_warn,
            warn_assign_unprofiled: prefs.warn_assign_unprofiled,
            hdr_default: prefs.hdr_32bit_default,
            ocio_config: (!prefs.ocio_config.trim().is_empty()).then(|| PathBuf::from(prefs.ocio_config.trim())),
            ocio_by_filename: prefs.ocio_by_filename,
            ocio_warn: prefs.ocio_warn_on_load,
        }
    }
}

pub fn sync_prefs(prefs: &ColourPrefs) -> Result<(), CommandError> {
    let mut services = services().write().map_err(|_| CommandError::new("INTERNAL"))?;
    services.preferences = prefs.clone();
    Ok(())
}

pub fn sync_app_prefs(prefs: &crate::prefs::Prefs) -> Result<(), CommandError> {
    sync_prefs(&ColourPrefs::from(prefs))
}

#[derive(Debug, Clone)]
pub struct OcioConfig {
    pub path: PathBuf,
    pub spaces: Vec<String>,
}

impl OcioConfig {
    pub fn load(path: &PathBuf) -> Result<OcioConfig, CommandError> {
        if !path.exists() {
            return Err(CommandError::validation("OCIO_CONFIG_INVALID"));
        }
        
        let content = std::fs::read_to_string(path).map_err(|_| CommandError::validation("OCIO_CONFIG_INVALID"))?;
        
        let spaces = parse_ocio_spaces(&content);
        
        Ok(OcioConfig {
            path: path.clone(),
            spaces,
        })
    }
}

fn parse_ocio_spaces(content: &str) -> Vec<String> {
    let mut spaces = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("name:") {
            if let Some(name) = trimmed.split(':').nth(1) {
                let name = name.trim().trim_matches('"');
                if !name.is_empty() {
                    spaces.push(name.to_string());
                }
            }
        }
    }
    spaces
}
