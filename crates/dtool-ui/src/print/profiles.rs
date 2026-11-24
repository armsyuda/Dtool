use std::{collections::BTreeMap, fs, io, path::{Path, PathBuf}};

use dtool_core::colour::RenderingIntent;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SizeMode {
    #[default]
    Actual,
    Fit,
    Scale,
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum PageSubset {
    #[default]
    All,
    Odd,
    Even,
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ColourMode {
    #[default]
    Driver,
    Windows,
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum PageLayout {
    #[default]
    Single,
    Two,
    Four,
}

impl PageLayout {
    pub(super) fn per_side(self) -> usize {
        match self {
            Self::Single => 1,
            Self::Two => 2,
            Self::Four => 4,
        }
    }
}

impl PageSubset {
    pub(super) fn includes(self, number: usize) -> bool {
        match self {
            Self::All => true,
            Self::Odd => number % 2 == 1,
            Self::Even => number % 2 == 0,
        }
    }
}

#[derive(Clone, Copy, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub(super) struct PrintOptions {
    pub size_mode: SizeMode,
    pub scale: u32,
    pub subset: PageSubset,
    pub reverse: bool,
    pub dpi: u32,
    pub clip_transparency: bool,
    pub prefer_vectors: bool,
    pub copies: usize,
    pub collate: bool,
    pub bleed: bool,
    pub crop_marks: bool,
    pub fold_marks: bool,
    pub registration_marks: bool,
    pub color_bars: bool,
    pub page_info: bool,
    pub colour_mode: ColourMode,
    pub intent: RenderingIntent,
    pub page_layout: PageLayout,
}

impl Default for PrintOptions {
    fn default() -> Self {
        Self {
            size_mode: SizeMode::Actual, scale: 100, subset: PageSubset::All, reverse: false,
            dpi: 300, clip_transparency: false, prefer_vectors: false,
            copies: 1, collate: false, bleed: false, crop_marks: false, fold_marks: false,
            registration_marks: false, color_bars: false, page_info: false,
            colour_mode: ColourMode::Driver, intent: RenderingIntent::Perceptual,
            page_layout: PageLayout::Single,
        }
    }
}

pub(super) type Profiles = BTreeMap<String, PrintOptions>;

fn path() -> PathBuf {
    dtool_core::prefs::Prefs::default_path().with_file_name("print_profiles.json")
}

fn read(path: &Path) -> io::Result<Profiles> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Profiles::new()),
        Err(error) => return Err(error),
    };
    let mut entries: Profiles = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    for options in entries.values_mut() {
        options.copies = options.copies.clamp(1, 99);
        options.dpi = options.dpi.clamp(72, 600);
        options.scale = options.scale.clamp(1, 500);
    }
    Ok(entries)
}

pub(super) fn load() -> io::Result<Profiles> {
    read(&path())
}

pub(super) enum SaveOutcome {
    Saved(Profiles),
    Changed(Profiles),
}

pub(super) fn save(name: &str, options: PrintOptions, expected: Option<PrintOptions>) -> io::Result<SaveOutcome> {
    let path = path();
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    // Serialize read–modify–write across app instances without blocking the UI.
    let lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false)
        .open(path.with_extension("lock"))?;
    lock.try_lock().map_err(io::Error::other)?;
    let mut entries = read(&path)?;
    if entries.get(name).copied() != expected {
        return Ok(SaveOutcome::Changed(entries));
    }
    entries.insert(name.to_owned(), options);
    let body = serde_json::to_vec_pretty(&entries).map_err(io::Error::other)?;
    dtool_core::native_io::write_atomic(&path, &body).map_err(|error| io::Error::other(format!("{error:?}")))?;
    Ok(SaveOutcome::Saved(entries))
}
