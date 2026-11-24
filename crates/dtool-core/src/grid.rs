use serde::{Deserialize, Serialize};

pub mod baseline;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum GridKind {
    #[default]
    Standard,
    Isometric,
    Axonometric,
    Custom,
}

impl GridKind {
    pub const ALL: [GridKind; 4] = [GridKind::Standard, GridKind::Isometric, GridKind::Axonometric, GridKind::Custom];

    pub fn key(self) -> &'static str {
        match self {
            GridKind::Standard => "standard",
            GridKind::Isometric => "isometric",
            GridKind::Axonometric => "axonometric",
            GridKind::Custom => "custom",
        }
    }

    pub fn from_key(key: &str) -> Option<GridKind> {
        GridKind::ALL.iter().copied().find(|kind| kind.key().eq_ignore_ascii_case(key))
    }

    pub fn label_key(self) -> String {
        format!("grid.kind.{}", self.key())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Grid {
    pub kind: GridKind,
    pub show: bool,
    pub spacing: [f32; 2],
    pub subdivisions: u32,
    pub show_pixel: bool,
    pub show_baseline: bool,
    pub baseline_spacing: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline: Option<baseline::Settings>,
    pub origin: [f32; 2],
    pub angles: [f32; 2],
}

impl Default for Grid {
    fn default() -> Self {
        Grid {
            kind: GridKind::Standard,
            show: false,
            spacing: [64.0, 64.0],
            subdivisions: 4,
            show_pixel: false,
            show_baseline: false,
            baseline_spacing: 12.0,
            baseline: None,
            origin: [0.0, 0.0],
            angles: [30.0, 30.0],
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridSetArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<GridKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdivisions: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_pixel: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_baseline: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_spacing: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "baseline_update")]
    pub baseline: Option<Option<baseline::Settings>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angles: Option<[f32; 2]>,
}

impl GridSetArgs {
    pub fn parse(params: &serde_json::Value) -> Result<GridSetArgs, GridError> {
        let params = if params.is_null() { serde_json::json!({}) } else { params.clone() };
        let args: GridSetArgs = serde_json::from_value(params).map_err(|_| GridError::InvalidParams)?;
        args.validate()?;
        Ok(args)
    }

    pub fn validate(&self) -> Result<(), GridError> {
        if let Some(spacing) = self.spacing {
            if !spacing.iter().all(|v| v.is_finite() && *v > 0.0) {
                return Err(GridError::InvalidSpacing);
            }
        }
        if let Some(baseline_spacing) = self.baseline_spacing {
            if !baseline_spacing.is_finite() || baseline_spacing <= 0.0 {
                return Err(GridError::InvalidSpacing);
            }
        }
        if self.baseline.flatten().is_some_and(|settings| !settings.valid()) { return Err(GridError::InvalidParams); }
        if let Some(subdivisions) = self.subdivisions {
            if subdivisions == 0 {
                return Err(GridError::InvalidSubdivisions);
            }
        }
        if self.origin.is_some_and(|origin| !origin.iter().all(|v| v.is_finite())) {
            return Err(GridError::InvalidParams);
        }
        if self.angles.is_some_and(|angles| !angles.iter().all(|v| v.is_finite() && v.abs() <= 360.0)) {
            return Err(GridError::InvalidParams);
        }
        Ok(())
    }

    pub fn from_grid(grid: &Grid) -> GridSetArgs {
        GridSetArgs {
            kind: Some(grid.kind),
            show: Some(grid.show),
            spacing: Some(grid.spacing),
            subdivisions: Some(grid.subdivisions),
            show_pixel: Some(grid.show_pixel),
            show_baseline: Some(grid.show_baseline),
            baseline_spacing: Some(grid.baseline_spacing),
            baseline: Some(grid.baseline),
            origin: Some(grid.origin),
            angles: Some(grid.angles),
        }
    }
}

fn baseline_update<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<Option<baseline::Settings>>, D::Error> {
    Option::<baseline::Settings>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridError {
    InvalidParams,
    InvalidSpacing,
    InvalidSubdivisions,
}

impl GridError {
    pub fn message_key(self) -> &'static str {
        match self {
            GridError::InvalidParams => "error.invalid_params",
            GridError::InvalidSpacing => "error.grid.spacing",
            GridError::InvalidSubdivisions => "error.grid.subdivisions",
        }
    }
}

impl Grid {
    pub fn set(&mut self, args: &GridSetArgs) -> Result<(), GridError> {
        args.validate()?;
        if let Some(kind) = args.kind {
            self.kind = kind;
        }
        if let Some(show) = args.show {
            self.show = show;
        }
        if let Some(spacing) = args.spacing {
            self.spacing = spacing;
        }
        if let Some(subdivisions) = args.subdivisions {
            self.subdivisions = subdivisions;
        }
        if let Some(show_pixel) = args.show_pixel {
            self.show_pixel = show_pixel;
        }
        if let Some(show_baseline) = args.show_baseline {
            self.show_baseline = show_baseline;
        }
        if let Some(baseline_spacing) = args.baseline_spacing {
            self.baseline_spacing = baseline_spacing;
        }
        if let Some(baseline) = args.baseline { self.baseline = baseline; }
        if let Some(origin) = args.origin {
            self.origin = origin;
        }
        if let Some(angles) = args.angles {
            self.angles = angles;
        }
        Ok(())
    }

    pub fn query(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct QuickGrid {
    pub kind: GridKind,
    pub spacing: [f64; 2],
    pub divisions: u32,
}

impl QuickGrid {
    pub fn square() -> Self {
        QuickGrid {
            kind: GridKind::Standard,
            spacing: [64.0, 64.0],
            divisions: 4,
        }
    }

    pub fn isometric() -> Self {
        QuickGrid {
            kind: GridKind::Isometric,
            spacing: [64.0, 37.0],
            divisions: 4,
        }
    }

    pub fn to_grid_set_args(&self) -> GridSetArgs {
        GridSetArgs {
            kind: Some(self.kind),
            show: Some(true),
            spacing: Some([self.spacing[0] as f32, self.spacing[1] as f32]),
            subdivisions: Some(self.divisions),
            show_pixel: None,
            show_baseline: None,
            baseline_spacing: None,
            baseline: None,
            origin: None,
            angles: None,
        }
    }

    pub fn parse(params: &serde_json::Value) -> Result<QuickGrid, GridError> {
        let params = if params.is_null() { serde_json::json!({}) } else { params.clone() };
        let kind_str = params.get("kind").and_then(|v| v.as_str()).unwrap_or("square");
        
        let quick_grid = match kind_str {
            "square" => QuickGrid::square(),
            "isometric" => QuickGrid::isometric(),
            _ => return Err(GridError::InvalidParams),
        };

        if let Some(spacing_val) = params.get("spacing") {
            if let Some(arr) = spacing_val.as_array() {
                if arr.len() == 2 {
                    let x = arr[0].as_f64().ok_or(GridError::InvalidSpacing)?;
                    let y = arr[1].as_f64().ok_or(GridError::InvalidSpacing)?;
                    if !x.is_finite() || !y.is_finite() || x <= 0.0 || y <= 0.0 {
                        return Err(GridError::InvalidSpacing);
                    }
                    return Ok(QuickGrid { spacing: [x, y], ..quick_grid });
                }
            }
        }

        if let Some(divisions_val) = params.get("divisions") {
            if let Some(d) = divisions_val.as_u64() {
                if d == 0 {
                    return Err(GridError::InvalidSubdivisions);
                }
                return Ok(QuickGrid { divisions: d as u32, ..quick_grid });
            }
        }

        Ok(quick_grid)
    }
}
