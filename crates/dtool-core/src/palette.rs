use crate::colour::swatch::Swatch;
use crate::colour::{Colour as ManagedColour, ColourModel};
use crate::paint::Colour;
use dtool_protocol::CommandError;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

pub const APP_DEFAULT: &str = "Application";
pub const PATH_ENV: &str = "DTOOL_PALETTE_PATH";
pub const PALETTE_NOT_FOUND: &str = "PALETTE_NOT_FOUND";
pub const PALETTE_EXISTS: &str = "PALETTE_EXISTS";
pub const PALETTE_FORMAT: &str = "PALETTE_FORMAT";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaletteScope {
    #[default]
    Document,
    Application,
    System,
}

impl PaletteScope {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "document" | "doc" => Some(Self::Document),
            "application" | "app" => Some(Self::Application),
            "system" => Some(Self::System),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Palette {
    pub name: String,
    pub scope: PaletteScope,
    pub swatches: Vec<Swatch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaletteSort {
    Name,
    Hue,
    Lightness,
    #[default]
    None,
}

impl PaletteSort {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "name" => Some(Self::Name),
            "hue" => Some(Self::Hue),
            "lightness" => Some(Self::Lightness),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaletteView {
    List,
    #[default]
    Grid,
}

impl PaletteView {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "list" => Some(Self::List),
            "grid" => Some(Self::Grid),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PaletteUi {
    pub scope: PaletteScope,
    pub palette: String,
    pub view: PaletteView,
    pub sort: PaletteSort,
    pub search: String,
}

thread_local! {
    static UI: RefCell<PaletteUi> = RefCell::new(PaletteUi::default());
}

pub fn ui_state() -> PaletteUi {
    UI.with(|state| state.borrow().clone())
}

pub fn set_ui_state(state: PaletteUi) {
    UI.with(|slot| *slot.borrow_mut() = state);
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AppPalettes {
    #[serde(default)]
    pub palettes: Vec<Palette>,
    #[serde(default)]
    pub next_id: u64,
    #[serde(default)]
    pub recent: Vec<crate::paint::Colour>,
}

impl AppPalettes {
    pub fn next_id(&self) -> u64 {
        let used = self.palettes.iter().flat_map(|palette| palette.swatches.iter()).map(|swatch| swatch.id).max().unwrap_or(0);
        self.next_id.max(used + 1).max(1)
    }
    pub fn find(&self, name: &str) -> Option<usize> {
        self.palettes.iter().position(|palette| palette.name == name)
    }
    pub fn ensure(&mut self, name: &str) -> usize {
        match self.find(name) {
            Some(index) => index,
            None => {
                self.palettes.push(Palette { name: name.to_string(), scope: PaletteScope::Application, swatches: Vec::new() });
                self.palettes.len() - 1
            }
        }
    }
}

fn path_override() -> &'static RwLock<Option<PathBuf>> {
    static OVERRIDE: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();
    OVERRIDE.get_or_init(|| RwLock::new(None))
}

pub fn set_app_path(path: Option<PathBuf>) {
    if let Ok(mut slot) = path_override().write() {
        *slot = path;
    }
}

pub fn app_path() -> PathBuf {
    if let Some(path) = path_override().read().ok().and_then(|slot| slot.clone()) {
        return path;
    }
    if let Some(path) = std::env::var_os(PATH_ENV) {
        return PathBuf::from(path);
    }
    let mut path = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    path.push("DTool");
    path.push("palettes.json");
    path
}

pub fn load_app(path: &Path) -> AppPalettes {
    std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

pub fn save_app(path: &Path, palettes: &AppPalettes) -> Result<(), CommandError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| CommandError::new("IO.OTHER"))?;
    }
    let text = serde_json::to_string_pretty(palettes).map_err(|_| CommandError::invalid_params())?;
    std::fs::write(path, text).map_err(|_| CommandError::new("IO.OTHER"))
}

fn app_store() -> &'static RwLock<Option<(PathBuf, AppPalettes)>> {
    static STORE: OnceLock<RwLock<Option<(PathBuf, AppPalettes)>>> = OnceLock::new();
    STORE.get_or_init(|| RwLock::new(None))
}

pub fn with_app<R>(save: bool, f: impl FnOnce(&mut AppPalettes) -> Result<R, CommandError>) -> Result<R, CommandError> {
    let path = app_path();
    let mut guard = app_store().write().map_err(|_| CommandError::new("INTERNAL"))?;
    if guard.as_ref().map(|(known, _)| known != &path).unwrap_or(true) {
        *guard = Some((path.clone(), load_app(&path)));
    }
    let (_, palettes) = guard.as_mut().ok_or_else(|| CommandError::new("INTERNAL"))?;
    let mut working = palettes.clone();
    let result = f(&mut working)?;
    if save && working != *palettes {
        save_app(&path, &working)?;
        *palettes = working;
    }
    Ok(result)
}

pub const RECENT_COLOURS_MAX: usize = 16;

pub fn push_recent_into(recent: &mut Vec<crate::paint::Colour>, colour: crate::paint::Colour) {
    recent.retain(|known| (known.r, known.g, known.b, known.a) != (colour.r, colour.g, colour.b, colour.a));
    recent.insert(0, colour);
    recent.truncate(RECENT_COLOURS_MAX);
}

pub fn push_recent(colour: crate::paint::Colour) -> Result<(), CommandError> {
    with_app(true, |palettes| {
        push_recent_into(&mut palettes.recent, colour);
        Ok(())
    })
}

pub fn recent_colours() -> Vec<crate::paint::Colour> {
    with_app(false, |palettes| Ok(palettes.recent.clone())).unwrap_or_default()
}

pub fn app_palettes() -> Vec<Palette> {
    with_app(false, |palettes| Ok(palettes.palettes.clone())).unwrap_or_default()
}

fn hsl(colour: &Colour) -> (f32, f32) {
    let r = colour.r as f32 / 255.0;
    let g = colour.g as f32 / 255.0;
    let b = colour.b as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let lightness = (max + min) / 2.0;
    let delta = max - min;
    if delta <= f32::EPSILON {
        return (0.0, lightness);
    }
    let hue = if max == r {
        ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    (hue * 60.0, lightness)
}

pub fn ordered_ids(swatches: &[Swatch], sort: PaletteSort, search: &str) -> Vec<u64> {
    let needle = search.trim().to_lowercase();
    let mut items: Vec<&Swatch> = swatches.iter().filter(|swatch| needle.is_empty() || swatch.name.to_lowercase().contains(&needle)).collect();
    match sort {
        PaletteSort::Name => items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.id.cmp(&b.id))),
        PaletteSort::Hue => items.sort_by(|a, b| hsl(&a.colour).0.total_cmp(&hsl(&b.colour).0).then(a.id.cmp(&b.id))),
        PaletteSort::Lightness => items.sort_by(|a, b| hsl(&a.colour).1.total_cmp(&hsl(&b.colour).1).then(a.id.cmp(&b.id))),
        PaletteSort::None => {}
    }
    items.into_iter().map(|swatch| swatch.id).collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct AseEntry {
    pub name: String,
    pub colour: Colour,
    pub global: bool,
    pub spot: bool,
}

fn put_name(out: &mut Vec<u8>, name: &str) {
    let units: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    out.extend_from_slice(&(units.len() as u16).to_be_bytes());
    for unit in units {
        out.extend_from_slice(&unit.to_be_bytes());
    }
}

fn block(out: &mut Vec<u8>, kind: u16, body: &[u8]) {
    out.extend_from_slice(&kind.to_be_bytes());
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
}

pub fn encode_ase(group: &str, entries: &[AseEntry]) -> Vec<u8> {
    let mut out = b"ASEF".to_vec();
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&((entries.len() + 2) as u32).to_be_bytes());
    let mut body = Vec::new();
    put_name(&mut body, group);
    block(&mut out, 0xC001, &body);
    for entry in entries {
        let managed = entry.colour.managed();
        let (model, values): (&[u8; 4], Vec<f32>) = match managed.model {
            ColourModel::Rgb => (b"RGB ", managed.values[..3].to_vec()),
            ColourModel::Cmyk => (b"CMYK", managed.values.iter().map(|v| v / 100.0).collect()),
            ColourModel::Lab => (b"LAB ", vec![managed.values[0] / 100.0, managed.values[1], managed.values[2]]),
            ColourModel::Grey => (b"Gray", vec![managed.values[0] / 100.0]),
        };
        let mut body = Vec::new();
        put_name(&mut body, &entry.name);
        body.extend_from_slice(model);
        for value in values {
            body.extend_from_slice(&value.to_be_bytes());
        }
        let kind: u16 = if entry.spot { 1 } else if entry.global { 0 } else { 2 };
        body.extend_from_slice(&kind.to_be_bytes());
        block(&mut out, 0x0001, &body);
    }
    block(&mut out, 0xC002, &[]);
    out
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, count: usize) -> Result<&[u8], CommandError> {
        let end = self.at.checked_add(count).filter(|end| *end <= self.bytes.len()).ok_or_else(|| CommandError::new(PALETTE_FORMAT))?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }
    fn u16(&mut self) -> Result<u16, CommandError> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }
    fn u32(&mut self) -> Result<u32, CommandError> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
    fn f32(&mut self) -> Result<f32, CommandError> {
        let bytes = self.take(4)?;
        Ok(f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
    fn name(&mut self) -> Result<String, CommandError> {
        let count = self.u16()? as usize;
        let mut units = Vec::with_capacity(count);
        for _ in 0..count {
            units.push(self.u16()?);
        }
        while units.last() == Some(&0) {
            units.pop();
        }
        String::from_utf16(&units).map_err(|_| CommandError::new(PALETTE_FORMAT))
    }
}

pub fn decode_ase(bytes: &[u8]) -> Result<(Option<String>, Vec<AseEntry>), CommandError> {
    let mut reader = Reader { bytes, at: 0 };
    if reader.take(4)? != b"ASEF" {
        return Err(CommandError::new(PALETTE_FORMAT));
    }
    reader.u16()?;
    reader.u16()?;
    let count = reader.u32()?;
    let mut group = None;
    let mut entries = Vec::new();
    for _ in 0..count {
        let kind = reader.u16()?;
        let length = reader.u32()? as usize;
        let start = reader.at;
        match kind {
            0xC001 => {
                let name = reader.name()?;
                if group.is_none() && !name.is_empty() {
                    group = Some(name);
                }
            }
            0x0001 => {
                let name = reader.name()?;
                let model = reader.take(4)?.to_vec();
                let managed = match model.as_slice() {
                    b"RGB " => ManagedColour::rgb(reader.f32()?, reader.f32()?, reader.f32()?, 1.0),
                    b"CMYK" => ManagedColour::cmyk(reader.f32()? * 100.0, reader.f32()? * 100.0, reader.f32()? * 100.0, reader.f32()? * 100.0, 1.0),
                    b"LAB " => ManagedColour::lab(reader.f32()? * 100.0, reader.f32()?, reader.f32()?, 1.0),
                    b"Gray" => ManagedColour::grey(reader.f32()? * 100.0, 1.0),
                    _ => return Err(CommandError::new(PALETTE_FORMAT)),
                };
                let colour_type = reader.u16()?;
                let colour = if managed.model == ColourModel::Rgb {
                    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
                    Colour::new(channel(managed.values[0]), channel(managed.values[1]), channel(managed.values[2]), 255)
                } else {
                    Colour::from_managed(managed)
                };
                entries.push(AseEntry { name, colour, global: colour_type == 0 || colour_type == 1, spot: colour_type == 1 });
            }
            _ => {}
        }
        reader.at = start.checked_add(length).filter(|end| *end <= bytes.len()).ok_or_else(|| CommandError::new(PALETTE_FORMAT))?;
    }
    Ok((group, entries))
}

pub fn entries_of(swatches: &[Swatch]) -> Vec<AseEntry> {
    swatches.iter().map(|swatch| AseEntry { name: swatch.name.clone(), colour: swatch.colour, global: swatch.global, spot: swatch.spot }).collect()
}