use std::fs;
use std::io;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SnapPrefs {
    pub enabled: bool,
    pub screen_tolerance: u32,
    pub candidates_mode: String,
    pub max_candidates: u32,
    pub show_candidates: bool,
    pub force_pixel_alignment: bool,
    pub move_by_whole_pixels: bool,
    pub page_snapping: bool,
    pub snap_to_grid: bool,
    pub snap_to_baseline_grid: bool,
    pub snap_to_guides: bool,
    pub snap_to_spreads: bool,
    pub include_spread_midpoints: bool,
    pub snap_to_margin: bool,
    pub include_margin_midpoints: bool,
    pub object_snapping: bool,
    pub only_visible_objects: bool,
    pub snap_to_object_bounds: bool,
    pub include_bbox_midpoints: bool,
    pub snap_to_gaps_and_sizes: bool,
    pub snap_to_shape_key_points: bool,
    pub snap_to_object_geometry: bool,
    pub snap_to_pixel_selection_bounds: bool,
    pub construction: bool,
    pub curve: bool,
    pub dynamic_guides: bool,
}

impl Default for SnapPrefs {
    fn default() -> Self {
        Self {
            enabled: true,
            screen_tolerance: 8,
            candidates_mode: "Candidate List".to_string(),
            max_candidates: 6,
            show_candidates: false,
            force_pixel_alignment: false,
            move_by_whole_pixels: true,
            page_snapping: true,
            snap_to_grid: false,
            snap_to_baseline_grid: false,
            snap_to_guides: true,
            snap_to_spreads: true,
            include_spread_midpoints: true,
            snap_to_margin: true,
            include_margin_midpoints: true,
            object_snapping: true,
            only_visible_objects: true,
            snap_to_object_bounds: true,
            include_bbox_midpoints: true,
            snap_to_gaps_and_sizes: false,
            snap_to_shape_key_points: false,
            snap_to_object_geometry: false,
            snap_to_pixel_selection_bounds: false,
            construction: false,
            curve: false,
            dynamic_guides: true,
        }
    }
}

impl SnapPrefs {
    pub fn load(path: PathBuf) -> Self {
        match fs::read_to_string(&path) {
            Ok(content) => {
                match serde_json::from_str::<serde_json::Value>(&content) {
                    Ok(value) => {
                        if value.is_object() && value.get("snap").is_some() {
                            return Self::default();
                        }
                        match serde_json::from_value::<SnapPrefs>(value) {
                            Ok(prefs) => prefs,
                            Err(_) => Self::default(),
                        }
                    }
                    Err(_) => Self::default(),
                }
            }
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: PathBuf) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        fs::write(path, content)
    }

    pub fn default_path() -> PathBuf {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return PathBuf::from(appdata).join("DTool").join("snap_prefs.json");
        }
        PathBuf::from("./snap_prefs.json")
    }

    pub fn active_path() -> PathBuf {
        THREAD_PATH.with(|cell| cell.borrow().clone()).unwrap_or_else(Self::default_path)
    }
}

thread_local! {
    static THREAD_PATH: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

pub fn set_thread_path(path: Option<PathBuf>) {
    THREAD_PATH.with(|cell| *cell.borrow_mut() = path);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapPrefsSetArgs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen_tolerance: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_candidates: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_candidates: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_pixel_alignment: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub move_by_whole_pixels: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_snapping: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snap_to_grid: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snap_to_baseline_grid: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snap_to_guides: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snap_to_spreads: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_spread_midpoints: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snap_to_margin: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_margin_midpoints: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_snapping: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub only_visible_objects: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snap_to_object_bounds: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_bbox_midpoints: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snap_to_gaps_and_sizes: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snap_to_shape_key_points: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snap_to_object_geometry: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snap_to_pixel_selection_bounds: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub construction: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curve: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dynamic_guides: Option<bool>,
}

pub const CANDIDATE_MODES: &[&str] = &["Candidate List"];
pub const SCREEN_TOLERANCE_RANGE: std::ops::RangeInclusive<u32> = 1..=100;
pub const MAX_CANDIDATES_RANGE: std::ops::RangeInclusive<u32> = 1..=50;

impl SnapPrefsSetArgs {
    pub fn is_valid(&self) -> bool {
        self.screen_tolerance.map_or(true, |v| SCREEN_TOLERANCE_RANGE.contains(&v))
            && self.max_candidates.map_or(true, |v| MAX_CANDIDATES_RANGE.contains(&v))
            && self.candidates_mode.as_deref().map_or(true, |m| CANDIDATE_MODES.contains(&m))
    }
}

#[derive(Debug)]
pub struct SnapPrefsService {
    prefs: std::sync::Arc<std::sync::RwLock<SnapPrefs>>,
    path: std::sync::Arc<PathBuf>,
}

impl SnapPrefsService {
    pub fn new() -> Self {
        Self::with_path(SnapPrefs::active_path())
    }

    pub fn with_path(path: PathBuf) -> Self {
        let prefs = SnapPrefs::load(path.clone());
        Self { prefs: std::sync::Arc::new(std::sync::RwLock::new(prefs)), path: std::sync::Arc::new(path) }
    }

    pub fn share(&self) -> Self {
        Self { prefs: std::sync::Arc::clone(&self.prefs), path: std::sync::Arc::clone(&self.path) }
    }

    pub fn is_shared_with(&self, other: &SnapPrefsService) -> bool {
        std::sync::Arc::ptr_eq(&self.prefs, &other.prefs)
    }

    pub fn path(&self) -> &std::path::Path {
        self.path.as_path()
    }

    pub fn query(&self) -> SnapPrefs {
        match self.prefs.read() {
            Ok(prefs) => prefs.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    pub fn replace(&mut self, prefs: SnapPrefs) {
        match self.prefs.write() {
            Ok(mut current) => *current = prefs,
            Err(poisoned) => *poisoned.into_inner() = prefs,
        }
    }

    pub fn set(&mut self, args: SnapPrefsSetArgs) -> io::Result<()> {
        if !args.is_valid() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "snap prefs out of range"));
        }
        let mut next = self.query();
        if let Some(v) = args.enabled {
            next.enabled = v;
        }
        if let Some(v) = args.screen_tolerance {
            next.screen_tolerance = v;
        }
        if let Some(v) = args.candidates_mode {
            next.candidates_mode = v;
        }
        if let Some(v) = args.max_candidates {
            next.max_candidates = v;
        }
        if let Some(v) = args.show_candidates {
            next.show_candidates = v;
        }
        if let Some(v) = args.force_pixel_alignment {
            next.force_pixel_alignment = v;
        }
        if let Some(v) = args.move_by_whole_pixels {
            next.move_by_whole_pixels = v;
        }
        if let Some(v) = args.page_snapping {
            next.page_snapping = v;
        }
        if let Some(v) = args.snap_to_grid {
            next.snap_to_grid = v;
        }
        if let Some(v) = args.snap_to_baseline_grid {
            next.snap_to_baseline_grid = v;
        }
        if let Some(v) = args.snap_to_guides {
            next.snap_to_guides = v;
        }
        if let Some(v) = args.snap_to_spreads {
            next.snap_to_spreads = v;
        }
        if let Some(v) = args.include_spread_midpoints {
            next.include_spread_midpoints = v;
        }
        if let Some(v) = args.snap_to_margin {
            next.snap_to_margin = v;
        }
        if let Some(v) = args.include_margin_midpoints {
            next.include_margin_midpoints = v;
        }
        if let Some(v) = args.object_snapping {
            next.object_snapping = v;
        }
        if let Some(v) = args.only_visible_objects {
            next.only_visible_objects = v;
        }
        if let Some(v) = args.snap_to_object_bounds {
            next.snap_to_object_bounds = v;
        }
        if let Some(v) = args.include_bbox_midpoints {
            next.include_bbox_midpoints = v;
        }
        if let Some(v) = args.snap_to_gaps_and_sizes {
            next.snap_to_gaps_and_sizes = v;
        }
        if let Some(v) = args.snap_to_shape_key_points {
            next.snap_to_shape_key_points = v;
        }
        if let Some(v) = args.snap_to_object_geometry {
            next.snap_to_object_geometry = v;
        }
        if let Some(v) = args.snap_to_pixel_selection_bounds {
            next.snap_to_pixel_selection_bounds = v;
        }
        if let Some(v) = args.construction {
            next.construction = v;
        }
        if let Some(v) = args.curve {
            next.curve = v;
        }
        if let Some(v) = args.dynamic_guides {
            next.dynamic_guides = v;
        }

        next.save(self.path.as_ref().clone())?;
        self.replace(next);
        Ok(())
    }
}

impl SnapPrefsService {
    pub fn candidates(&self,document:&crate::snap_candidates::SnapDocument<'_>,view:&crate::ViewState,point:[f32;2])->Vec<crate::snap_candidates::Candidate> {
        let snapshot=self.query();
        crate::snap_candidates::snap_candidates(document,view,&snapshot,point)
    }
}
