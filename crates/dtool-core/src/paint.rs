use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy)]
pub struct Colour {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
    pub model: crate::colour::ColourModel,
    pub values: [f32; 4],
}

impl Colour {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r,
            g,
            b,
            a,
            model: crate::colour::ColourModel::Rgb,
            values: [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 0.0],
        }
    }

    pub fn from_managed(mut colour: crate::colour::Colour) -> Self {
        colour.clamp_on_write();
        let (r, g, b) = preview_rgb(&colour);
        Self {
            r,
            g,
            b,
            a: (colour.alpha * 255.0).round().clamp(0.0, 255.0) as u8,
            model: colour.model,
            values: colour.values,
        }
    }

    pub fn managed(&self) -> crate::colour::Colour {
        if self.model == crate::colour::ColourModel::Rgb {
            crate::colour::Colour {
                model: crate::colour::ColourModel::Rgb,
                values: [self.r as f32 / 255.0, self.g as f32 / 255.0, self.b as f32 / 255.0, 0.0],
                alpha: self.a as f32 / 255.0,
            }
        } else {
            crate::colour::Colour {
                model: self.model,
                values: self.values,
                alpha: self.a as f32 / 255.0,
            }
        }
    }

    pub fn write_for_document(&self, format: crate::colour::ColourFormat, prefs: &crate::colour::ColourPrefs) -> Self {
        Self::from_managed(self.managed().write_for_document(format, prefs))
    }
}

impl PartialEq for Colour {
    fn eq(&self, other: &Self) -> bool {
        if self.model != other.model || self.a != other.a {
            return false;
        }
        match self.model {
            crate::colour::ColourModel::Rgb => self.r == other.r && self.g == other.g && self.b == other.b,
            _ => self.values.iter().zip(other.values.iter()).all(|(left, right)| left.to_bits() == right.to_bits()),
        }
    }
}

impl Eq for Colour {}

impl std::hash::Hash for Colour {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.model.hash(state);
        self.a.hash(state);
        match self.model {
            crate::colour::ColourModel::Rgb => {
                self.r.hash(state);
                self.g.hash(state);
                self.b.hash(state);
            }
            _ => {
                for value in self.values {
                    value.to_bits().hash(state);
                }
            }
        }
    }
}

impl Serialize for Colour {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        if self.model == crate::colour::ColourModel::Rgb {
            let mut rec = serializer.serialize_struct("Colour", 4)?;
            rec.serialize_field("r", &self.r)?;
            rec.serialize_field("g", &self.g)?;
            rec.serialize_field("b", &self.b)?;
            rec.serialize_field("a", &self.a)?;
            rec.end()
        } else {
            let mut rec = serializer.serialize_struct("Colour", 3)?;
            rec.serialize_field("model", &self.model)?;
            rec.serialize_field("values", &self.values)?;
            rec.serialize_field("alpha", &(self.a as f32 / 255.0))?;
            rec.end()
        }
    }
}

impl<'de> Deserialize<'de> for Colour {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct ColourDe {
            #[serde(default)]
            r: Option<u8>,
            #[serde(default)]
            g: Option<u8>,
            #[serde(default)]
            b: Option<u8>,
            #[serde(default)]
            a: Option<u8>,
            #[serde(default)]
            model: Option<crate::colour::ColourModel>,
            #[serde(default)]
            values: Option<[f32; 4]>,
            #[serde(default)]
            alpha: Option<f32>,
        }
        let de = ColourDe::deserialize(deserializer)?;
        if let (Some(model), Some(values)) = (de.model, de.values) {
            if model != crate::colour::ColourModel::Rgb {
                let alpha = de.alpha.unwrap_or(1.0);
                return Ok(Self::from_managed(crate::colour::Colour { model, values, alpha }));
            }
        }
        Ok(Self::new(de.r.unwrap_or(0), de.g.unwrap_or(0), de.b.unwrap_or(0), de.a.unwrap_or(255)))
    }
}

fn preview_rgb(colour: &crate::colour::Colour) -> (u8, u8, u8) {
    let to_u8 = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    match colour.model {
        crate::colour::ColourModel::Rgb => (to_u8(colour.values[0]), to_u8(colour.values[1]), to_u8(colour.values[2])),
        crate::colour::ColourModel::Grey => {
            let value = to_u8(colour.values[0] / 100.0);
            (value, value, value)
        }
        crate::colour::ColourModel::Cmyk => {
            let c = colour.values[0] / 100.0;
            let m = colour.values[1] / 100.0;
            let y = colour.values[2] / 100.0;
            let k = colour.values[3] / 100.0;
            (to_u8((1.0 - c) * (1.0 - k)), to_u8((1.0 - m) * (1.0 - k)), to_u8((1.0 - y) * (1.0 - k)))
        }
        crate::colour::ColourModel::Lab => {
            let rgb = colour.clone().try_write_for_document(
                crate::colour::ColourFormat { model: crate::colour::ColourModel::Rgb, depth: crate::colour::BitDepth::U8 },
                &crate::colour::ColourPrefs::default(),
            ).map(|rgb| [rgb.values[0], rgb.values[1], rgb.values[2]]).ok().or_else(|| colour.lab_preview_srgb()).unwrap_or([0.5, 0.5, 0.5]);
            (to_u8(rgb[0]), to_u8(rgb[1]), to_u8(rgb[2]))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GradientKind {
    #[serde(alias="linear")]
    Linear,
    #[serde(alias="radial")]
    Radial,
    #[serde(alias="elliptical")]
    Elliptical,
    #[serde(alias="conical")]
    Conical,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    pub offset: f32,
    pub colour: Colour,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GradientSpread {
    #[default]
    Pad,
    Repeat,
    Reflect,
}

impl GradientSpread {
    pub fn is_pad(&self) -> bool {
        *self == Self::Pad
    }

    pub fn as_str(self) -> &'static str {
        match self { Self::Pad => "pad", Self::Repeat => "repeat", Self::Reflect => "reflect" }
    }

    pub fn map(self, position: f64) -> f32 {
        (match self {
            Self::Pad => position,
            Self::Repeat => position.rem_euclid(1.0),
            Self::Reflect => 1.0 - (1.0 - position.rem_euclid(2.0)).abs(),
        }) as f32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GradientGeom {
    pub start: [f64; 2],
    pub end: [f64; 2],
    pub ratio: f64,
    #[serde(default, skip_serializing_if = "GradientSpread::is_pad")]
    pub spread: GradientSpread,
    /// Focal point in units of the two ellipse radii, measured from its center.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<[f64; 2]>,
    /// Radius of the start circle, relative to the end circle.
    #[serde(default, skip_serializing_if = "is_zero_radius")]
    pub focus_radius: f64,
    /// The sweep's second axis, expressed in units along and perpendicular to its first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sweep_basis: Option<[f64; 2]>,
}

fn is_zero_radius(radius: &f64) -> bool { *radius == 0.0 }

impl Default for GradientGeom {
    fn default() -> Self {
        Self { start: [0.0, 0.0], end: [1.0, 0.0], ratio: 1.0, spread: GradientSpread::Pad, focus: None, focus_radius: 0.0, sweep_basis: None }
    }
}

impl GradientGeom {
    pub fn from_legacy_angle(angle: f64, bounds: [f64; 4]) -> Self {
        let (start, end) = crate::geometry::gradient_line_from_legacy_angle(angle, bounds);
        Self { start, end, ..Self::default() }
    }

    pub fn from_legacy_matrix(matrix: [f64; 6]) -> Self {
        let start = [matrix[4], matrix[5]];
        let end = [matrix[0] + matrix[4], matrix[1] + matrix[5]];
        let main = (matrix[0] * matrix[0] + matrix[1] * matrix[1]).sqrt();
        let side = (matrix[2] * matrix[2] + matrix[3] * matrix[3]).sqrt();
        let ratio = if main.abs() > 1e-14 { side / main } else { 1.0 };
        Self { start, end, ratio: if ratio.is_finite() { ratio } else { 1.0 }, ..Self::default() }
    }

    pub fn to_matrix(&self) -> [f64; 6] {
        let dx = self.end[0] - self.start[0];
        let dy = self.end[1] - self.start[1];
        let len = (dx * dx + dy * dy).sqrt();
        let (px, py) = if len > 1e-14 {
            (-dy / len * self.ratio * len, dx / len * self.ratio * len)
        } else {
            (0.0, 0.0)
        };
        [dx, dy, px, py, self.start[0], self.start[1]]
    }

    pub fn gradient_matrix(&self, kind: GradientKind) -> [f64; 6] {
        match kind {
            GradientKind::Conical => {
                let dx = self.end[0] - self.start[0];
                let dy = self.end[1] - self.start[1];
                let [along, across] = self.sweep_basis.unwrap_or([0.0, 1.0]);
                [dx, dy, along * dx - across * dy, along * dy + across * dx, self.start[0], self.start[1]]
            }
            GradientKind::Radial => Self { ratio: 1.0, ..*self }.to_matrix(),
            GradientKind::Elliptical | GradientKind::Linear => self.to_matrix(),
        }
    }

    pub fn transform(&self, matrix: [f64; 6]) -> Self {
        let transform = kurbo::Affine::new(matrix);
        let start = transform * kurbo::Point::new(self.start[0], self.start[1]);
        let end = transform * kurbo::Point::new(self.end[0], self.end[1]);
        Self { start: [start.x, start.y], end: [end.x, end.y], ..*self }
    }

    pub fn transform_for_kind(&self, kind: GradientKind, matrix: [f64; 6]) -> Self {
        let mut result = self.transform(matrix);
        if kind == GradientKind::Conical {
            let transform = kurbo::Affine::new(matrix);
            let [dx, dy, sx, sy, ..] = (transform * kurbo::Affine::new(self.gradient_matrix(kind))).as_coeffs();
            let length = dx.hypot(dy);
            let direction = [dx / length, dy / length];
            let along = direction[0] * (sx / length) + direction[1] * (sy / length);
            let across = direction[0] * (sy / length) - direction[1] * (sx / length);
            result.sweep_basis = if along.is_finite() && across.is_finite() && across.abs() > 1e-14
                && (along.abs() > 1e-12 || (across - 1.0).abs() > 1e-12)
            {
                Some([along, across])
            } else {
                None
            };
        }
        result
    }
}

#[derive(Default,Deserialize)]
#[serde(default)]
struct GradientGeometryWire {
    geom:Option<GradientGeom>,
    start:Option<[f64;2]>,
    end:Option<[f64;2]>,
    ratio:Option<f64>,
    matrix:Option<[f64;6]>,
    angle:Option<f64>,
    bounds:Option<[f64;4]>,
}
impl GradientGeometryWire {
    fn resolve(self)->GradientGeom {
        if let Some(geom)=self.geom {return geom;}
        let mut geom=if let (Some(start),Some(end))=(self.start,self.end) {GradientGeom{start,end,..GradientGeom::default()}}
        else if let Some(matrix)=self.matrix {GradientGeom::from_legacy_matrix(matrix)}
        else if let Some(angle)=self.angle {GradientGeom::from_legacy_angle(angle,self.bounds.unwrap_or([0.0,0.0,1.0,1.0]))}
        else {GradientGeom::default()};
        if let Some(ratio)=self.ratio {geom.ratio=ratio;}geom
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub enum Transparency {
    #[default]
    None,
    Gradient { kind: GradientKind, stops: Vec<GradientStop>, geom: GradientGeom },
    InverseBitmap(BitmapPaint),
}


impl Transparency {
    pub fn alpha_at(&self, local: [f64; 2]) -> f32 {
        match self {
            Self::None | Self::InverseBitmap(_) => 1.0,
            Self::Gradient { kind, stops, geom } => {
                let t = gradient_parameter(*kind, local[0], local[1], geom);
                f32::from(interpolate_stops(stops, t).a) / 255.0
            }
        }
    }
}

pub fn gradient_parameter(kind: GradientKind, x: f64, y: f64, geom: &GradientGeom) -> f32 {
    let dx=geom.end[0]-geom.start[0];let dy=geom.end[1]-geom.start[1];
    let len=dx.hypot(dy);let px=x-geom.start[0];let py=y-geom.start[1];
    if !len.is_finite() || len<=1e-14 || !px.is_finite() || !py.is_finite() {return 0.0;}
    let ux=dx/len;let uy=dy/len;
    let position = match kind {
        GradientKind::Linear => (px * ux + py * uy) / len,
        GradientKind::Radial | GradientKind::Elliptical => {
            let ratio = if kind == GradientKind::Elliptical && geom.ratio.is_finite() && geom.ratio.abs() > 1e-14 {
                geom.ratio
            } else { 1.0 };
            let u = (px * ux + py * uy) / len;
            let v = (-px * uy + py * ux) / (len * ratio);
            let [fx, fy] = geom.focus.unwrap_or([0.0; 2]);
            let inner = geom.focus_radius;
            let growth = 1.0 - inner;
            if fx == 0.0 && fy == 0.0 && growth > 0.0 {
                (u.hypot(v) - inner) / growth
            } else {
                // Intersect the sample with the expanding circle at stop position t.
                let qx = u - fx;
                let qy = v - fy;
                let a = growth * growth - fx * fx - fy * fy;
                let b = inner * growth - qx * fx - qy * fy;
                let c = inner * inner - qx * qx - qy * qy;
                if a > 0.0 {
                    let root = (b * b - a * c).max(0.0).sqrt();
                    if b > 0.0 { -c / (b + root) } else { (root - b) / a }
                } else { u.hypot(v) }
            }
        },
        GradientKind::Conical => {
            let angle = if let Some([along, across]) = geom.sweep_basis {
                if !along.is_finite() || !across.is_finite() || across.abs() <= 1e-14 { return 0.0; }
                let v = (-px * uy + py * ux) / across;
                let u = px * ux + py * uy - along * v;
                v.atan2(u)
            } else {
                py.atan2(px) - dy.atan2(dx)
            };
            return (angle / std::f64::consts::TAU).rem_euclid(1.0) as f32;
        }
    };
    geom.spread.map(position)
}

pub fn interpolate_stops(stops: &[GradientStop], t: f32) -> Colour {
    if stops.is_empty() {
        return Colour::new(0, 0, 0, 255);
    }
    if stops.len() == 1 || t <= stops[0].offset {
        return stops[0].colour;
    }
    for index in 0..stops.len() - 1 {
        let start = &stops[index];
        let end = &stops[index + 1];
        if t >= start.offset && t <= end.offset {
            let range = end.offset - start.offset;
            if range == 0.0 {
                return start.colour;
            }
            let ratio = (t - start.offset) / range;
            return lerp_colour(start.colour, end.colour, ratio);
        }
    }
    stops[stops.len() - 1].colour
}

pub(crate) fn lerp_colour(start: Colour, end: Colour, t: f32) -> Colour {
    let mix = |a: u8, b: u8| -> u8 {
        (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8
    };
    if start.model == end.model && start.model != crate::colour::ColourModel::Rgb {
        let mut values = [0.0; 4];
        for i in 0..4 {
            values[i] = start.values[i] + (end.values[i] - start.values[i]) * t;
        }
        let alpha = (start.a as f32 + (end.a as f32 - start.a as f32) * t) / 255.0;
        let mut colour = Colour::from_managed(crate::colour::Colour { model: start.model, values, alpha });
        colour.r = mix(start.r, end.r);
        colour.g = mix(start.g, end.g);
        colour.b = mix(start.b, end.b);
        colour
    } else {
        Colour::new(mix(start.r, end.r), mix(start.g, end.g), mix(start.b, end.b), mix(start.a, end.a))
    }
}

#[derive(Deserialize)]
enum TransparencyWire {
    #[serde(alias="none")]
    None,
    #[serde(alias="gradient")]
    Gradient {kind:GradientKind,stops:Vec<GradientStop>,#[serde(flatten)] geometry:GradientGeometryWire},
    InverseBitmap(BitmapPaint),
}
impl<'de> Deserialize<'de> for Transparency {
    fn deserialize<D:serde::Deserializer<'de>>(deserializer:D)->Result<Self,D::Error> {
        Ok(match TransparencyWire::deserialize(deserializer)? {
            TransparencyWire::None=>Self::None,
            TransparencyWire::InverseBitmap(bitmap)=>Self::InverseBitmap(bitmap),
            TransparencyWire::Gradient{kind,stops,geometry}=>Self::Gradient{kind,stops,geom:geometry.resolve()},
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HatchUnits {
    Document,
    Px,
    Pt,
    Pc,
    In,
    Ft,
    Yd,
    Mm,
    Cm,
    M,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HatchLine {
    pub angle_rad: f64,
    pub origin: [f64; 2],
    pub shift: f64,
    pub spacing: f64,
    pub dash: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hatch {
    pub lines: Vec<HatchLine>,
    pub colour: Colour,
    pub background: Option<Colour>,
    pub weight: f64,
    pub units: HatchUnits,
    pub matrix: [f64; 6],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BitmapPaint {
    pub resource_id: String,
    pub matrix: [f64; 6],
    pub seed: Option<u64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub single_image: bool,
}

impl BitmapPaint {
    /// PDF and SVG use repeating patterns. Keep the neighboring copies outside the painted area.
    pub fn tile_period(&self, size: [f64; 2], transform: kurbo::Affine, bounds: kurbo::Rect) -> [f64; 2] {
        if !self.single_image || !transform.is_finite() || transform.determinant().abs() < 1e-12 {
            return size;
        }
        let local = transform.inverse().transform_rect_bbox(bounds);
        if !local.is_finite() {
            return size;
        }
        [
            local.x1.max(size[0] - local.x0).max(size[0]).ceil() + 1.0,
            local.y1.max(size[1] - local.y0).max(size[1]).ceil() + 1.0,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatternPaint {
    pub resource_id: String,
    pub matrix: [f64; 6],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeshPaint {
    pub vertices: Vec<[f64; 2]>,
    pub colours: Vec<Colour>,
    pub indices: Vec<usize>,
    pub matrix: [f64; 6],
}

impl MeshPaint {
    pub fn new() -> Self {
        Self {
            vertices: Vec::new(),
            colours: Vec::new(),
            indices: Vec::new(),
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        }
    }

    pub fn add_vertex(&mut self, position: [f64; 2], colour: Colour) -> usize {
        let index = self.vertices.len();
        self.vertices.push(position);
        self.colours.push(colour);
        index
    }

    pub fn set_colour_at(&mut self, vertex_index: usize, colour: Colour) {
        if vertex_index < self.colours.len() {
            self.colours[vertex_index] = colour;
        }
    }

    pub fn add_triangle(&mut self, a: usize, b: usize, c: usize) {
        self.indices.push(a);
        self.indices.push(b);
        self.indices.push(c);
    }

    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

impl Default for MeshPaint {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiffusionCurveCreationMode {
    Manual,
    Automatic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffusionPaint {
    pub mode: DiffusionCurveCreationMode,
    pub seed: Option<u64>,
    pub matrix: [f64; 6],
}

impl DiffusionPaint {
    pub fn new(mode: DiffusionCurveCreationMode) -> Self {
        Self {
            mode,
            seed: None,
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        }
    }

    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Paint {
    None,
    Solid { colour: Colour },
    Gradient { kind: GradientKind, stops: Vec<GradientStop>, geom: GradientGeom },
    Hatch(Hatch),
    Bitmap(BitmapPaint),
    Pattern(PatternPaint),
    Mesh(MeshPaint),
    Diffusion(DiffusionPaint),
}

impl Default for Paint {
    fn default() -> Self {
        Paint::None
    }
}

#[derive(Serialize, Deserialize)]
struct PaintWire {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    colour: Option<Colour>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gradient_kind: Option<GradientKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stops: Option<Vec<GradientStop>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    geom: Option<GradientGeom>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    start: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    end: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    matrix: Option<[f64; 6]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    angle: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bounds: Option<[f64; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hatch: Option<Hatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bitmap: Option<BitmapPaint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pattern: Option<PatternPaint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mesh: Option<MeshPaint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    diffusion: Option<DiffusionPaint>,
}

impl Serialize for Paint {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let wire = paint_to_wire(self);
        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Paint {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = PaintWire::deserialize(deserializer)?;
        Ok(paint_from_wire(wire))
    }
}

fn paint_to_wire(paint: &Paint) -> PaintWire {
    match paint {
        Paint::None => PaintWire {
            kind: "none".to_string(),
            colour: None,
            gradient_kind: None,
            stops: None,
            geom: None,
            start: None,
            end: None,
            ratio: None,
            matrix: None,
            angle: None,
            bounds: None,
            hatch: None,
            bitmap: None,
            pattern: None,
            mesh: None,
            diffusion: None,
        },
        Paint::Solid { colour } => PaintWire {
            kind: "solid".to_string(),
            colour: Some(*colour),
            gradient_kind: None,
            stops: None,
            geom: None,
            start: None,
            end: None,
            ratio: None,
            matrix: None,
            angle: None,
            bounds: None,
            hatch: None,
            bitmap: None,
            pattern: None,
            mesh: None,
            diffusion: None,
        },
        Paint::Gradient { kind, stops, geom } => PaintWire {
            kind: "gradient".to_string(),
            colour: None,
            gradient_kind: Some(*kind),
            stops: Some(stops.clone()),
            geom: Some(*geom),
            start: None,
            end: None,
            ratio: None,
            matrix: None,
            angle: None,
            bounds: None,
            hatch: None,
            bitmap: None,
            pattern: None,
            mesh: None,
            diffusion: None,
        },
        Paint::Hatch(hatch) => PaintWire {
            kind: "hatch".to_string(),
            colour: None,
            gradient_kind: None,
            stops: None,
            geom: None,
            start: None,
            end: None,
            ratio: None,
            matrix: None,
            angle: None,
            bounds: None,
            hatch: Some(hatch.clone()),
            bitmap: None,
            pattern: None,
            mesh: None,
            diffusion: None,
        },
        Paint::Bitmap(bitmap) => PaintWire {
            kind: "bitmap".to_string(),
            colour: None,
            gradient_kind: None,
            stops: None,
            geom: None,
            start: None,
            end: None,
            ratio: None,
            matrix: None,
            angle: None,
            bounds: None,
            hatch: None,
            bitmap: Some(bitmap.clone()),
            pattern: None,
            mesh: None,
            diffusion: None,
        },
        Paint::Pattern(pattern) => PaintWire {
            kind: "pattern".to_string(),
            colour: None,
            gradient_kind: None,
            stops: None,
            geom: None,
            start: None,
            end: None,
            ratio: None,
            matrix: None,
            angle: None,
            bounds: None,
            hatch: None,
            bitmap: None,
            pattern: Some(pattern.clone()),
            mesh: None,
            diffusion: None,
        },
        Paint::Mesh(mesh) => PaintWire {
            kind: "mesh".to_string(),
            colour: None,
            gradient_kind: None,
            stops: None,
            geom: None,
            start: None,
            end: None,
            ratio: None,
            matrix: None,
            angle: None,
            bounds: None,
            hatch: None,
            bitmap: None,
            pattern: None,
            mesh: Some(mesh.clone()),
            diffusion: None,
        },
        Paint::Diffusion(diffusion) => PaintWire {
            kind: "diffusion".to_string(),
            colour: None,
            gradient_kind: None,
            stops: None,
            geom: None,
            start: None,
            end: None,
            ratio: None,
            matrix: None,
            angle: None,
            bounds: None,
            hatch: None,
            bitmap: None,
            pattern: None,
            mesh: None,
            diffusion: Some(diffusion.clone()),
        },
    }
}

fn paint_from_wire(wire: PaintWire) -> Paint {
    match wire.kind.as_str() {
        "none" => Paint::None,
        "solid" => Paint::Solid { colour: wire.colour.unwrap_or(Colour::new(0, 0, 0, 255)) },
        "gradient" => {
            let kind = wire.gradient_kind.unwrap_or(GradientKind::Linear);
            let stops = wire.stops.unwrap_or_default();
            let geom=GradientGeometryWire{geom:wire.geom,start:wire.start,end:wire.end,ratio:wire.ratio,matrix:wire.matrix,angle:wire.angle,bounds:wire.bounds}.resolve();
            Paint::Gradient { kind, stops, geom }
        }
        "hatch" => Paint::Hatch(wire.hatch.unwrap_or(Hatch {
            lines: Vec::new(),
            colour: Colour::new(0, 0, 0, 255),
            background: None,
            weight: 1.0,
            units: HatchUnits::Document,
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        })),
        "bitmap" => Paint::Bitmap(wire.bitmap.unwrap_or(BitmapPaint {
            resource_id: String::new(),
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            seed: None,
            single_image: false,
        })),
        "pattern" => Paint::Pattern(wire.pattern.unwrap_or(PatternPaint {
            resource_id: String::new(),
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        })),
        "mesh" => Paint::Mesh(wire.mesh.unwrap_or_else(MeshPaint::new)),
        "diffusion" => Paint::Diffusion(wire.diffusion.unwrap_or(DiffusionPaint {
            mode: DiffusionCurveCreationMode::Manual,
            seed: None,
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        })),
        _ => Paint::None,
    }
}

pub fn paint_to_json_value(paint: &Paint) -> serde_json::Value {
    serde_json::to_value(paint).unwrap_or(serde_json::Value::Null)
}

pub fn paint_from_json_value(value: &serde_json::Value) -> Paint {
    if let Some(s) = value.as_str() {
        return normalize_legacy_paint_alias(Some(s)).unwrap_or(Paint::None);
    }
    serde_json::from_value::<Paint>(value.clone()).unwrap_or(Paint::None)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineStyle {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

impl LineStyle {
    pub fn is_solid(&self) -> bool { *self == Self::Solid }

    pub fn preset_dash(self) -> &'static [f32] {
        match self {
            Self::Solid => &[],
            Self::Dashed => &[4.0, 2.0],
            Self::Dotted => &[1.0, 2.0],
        }
    }

    pub fn from_dash(dash: &[f32]) -> Self {
        match dash {
            [] => Self::Solid,
            [first, ..] if *first <= 1.0 => Self::Dotted,
            _ => Self::Dashed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Cap {
    Butt,
    Round,
    Square,
    RoundInset,
}

impl Cap {
    pub fn extends(self) -> bool { matches!(self, Self::Round | Self::Square) }

    pub fn is_round(self) -> bool { matches!(self, Self::Round | Self::RoundInset) }

    pub fn from_options(extends: bool, round: bool) -> Self {
        match (extends, round) {
            (false, false) => Self::Butt,
            (true, false) => Self::Square,
            (false, true) => Self::RoundInset,
            (true, true) => Self::Round,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Join {
    Miter { limit: f32 },
    Round,
    Bevel,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StrokeAlign {
    Inside,
    #[default]
    Centre,
    Outside,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Arrow {
    pub name: String,
    pub scale: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PaintOrder {
    StrokeOver,
    StrokeUnder,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrokeStyle {
    pub paint: Paint,
    pub width: f32,
    pub dash: Vec<f32>,
    #[serde(default)]
    pub dash_offset: f32,
    pub cap: Cap,
    pub join: Join,
    pub align: StrokeAlign,
    pub start_arrow: Option<Arrow>,
    pub end_arrow: Option<Arrow>,
    pub scale_with_object: bool,
    pub pressure: Vec<[f32; 2]>,
}

impl Default for StrokeStyle {
    fn default() -> Self {
        Self {
            paint: Paint::None,
            width: 0.0,
            dash: Vec::new(),
            dash_offset: 0.0,
            cap: Cap::Butt,
            join: Join::Miter { limit: 10.0 },
            align: StrokeAlign::Centre,
            start_arrow: None,
            end_arrow: None,
            scale_with_object: true,
            pressure: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableCell {
    pub fill: Paint,
    pub stroke: StrokeStyle,
}

impl TableCell {
    pub fn new() -> Self {
        Self {
            fill: Paint::None,
            stroke: StrokeStyle::default(),
        }
    }

    pub fn set_fill(&mut self, paint: Paint) {
        self.fill = paint;
    }

    pub fn set_stroke(&mut self, style: StrokeStyle) {
        self.stroke = style;
    }
}

impl Default for TableCell {
    fn default() -> Self {
        Self::new()
    }
}

fn matrix_is_finite(matrix: &[f64; 6]) -> bool {
    matrix.iter().all(|v| v.is_finite())
}

pub fn validate_paint(paint: &Paint) -> bool {
    match paint {
        Paint::None | Paint::Solid { .. } => true,
        Paint::Gradient { stops, geom, .. } => !stops.is_empty()
            && stops.iter().all(|stop| stop.offset.is_finite() && (0.0..=1.0).contains(&stop.offset))
            && geom.sweep_basis.is_none_or(|[along, across]| along.is_finite() && across.is_finite() && across.abs() > 1e-14),
        Paint::Hatch(hatch) => hatch.weight.is_finite() && hatch.weight >= 0.0 && matrix_is_finite(&hatch.matrix) && hatch.lines.iter().all(|line| line.spacing.is_finite() && line.spacing > 0.0 && line.angle_rad.is_finite()),
        Paint::Bitmap(bitmap) => !bitmap.resource_id.is_empty() && matrix_is_finite(&bitmap.matrix),
        Paint::Pattern(pattern) => !pattern.resource_id.is_empty() && matrix_is_finite(&pattern.matrix),
        Paint::Mesh(mesh) => mesh.vertices.len() == mesh.colours.len() && mesh.indices.len() % 3 == 0 && mesh.indices.iter().all(|i| *i < mesh.vertices.len()) && matrix_is_finite(&mesh.matrix),
        Paint::Diffusion(diffusion) => matrix_is_finite(&diffusion.matrix),
    }
}

pub fn validate_stroke(style: &StrokeStyle) -> bool {
    validate_paint(&style.paint) && style.width.is_finite() && style.width >= 0.0 && style.dash_offset.is_finite() && style.dash.iter().all(|d| d.is_finite() && *d >= 0.0)
}

impl TableCell {
    pub fn try_set_fill(&mut self, paint: Paint) -> Result<(), PaintParseError> {
        if !validate_paint(&paint) {
            return Err(PaintParseError::InvalidParams);
        }
        self.fill = paint;
        Ok(())
    }

    pub fn try_set_stroke(&mut self, style: StrokeStyle) -> Result<(), PaintParseError> {
        if !validate_stroke(&style) {
            return Err(PaintParseError::InvalidParams);
        }
        self.stroke = style;
        Ok(())
    }

    pub fn try_set(&mut self, fill: Paint, stroke: StrokeStyle) -> Result<(), PaintParseError> {
        if !validate_paint(&fill) || !validate_stroke(&stroke) {
            return Err(PaintParseError::InvalidParams);
        }
        self.fill = fill;
        self.stroke = stroke;
        Ok(())
    }

    pub fn from_cell(cell: &crate::table::model::TableCell) -> Self {
        let fill = cell.local.fill.clone().unwrap_or(Paint::None);
        let stroke = cell.local.edges.iter().flatten().next().map(edge_stroke_style).unwrap_or_default();
        Self { fill, stroke }
    }

    pub fn apply_to_cell(&self, cell: &mut crate::table::model::TableCell) -> Result<(), PaintParseError> {
        if !validate_paint(&self.fill) || !validate_stroke(&self.stroke) {
            return Err(PaintParseError::InvalidParams);
        }
        cell.local.fill = Some(self.fill.clone());
        let edge = if self.stroke.paint == Paint::None || self.stroke.width <= 0.0 {
            None
        } else {
            Some(crate::table::model::EdgeStyle {
                paint: self.stroke.paint.clone(),
                width_pt: self.stroke.width,
                line_style: LineStyle::from_dash(&self.stroke.dash),
                align: self.stroke.align,
            })
        };
        cell.local.edges = [edge.clone(), edge.clone(), edge.clone(), edge];
        cell.mask.0 |= crate::table::resolve::CELL_FILL | crate::table::resolve::CELL_EDGE_LEFT | crate::table::resolve::CELL_EDGE_TOP | crate::table::resolve::CELL_EDGE_RIGHT | crate::table::resolve::CELL_EDGE_BOTTOM;
        Ok(())
    }
}

pub fn set_cell_fill(cell: &mut crate::table::model::TableCell, paint: Paint) -> Result<(), PaintParseError> {
    let mut owner = TableCell::from_cell(cell);
    owner.try_set_fill(paint)?;
    cell.local.fill = Some(owner.fill);
    cell.mask.0 |= crate::table::resolve::CELL_FILL;
    Ok(())
}

pub fn set_cell_stroke(cell: &mut crate::table::model::TableCell, style: StrokeStyle) -> Result<(), PaintParseError> {
    let mut owner = TableCell::from_cell(cell);
    owner.try_set_stroke(style)?;
    let fill = cell.local.fill.clone();
    let mask = cell.mask.0;
    owner.apply_to_cell(cell)?;
    cell.local.fill = fill;
    cell.mask.0 = (cell.mask.0 & !crate::table::resolve::CELL_FILL) | (mask & crate::table::resolve::CELL_FILL);
    Ok(())
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextPaint {
    pub fill: Paint,
    pub stroke: Option<StrokeStyle>,
}

impl TextPaint {
    pub fn new() -> Self {
        Self {
            fill: Paint::None,
            stroke: None,
        }
    }

    pub fn set_fill(&mut self, paint: Paint) {
        self.fill = paint;
    }

    pub fn set_stroke(&mut self, style: Option<StrokeStyle>) {
        self.stroke = style;
    }
}

impl Default for TextPaint {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParagraphPaint {
    pub fill: Option<Paint>,
    pub stroke: Option<StrokeStyle>,
}

impl ParagraphPaint {
    pub fn new() -> Self {
        Self {
            fill: None,
            stroke: None,
        }
    }

    pub fn set_fill(&mut self, paint: Paint) {
        self.fill = Some(paint);
    }

    pub fn reset_fill(&mut self) {
        self.fill = None;
    }

    pub fn set_stroke(&mut self, style: StrokeStyle) {
        self.stroke = Some(style);
    }

    pub fn reset_stroke(&mut self) {
        self.stroke = None;
    }
}

impl Default for ParagraphPaint {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CharacterPaint {
    pub fill: Option<Paint>,
    pub stroke: Option<StrokeStyle>,
}

impl CharacterPaint {
    pub fn new() -> Self {
        Self {
            fill: None,
            stroke: None,
        }
    }

    pub fn set_fill(&mut self, paint: Paint) {
        self.fill = Some(paint);
    }

    pub fn reset_fill(&mut self) {
        self.fill = None;
    }

    pub fn set_stroke(&mut self, style: StrokeStyle) {
        self.stroke = Some(style);
    }

    pub fn reset_stroke(&mut self) {
        self.stroke = None;
    }
}

impl Default for CharacterPaint {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunPaint {
    pub fill: Option<Paint>,
    pub stroke: Option<StrokeStyle>,
}

impl RunPaint {
    pub fn new() -> Self {
        Self {
            fill: None,
            stroke: None,
        }
    }

    pub fn set_fill(&mut self, paint: Paint) {
        self.fill = Some(paint);
    }

    pub fn reset_fill(&mut self) {
        self.fill = None;
    }

    pub fn set_stroke(&mut self, style: StrokeStyle) {
        self.stroke = Some(style);
    }

    pub fn reset_stroke(&mut self) {
        self.stroke = None;
    }
}

impl Default for RunPaint {
    fn default() -> Self {
        Self::new()
    }
}

pub fn resolve_run_paint(
    run: &RunPaint,
    character: &CharacterPaint,
    paragraph: &ParagraphPaint,
) -> TextPaint {
    let fill = run.fill.clone()
        .or_else(|| character.fill.clone())
        .or_else(|| paragraph.fill.clone())
        .unwrap_or(Paint::None);

    let stroke = run.stroke.clone()
        .or_else(|| character.stroke.clone())
        .or_else(|| paragraph.stroke.clone());

    TextPaint { fill, stroke }
}

pub enum PaintParseError {
    InvalidParams,
}

pub fn normalize_legacy_paint_alias(input: Option<&str>) -> Result<Paint, PaintParseError> {
    match input {
        None => Ok(Paint::None),
        Some(s) if s.is_empty() => Ok(Paint::None),
        Some(s) => {
            let hex = s.trim_start_matches('#');
            if hex.len() == 6 || hex.len() == 8 {
                match parse_hex_color(hex) {
                    Some(colour) => Ok(Paint::Solid { colour }),
                    None => Err(PaintParseError::InvalidParams),
                }
            } else {
                Err(PaintParseError::InvalidParams)
            }
        }
    }
}

fn parse_hex_color(hex: &str) -> Option<Colour> {
    if hex.len() != 6 && hex.len() != 8 {
        return None;
    }

    let mut bytes = [0u8; 4];
    for i in 0..hex.len() / 2 {
        let byte_str = &hex[i * 2..i * 2 + 2];
        if !byte_str.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        bytes[i] = u8::from_str_radix(byte_str, 16).ok()?;
    }

    Some(Colour::new(
        bytes[0],
        bytes[1],
        bytes[2],
        if hex.len() == 8 { bytes[3] } else { 255 },
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaintRole {
    SourceObject,
    PictureFrame,
    Cell,
    Edge,
    Glyph,
}

pub fn project_display_paint(
    role: PaintRole,
    source_fill: Option<&Paint>,
    frame_fill: Option<&Paint>,
    cell_fill: Option<&Paint>,
    edge_stroke: Option<&StrokeStyle>,
    glyph_fill: Option<&Paint>,
) -> Paint {
    match role {
        PaintRole::SourceObject => source_fill.cloned().unwrap_or(Paint::None),
        PaintRole::PictureFrame => frame_fill.cloned().unwrap_or(Paint::None),
        PaintRole::Cell => cell_fill.cloned().unwrap_or(Paint::None),
        PaintRole::Edge => edge_stroke.map(|s| s.paint.clone()).unwrap_or(Paint::None),
        PaintRole::Glyph => glyph_fill.cloned().unwrap_or(Paint::None),
    }
}

impl PaintRole {
    pub fn from_display_role(role: &str) -> PaintRole {
        match role {
            "TableFrame" | "TableHeader" | "TableFooter" | "PictureFrame" => PaintRole::PictureFrame,
            "TableCell" => PaintRole::Cell,
            "TableEdge" => PaintRole::Edge,
            "TableGlyph" | "FrameText" | "ArtText" | "PathText" => PaintRole::Glyph,
            _ => PaintRole::SourceObject,
        }
    }
}

pub fn edge_stroke_style(edge: &crate::table::model::EdgeStyle) -> StrokeStyle {
    StrokeStyle {
        paint: edge.paint.clone(),
        width: edge.width_pt,
        dash: edge.line_style.preset_dash().to_vec(),
        align: edge.align,
        ..StrokeStyle::default()
    }
}

pub fn project_object_paint(role: PaintRole, object: &crate::object::Object) -> Paint {
    match role {
        PaintRole::SourceObject => object.fill.clone(),
        PaintRole::Edge => object.stroke.paint.clone(),
        _ => Paint::None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DisplayColourCtx {
    pub cmyk: String,
    pub grey: String,
    pub intent: crate::colour::RenderingIntent,
    pub bpc: bool,
}

impl DisplayColourCtx {
    pub fn for_document(document: &crate::document::Document) -> Self {
        Self::for_optional(Some(document))
    }

    pub fn app_default() -> Self {
        Self::for_optional(None)
    }

    pub fn for_optional(document: Option<&crate::document::Document>) -> Self {
        let prefs = crate::colour::services().read().map(|services| services.preferences.clone()).unwrap_or_default();
        let doc_cmyk = document.filter(|document| document.colour.format.model == crate::colour::ColourModel::Cmyk && !document.colour.profile.is_empty()).map(|document| document.colour.profile.clone());
        let app_cmyk = crate::colour::ColourFormat { model: crate::colour::ColourModel::Cmyk, depth: crate::colour::BitDepth::U8 }.default_profile(&prefs);
        let grey = if prefs.grey.is_empty() { crate::colour::ProfileStore::GREY_D50.to_string() } else { prefs.grey.clone() };
        Self { cmyk: doc_cmyk.unwrap_or(app_cmyk), grey, intent: prefs.intent, bpc: prefs.bpc }
    }
}

type IccDisplayKey = (String, crate::colour::ColourModel, crate::colour::RenderingIntent, bool, [u32; 4]);

fn icc_display_cache() -> &'static std::sync::Mutex<std::collections::HashMap<IccDisplayKey, Option<[u8; 3]>>> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<IccDisplayKey, Option<[u8; 3]>>>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

const ICC_DISPLAY_CACHE_MAX: usize = 4096;

pub fn icc_display_cache_len() -> usize {
    icc_display_cache().lock().map(|cache| cache.len()).unwrap_or(0)
}

pub fn display_colour(colour: &Colour, icc: Option<&crate::colour::model::ObjectIcc>, ctx: &DisplayColourCtx) -> Colour {
    let model = colour.model;
    let object_profile = icc.and_then(|icc| icc.profile.as_deref());
    let Ok(services) = crate::colour::services().read() else { return *colour };
    let source = match object_profile.and_then(|name| services.profiles.find(name)) {
        Some(profile) if profile.model == model => profile.name.clone(),
        Some(_) if model == crate::colour::ColourModel::Rgb => return *colour,
        _ => match model {
            crate::colour::ColourModel::Rgb => return *colour,
            crate::colour::ColourModel::Cmyk => ctx.cmyk.clone(),
            crate::colour::ColourModel::Grey if icc.is_some_and(|icc| icc.profile.is_some() || icc.intent.is_some()) => ctx.grey.clone(),
            crate::colour::ColourModel::Lab if icc.is_some_and(|icc| icc.profile.is_some() || icc.intent.is_some()) => crate::colour::ProfileStore::LAB_D50.to_string(),
            _ => return *colour,
        },
    };
    if model == crate::colour::ColourModel::Rgb && services.profiles.canonical(&source) == crate::colour::ProfileStore::SRGB {
        return *colour;
    }
    let intent = icc.map(|icc| icc.effective_intent(ctx.intent)).unwrap_or(ctx.intent);
    let managed = colour.managed();
    let key = (source.clone(), model, intent, ctx.bpc, managed.values.map(f32::to_bits));
    let cached = icc_display_cache().lock().ok().and_then(|cache| cache.get(&key).copied());
    let rgb = match cached {
        Some(rgb) => rgb,
        None => {
            let convert_ctx = crate::colour::ConvertCtx { from: source, to: crate::colour::ProfileStore::SRGB.to_string(), intent, bpc: ctx.bpc };
            let rgb = crate::colour::convert::convert(&managed, crate::colour::ColourModel::Rgb, &convert_ctx, &services.profiles)
                .ok()
                .map(|out| [out.values[0], out.values[1], out.values[2]].map(|value| (value.clamp(0.0, 1.0) * 255.0).round() as u8));
            if let Ok(mut cache) = icc_display_cache().lock() {
                if cache.len() >= ICC_DISPLAY_CACHE_MAX {
                    cache.clear();
                }
                cache.insert(key, rgb);
            }
            rgb
        }
    };
    match rgb {
        Some([r, g, b]) => Colour { r, g, b, ..*colour },
        None => *colour,
    }
}

pub fn display_paint(paint: &Paint, icc: Option<&crate::colour::model::ObjectIcc>, ctx: &DisplayColourCtx) -> Paint {
    let convert = |colour: &Colour| display_colour(colour, icc, ctx);
    match paint {
        Paint::Solid { colour } => Paint::Solid { colour: convert(colour) },
        Paint::Gradient { kind, stops, geom } => Paint::Gradient { kind: *kind, stops: stops.iter().map(|stop| GradientStop { colour: convert(&stop.colour), ..stop.clone() }).collect(), geom: geom.clone() },
        Paint::Hatch(hatch) => Paint::Hatch(Hatch { colour: convert(&hatch.colour), background: hatch.background.as_ref().map(convert), ..hatch.clone() }),
        Paint::Mesh(mesh) => Paint::Mesh(MeshPaint { colours: mesh.colours.iter().map(convert).collect(), ..mesh.clone() }),
        other => other.clone(),
    }
}

pub fn display_scene(document: &crate::document::Document, scene: &mut crate::scene::Scene) {
    let ctx = DisplayColourCtx::for_document(document);
    let icc_of = |id: crate::object::ObjectId| document.get_object(id).and_then(|object| object.icc.clone());
    for prim in &mut scene.prims {
        match prim {
            crate::scene::Prim::Fill { object, paint, .. } | crate::scene::Prim::Stroke { object, paint, .. } | crate::scene::Prim::Label { object, paint, .. } => {
                *paint = display_paint(paint, icc_of(*object).as_ref(), &ctx);
            }
            crate::scene::Prim::Text { object, lines, .. } => {
                let icc = icc_of(*object);
                for line in lines.iter_mut() {
                    line.paint = display_paint(&line.paint, icc.as_ref(), &ctx);
                }
            }
            _ => {}
        }
    }
}

pub fn project_table_paint(
    role: PaintRole,
    frame: Option<&crate::table::model::TableFrameFormat>,
    cell: Option<&crate::table::model::CellFormat>,
    edge: Option<&crate::table::model::EdgeStyle>,
    glyph_fill: Option<&Paint>,
) -> Paint {
    let edge_stroke = edge.map(edge_stroke_style);
    project_display_paint(role, None, frame.and_then(|f| f.fill.as_ref()), cell.and_then(|c| c.fill.as_ref()), edge_stroke.as_ref(), glyph_fill)
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PaintResourceKind {
    Bitmap,
    Pattern,
    HatchSwatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PaintResourceRef {
    pub kind: PaintResourceKind,
    pub id: String,
}

pub trait PaintVisitor {
    fn visit_paint(&mut self, paint: &Paint);
    fn visit_stroke(&mut self, stroke: &StrokeStyle) {
        self.visit_paint(&stroke.paint);
    }
    fn visit_cell(&mut self, cell: &TableCell) {
        self.visit_paint(&cell.fill);
        self.visit_stroke(&cell.stroke);
    }
    fn visit_text(&mut self, text: &TextPaint) {
        self.visit_paint(&text.fill);
        if let Some(ref s) = text.stroke {
            self.visit_stroke(s);
        }
    }
}

pub struct ResourceCollector {
    pub resources: Vec<PaintResourceRef>,
}

impl ResourceCollector {
    pub fn new() -> Self {
        Self { resources: Vec::new() }
    }

    pub fn collect(&mut self, paint: &Paint) {
        match paint {
            Paint::Bitmap(b) => {
                self.resources.push(PaintResourceRef {
                    kind: PaintResourceKind::Bitmap,
                    id: b.resource_id.clone(),
                });
            }
            Paint::Pattern(p) => {
                self.resources.push(PaintResourceRef {
                    kind: PaintResourceKind::Pattern,
                    id: p.resource_id.clone(),
                });
            }
            Paint::Hatch(h) => {
                let id = hatch_resource_id(h);
                self.resources.push(PaintResourceRef {
                    kind: PaintResourceKind::HatchSwatch,
                    id,
                });
            }
            _ => {}
        }
    }

    pub fn finish(self) -> Vec<PaintResourceRef> {
        let mut seen = std::collections::HashSet::new();
        self.resources.into_iter().filter(|r| seen.insert(r.clone())).collect()
    }
}

impl PaintVisitor for ResourceCollector {
    fn visit_paint(&mut self, paint: &Paint) {
        self.collect(paint);
    }
}

fn hatch_resource_id(hatch: &Hatch) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(hatch).unwrap_or_default().hash(&mut hasher);
    format!("hatch_{:016x}", hasher.finish())
}

fn visit_edges(collector: &mut ResourceCollector, edges: &[Option<crate::table::model::EdgeStyle>; 4]) {
    for edge in edges.iter().flatten() {
        collector.visit_paint(&edge.paint);
    }
}

fn visit_frame_format(collector: &mut ResourceCollector, frame: &crate::table::model::TableFrameFormat) {
    if let Some(fill) = &frame.fill {
        collector.visit_paint(fill);
    }
    visit_edges(collector, &frame.edges);
}

fn visit_cell_format(collector: &mut ResourceCollector, cell: &crate::table::model::CellFormat) {
    if let Some(fill) = &cell.fill {
        collector.visit_paint(fill);
    }
    visit_edges(collector, &cell.edges);
}

const TEXT_PAINT_KEYS: [&str; 5] = ["fill", "background", "highlight", "stroke", "outline"];

fn visit_text_value(collector: &mut ResourceCollector, value: &serde_json::Value) {
    let Some(map) = value.as_object() else {
        return;
    };
    for key in TEXT_PAINT_KEYS {
        if let Some(paint) = map.get(key).and_then(paint_from_legacy_value) {
            collector.visit_paint(&paint);
        }
    }
    for key in ["style", "char_style", "character"] {
        if let Some(inner) = map.get(key) {
            visit_text_value(collector, inner);
        }
    }
}

pub(crate) fn visit_vector_resources(collector: &mut ResourceCollector, object: &crate::Object) {
    for fill in std::iter::once(&object.fill).chain(&object.additional_fills) {
        collector.visit_paint(fill);
    }
    for stroke in std::iter::once(&object.stroke).chain(&object.additional_strokes) {
        collector.visit_stroke(stroke);
    }
    if let Transparency::InverseBitmap(bitmap) = &object.transparency {
        collector.resources.push(PaintResourceRef { kind: PaintResourceKind::Bitmap, id: bitmap.resource_id.clone() });
    }
    if let Some(texture) = &object.path_brush {
        collector.resources.push(PaintResourceRef { kind: PaintResourceKind::Bitmap, id: texture.resource_id.clone() });
    }
    if let crate::object::ObjectKind::Compound(compound) = &object.kind {
        for operand in &compound.operands {
            visit_vector_resources(collector, operand);
        }
    }
}

pub(crate) fn visit_object_resources(collector: &mut ResourceCollector, object: &crate::Object) {
    visit_vector_resources(collector, object);
    if let crate::object::ObjectKind::Table(table) = &object.kind {
        visit_frame_format(collector, &table.frame_local);
        for cell in &table.cells {
            visit_cell_format(collector, &cell.local);
        }
    }
}

pub fn collect_document_resources(doc: &crate::document::Document) -> Vec<PaintResourceRef> {
    let mut collector = ResourceCollector::new();
    for object in &doc.objects {
        visit_object_resources(&mut collector, object);
    }
    for format in &doc.table_formats {
        visit_frame_format(&mut collector, &format.frame);
        for (_, _, cell) in &format.cell_formats {
            visit_cell_format(&mut collector, cell);
        }
    }
    for story in &doc.stories {
        for value in story.ranges.iter().chain(story.paras.iter()) {
            visit_text_value(&mut collector, value);
        }
    }

    collector.finish()
}

pub fn clone_paint(paint: &Paint) -> Paint {
    paint.clone()
}

pub fn clone_stroke_style(style: &StrokeStyle) -> StrokeStyle {
    style.clone()
}

pub fn transform_paint_matrix(paint: &mut Paint, transform: [f64; 6]) {
    match paint {
        Paint::Gradient { kind, geom, .. } => {
            *geom = geom.transform_for_kind(*kind, transform);
        }
        Paint::Hatch(hatch) => {
            hatch.matrix = multiply_matrices(transform, hatch.matrix);
        }
        Paint::Bitmap(bitmap) => {
            bitmap.matrix = multiply_matrices(transform, bitmap.matrix);
        }
        Paint::Pattern(pattern) => {
            pattern.matrix = multiply_matrices(transform, pattern.matrix);
        }
        Paint::Mesh(mesh) => {
            mesh.matrix = multiply_matrices(transform, mesh.matrix);
        }
        Paint::Diffusion(diffusion) => {
            diffusion.matrix = multiply_matrices(transform, diffusion.matrix);
        }
        _ => {}
    }
}

pub fn transform_stroke_style_matrix(style: &mut StrokeStyle, transform: [f64; 6]) {
    transform_paint_matrix(&mut style.paint, transform);
}

pub(crate) fn transform_transparency_matrix(transparency: &mut Transparency, transform: [f64; 6]) {
    match transparency {
        Transparency::None => {}
        Transparency::Gradient { kind, geom, .. } => *geom = geom.transform_for_kind(*kind, transform),
        Transparency::InverseBitmap(bitmap) => bitmap.matrix = multiply_matrices(transform, bitmap.matrix),
    }
}

fn multiply_matrices(a: [f64; 6], b: [f64; 6]) -> [f64; 6] {
    let a0 = a[0]; let a1 = a[1]; let a2 = a[2]; let a3 = a[3]; let a4 = a[4]; let a5 = a[5];
    let b0 = b[0]; let b1 = b[1]; let b2 = b[2]; let b3 = b[3]; let b4 = b[4]; let b5 = b[5];

    [
        a0 * b0 + a2 * b1,
        a1 * b0 + a3 * b1,
        a0 * b2 + a2 * b3,
        a1 * b2 + a3 * b3,
        a0 * b4 + a2 * b5 + a4,
        a1 * b4 + a3 * b5 + a5,
    ]
}

pub fn apply_transform_to_paint(paint: &mut Paint, scale_x: f64, scale_y: f64, rotate_rad: f64) {
    let cos = rotate_rad.cos();
    let sin = rotate_rad.sin();

    let transform = [
        scale_x * cos,
        scale_x * sin,
        -scale_y * sin,
        scale_y * cos,
        0.0,
        0.0,
    ];

    transform_paint_matrix(paint, transform);
}

pub fn apply_transform_to_stroke(style: &mut StrokeStyle, scale_x: f64, scale_y: f64, rotate_rad: f64) {
    let cos = rotate_rad.cos();
    let sin = rotate_rad.sin();

    let transform = [
        scale_x * cos,
        scale_x * sin,
        -scale_y * sin,
        scale_y * cos,
        0.0,
        0.0,
    ];

    transform_stroke_style_matrix(style, transform);
}

pub fn copy_paint(source: &Paint) -> Paint {
    source.clone()
}

pub fn copy_stroke(source: &StrokeStyle) -> StrokeStyle {
    source.clone()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HatchSegment {
    pub line_index: usize,
    pub replica_index: i64,
    pub start: [f64; 2],
    pub end: [f64; 2],
}

pub const MAX_HATCH_GEOMETRY_STEPS: usize = 1_000_000;

fn hatch_error() -> dtool_protocol::CommandError {
    dtool_protocol::CommandError::invalid_params()
}

fn hatch_point(matrix: [f64; 6], point: [f64; 2]) -> [f64; 2] {
    [matrix[0] * point[0] + matrix[2] * point[1] + matrix[4], matrix[1] * point[0] + matrix[3] * point[1] + matrix[5]]
}

fn hatch_cross(a: [f64; 2], b: [f64; 2]) -> f64 { a[0] * b[1] - a[1] * b[0] }

impl Hatch {
    pub fn clipped_segments(&self, outline: &[[f64; 2]]) -> Result<Vec<HatchSegment>, dtool_protocol::CommandError> {
        if outline.len() < 3 || !self.matrix.iter().all(|n| n.is_finite()) || !outline.iter().flatten().all(|n| n.is_finite()) {
            return Err(hatch_error());
        }
        let m = self.matrix;
        let det = m[0] * m[3] - m[1] * m[2];
        if !det.is_finite() || det == 0.0 { return Err(hatch_error()); }
        let inverse = [m[3] / det, -m[1] / det, -m[2] / det, m[0] / det, (m[2] * m[5] - m[3] * m[4]) / det, (m[1] * m[4] - m[0] * m[5]) / det];
        let polygon: Vec<_> = outline.iter().map(|point| hatch_point(inverse, *point)).collect();
        if !polygon.iter().flatten().all(|value| value.is_finite()) { return Err(hatch_error()); }
        let mut result = Vec::new();
        let mut work = 0usize;
        for (line_index, line) in self.lines.iter().enumerate() {
            if !line.angle_rad.is_finite() || !line.origin.iter().all(|n| n.is_finite()) || !line.shift.is_finite() || !line.spacing.is_finite() || line.spacing <= 0.0 || !line.dash.iter().all(|n| n.is_finite()) { return Err(hatch_error()); }
            let direction = [line.angle_rad.cos(), line.angle_rad.sin()];
            let normal = [-direction[1], direction[0]];
            let projections: Vec<_> = polygon.iter().map(|point| ((point[0] - line.origin[0]) * normal[0] + (point[1] - line.origin[1]) * normal[1]) / line.spacing).collect();
            let first = projections.iter().copied().fold(f64::INFINITY, f64::min).ceil();
            let last = projections.iter().copied().fold(f64::NEG_INFINITY, f64::max).floor();
            if first > last { continue; }
            let replicas = last - first + 1.0;
            if !replicas.is_finite() || first <= i64::MIN as f64 || last >= i64::MAX as f64 || replicas > MAX_HATCH_GEOMETRY_STEPS as f64 || replicas * polygon.len() as f64 > MAX_HATCH_GEOMETRY_STEPS as f64 {
                return Err(dtool_protocol::CommandError::new("REQUEST_TOO_LARGE"));
            }
            for replica_index in first as i64..=last as i64 {
                let replica = replica_index as f64;
                let origin = [line.origin[0] + replica * (normal[0] * line.spacing + direction[0] * line.shift), line.origin[1] + replica * (normal[1] * line.spacing + direction[1] * line.shift)];
                let mut intersections = Vec::new();
                for index in 0..polygon.len() {
                    work += 1;
                    if work > MAX_HATCH_GEOMETRY_STEPS { return Err(dtool_protocol::CommandError::new("REQUEST_TOO_LARGE")); }
                    let a = polygon[index]; let b = polygon[(index + 1) % polygon.len()];
                    let edge = [b[0] - a[0], b[1] - a[1]];
                    let delta = [a[0] - origin[0], a[1] - origin[1]];
                    let denominator = hatch_cross(direction, edge);
                    if denominator.abs() <= f64::EPSILON * edge[0].abs().max(edge[1].abs()).max(1.0) { continue; }
                    let parameter = hatch_cross(delta, direction) / denominator;
                    if (0.0..1.0).contains(&parameter) {
                        let along = hatch_cross(delta, edge) / denominator;
                        if along.is_finite() { intersections.push(along); }
                    }
                }
                intersections.sort_by(f64::total_cmp);
                for pair in intersections.chunks_exact(2) {
                    let low = pair[0]; let high = pair[1];
                    if high <= low { continue; }
                    let mut add = |a: f64, b: f64| -> Result<(), dtool_protocol::CommandError> {
                        if result.len() >= MAX_HATCH_GEOMETRY_STEPS { return Err(dtool_protocol::CommandError::new("REQUEST_TOO_LARGE")); }
                        let start = hatch_point(m, [origin[0] + direction[0] * a, origin[1] + direction[1] * a]);
                        let end = hatch_point(m, [origin[0] + direction[0] * b, origin[1] + direction[1] * b]);
                        if !start.iter().chain(end.iter()).all(|v| v.is_finite()) { return Err(hatch_error()); }
                        result.push(HatchSegment { line_index, replica_index, start, end });
                        Ok(())
                    };
                    if line.dash.is_empty() { add(low, high)?; continue; }
                    let period: f64 = line.dash.iter().map(|length| length.abs()).sum();
                    if !period.is_finite() || period <= 0.0 { return Err(hatch_error()); }
                    let start_cycle = (low / period).floor();
                    let end_cycle = (high / period).ceil();
                    if start_cycle <= i64::MIN as f64 || end_cycle >= i64::MAX as f64 || (end_cycle - start_cycle + 1.0) * line.dash.len() as f64 > MAX_HATCH_GEOMETRY_STEPS as f64 { return Err(dtool_protocol::CommandError::new("REQUEST_TOO_LARGE")); }
                    let signed = line.dash.iter().any(|length| *length < 0.0);
                    for cycle in start_cycle as i64..=end_cycle as i64 {
                        let mut cursor = cycle as f64 * period;
                        for (index, length) in line.dash.iter().enumerate() {
                            work += 1;
                            if work > MAX_HATCH_GEOMETRY_STEPS { return Err(dtool_protocol::CommandError::new("REQUEST_TOO_LARGE")); }
                            let next = cursor + length.abs();
                            let ink = if signed { *length >= 0.0 } else { index % 2 == 0 };
                            if ink && *length == 0.0 && cursor >= low && cursor <= high { add(cursor, cursor)?; }
                            else if ink && next.min(high) > cursor.max(low) { add(cursor.max(low), next.min(high))?; }
                            cursor = next;
                        }
                    }
                }
            }
        }
        Ok(result)
    }
}

pub const HATCH_LINES_PROPERTY: &str = "Hatch.lines";

#[cfg(test)]
mod gradient_wire_contracts {
    use super::*;
    use serde_json::json;
    #[test]
    fn angle_fill_and_transparency_promote_to_identical_points_and_roundtrip() {
        for angle in [0.0,45.0,90.0] {
            let expected=GradientGeom::from_legacy_angle(angle,[0.0,0.0,100.0,50.0]);
            let paint:Paint=serde_json::from_value(json!({"type":"gradient","gradient_kind":"Linear","stops":[],"angle":angle,"bounds":[0,0,100,50]})).unwrap();
            assert!(matches!(&paint,Paint::Gradient{geom,..} if *geom==expected));
            let stored=serde_json::to_value(&paint).unwrap();assert_eq!(stored["geom"],serde_json::to_value(expected).unwrap());
            for key in ["angle","matrix","start","end","bounds"] {assert!(stored.get(key).is_none());}
            assert_eq!(serde_json::from_value::<Paint>(stored).unwrap(),paint);
            let transparency:Transparency=serde_json::from_value(json!({"Gradient":{"kind":"Linear","stops":[],"angle":angle,"bounds":[0,0,100,50]}})).unwrap();
            assert!(matches!(&transparency,Transparency::Gradient{geom,..} if *geom==expected));
            let stored=serde_json::to_value(&transparency).unwrap();assert!(stored["Gradient"].get("angle").is_none());
            assert_eq!(serde_json::from_value::<Transparency>(stored).unwrap(),transparency);
        }
    }
    #[test]
    fn explicit_geometry_wins_and_legacy_ratio_is_shared() {
        let value=json!({"type":"gradient","geom":{"start":[3,4],"end":[9,12],"ratio":0.25},"angle":90,"bounds":[0,0,100,50],"ratio":2});
        let Paint::Gradient{geom,..}=serde_json::from_value::<Paint>(value).unwrap() else {panic!("not gradient");};
        assert_eq!(geom,GradientGeom{start:[3.0,4.0],end:[9.0,12.0],ratio:0.25, ..GradientGeom::default() });
        let value=json!({"Gradient":{"kind":"elliptical","stops":[],"angle":0,"bounds":[0,0,100,50],"ratio":0.5}});
        let Transparency::Gradient{geom,..}=serde_json::from_value::<Transparency>(value).unwrap() else {panic!("not gradient");};
        assert_eq!(geom,GradientGeom{start:[0.0,25.0],end:[100.0,25.0],ratio:0.5, ..GradientGeom::default() });
    }
    #[test]
    fn managed_stop_values_survive_geometry_promotion() {
        for model in [crate::colour::ColourModel::Cmyk,crate::colour::ColourModel::Lab,crate::colour::ColourModel::Grey] {
            let colour=Colour::from_managed(crate::colour::Colour{model,values:[0.25,0.5,0.125,0.75],alpha:0.5});
            let stop=GradientStop{offset:0.0,colour};
            let value=json!({"type":"gradient","gradient_kind":"linear","stops":[stop],"angle":45,"bounds":[0,0,100,50]});
            let paint:Paint=serde_json::from_value(value).unwrap();let stored=serde_json::to_value(&paint).unwrap();
            let Paint::Gradient{stops,..}=serde_json::from_value::<Paint>(stored).unwrap() else {panic!("not gradient");};
            assert_eq!(stops[0].colour,colour);assert_eq!(stops[0].colour.model,model);assert_eq!(stops[0].colour.values,colour.values);
        }
    }
}

pub const LEGACY_COLOUR_NAMES: [(&str, [u8; 3]); 18] = [
    ("black", [0, 0, 0]),
    ("white", [255, 255, 255]),
    ("red", [255, 0, 0]),
    ("lime", [0, 255, 0]),
    ("green", [0, 128, 0]),
    ("blue", [0, 0, 255]),
    ("yellow", [255, 255, 0]),
    ("cyan", [0, 255, 255]),
    ("aqua", [0, 255, 255]),
    ("magenta", [255, 0, 255]),
    ("fuchsia", [255, 0, 255]),
    ("gray", [128, 128, 128]),
    ("grey", [128, 128, 128]),
    ("silver", [192, 192, 192]),
    ("maroon", [128, 0, 0]),
    ("navy", [0, 0, 128]),
    ("olive", [128, 128, 0]),
    ("purple", [128, 0, 128]),
];

pub fn paint_from_legacy_str(input: &str) -> Option<Paint> {
    let text = input.trim();
    let lower = text.to_ascii_lowercase();
    if lower.is_empty() || lower == "none" || lower == "transparent" {
        return Some(Paint::None);
    }
    if let Some((_, [r, g, b])) = LEGACY_COLOUR_NAMES.iter().find(|(name, _)| *name == lower) {
        return Some(Paint::Solid { colour: Colour::new(*r, *g, *b, 255) });
    }
    let hex = lower.trim_start_matches('#');
    let expanded: String = if hex.len() == 3 || hex.len() == 4 { hex.chars().flat_map(|c| [c, c]).collect() } else { hex.to_string() };
    parse_hex_color(&expanded).map(|colour| Paint::Solid { colour })
}

pub fn paint_to_legacy_str(paint: &Paint) -> Option<String> {
    let colour = match paint {
        Paint::None => return None,
        Paint::Solid { colour } => *colour,
        Paint::Gradient { stops, .. } => stops.first()?.colour,
        _ => return None,
    };
    if colour.a == 255 {
        Some(format!("#{:02x}{:02x}{:02x}", colour.r, colour.g, colour.b))
    } else {
        Some(format!("#{:02x}{:02x}{:02x}{:02x}", colour.r, colour.g, colour.b, colour.a))
    }
}

pub fn paint_from_legacy_value(value: &serde_json::Value) -> Option<Paint> {
    match value {
        serde_json::Value::Null => None,
        serde_json::Value::String(text) => paint_from_legacy_str(text),
        serde_json::Value::Array(items) => {
            let parts: Vec<u8> = items.iter().filter_map(|v| v.as_u64()).map(|v| v.min(255) as u8).collect();
            if parts.len() != items.len() || !(parts.len() == 3 || parts.len() == 4) {
                return None;
            }
            Some(Paint::Solid { colour: Colour::new(parts[0], parts[1], parts[2], parts.get(3).copied().unwrap_or(255)) })
        }
        serde_json::Value::Object(map) => {
            if map.contains_key("type") {
                return serde_json::from_value::<Paint>(value.clone()).ok();
            }
            if map.contains_key("r") || map.contains_key("model") {
                return serde_json::from_value::<Colour>(value.clone()).ok().map(|colour| Paint::Solid { colour });
            }
            for key in ["paint", "fill", "colour", "color", "value", "hex"] {
                if let Some(inner) = map.get(key) {
                    return paint_from_legacy_value(inner);
                }
            }
            None
        }
        _ => None,
    }
}

pub const PARAM_PAINT_KINDS: &[&str] = &["none", "solid", "gradient", "hatch", "bitmap", "pattern", "mesh", "diffusion"];

pub fn paint_from_param(value: &serde_json::Value) -> Result<Paint, dtool_protocol::CommandError> {
    let paint = paint_from_param_unchecked(value)?;
    if validate_paint(&paint) { Ok(paint) } else { Err(dtool_protocol::CommandError::invalid_params()) }
}

fn paint_from_param_unchecked(value: &serde_json::Value) -> Result<Paint, dtool_protocol::CommandError> {
    match value {
        serde_json::Value::Null => Ok(Paint::None),
        serde_json::Value::String(text) if text.trim().is_empty() || text.trim().eq_ignore_ascii_case("none") => Ok(Paint::None),
        serde_json::Value::Object(map) if map.contains_key("type") => {
            let kind = map.get("type").and_then(|v| v.as_str()).ok_or_else(dtool_protocol::CommandError::invalid_params)?;
            if !PARAM_PAINT_KINDS.contains(&kind) {
                return Err(dtool_protocol::CommandError::invalid_params());
            }
            serde_json::from_value::<Paint>(value.clone()).map_err(|_| dtool_protocol::CommandError::invalid_params())
        }
        _ => paint_from_legacy_value(value).ok_or_else(dtool_protocol::CommandError::invalid_params),
    }
}

pub fn normalize_fill_param(params: &serde_json::Value) -> Result<serde_json::Value, dtool_protocol::CommandError> {
    let mut params = params.clone();
    if let Some(map) = params.as_object_mut() {
        if let Some(fill) = map.get("fill").filter(|fill| !fill.is_null()).cloned() {
            map.insert("fill".to_string(), paint_to_json_value(&paint_from_param(&fill)?));
        }
    }
    Ok(params)
}

pub mod legacy_stroke {
    use super::{paint_from_legacy_value, StrokeStyle};
    use serde::Deserialize;

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<StrokeStyle, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        if let Ok(style) = serde_json::from_value::<StrokeStyle>(value.clone()) {
            return Ok(style);
        }
        let mut style = StrokeStyle::default();
        match &value {
            serde_json::Value::Object(map) => {
                if let Some(paint) = ["paint", "colour", "color", "fill"].iter().find_map(|key| map.get(*key)).and_then(paint_from_legacy_value) {
                    style.paint = paint;
                }
                if let Some(width) = map.get("width").and_then(|v| v.as_f64()) {
                    style.width = width as _;
                }
            }
            other => {
                if let Some(paint) = paint_from_legacy_value(other) {
                    style.paint = paint;
                }
            }
        }
        Ok(style)
    }
}

pub mod legacy_fill {
    use super::{paint_from_legacy_value, Paint};
    use serde::Deserialize;

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<Paint>, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(paint_from_legacy_value(&value))
    }
}

pub mod legacy_paint {
    use super::{paint_from_legacy_value, Paint};
    use serde::Deserialize;

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Paint, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(paint_from_legacy_value(&value).unwrap_or(Paint::None))
    }
}

pub fn normalize_range_fill(range: &mut serde_json::Value) {
    let Some(map) = range.as_object_mut() else {
        return;
    };
    let Some(old) = map.get("fill").cloned() else {
        return;
    };
    let typed = paint_from_legacy_value(&old).map(|paint| paint_to_json_value(&paint)).unwrap_or(serde_json::Value::Null);
    map.insert("fill".to_string(), typed);
}

pub fn deserialize_ranges<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<serde_json::Value>, D::Error> {
    let mut ranges = <Vec<serde_json::Value> as serde::Deserialize>::deserialize(deserializer)?;
    for range in &mut ranges {
        normalize_range_fill(range);
    }
    Ok(ranges)
}

pub fn range_fill(range: &serde_json::Value) -> Option<Paint> {
    range.get("fill").and_then(paint_from_legacy_value)
}

pub fn set_range_fill(range: &mut serde_json::Value, paint: Option<&Paint>) {
    if let Some(map) = range.as_object_mut() {
        map.insert("fill".to_string(), paint.map(paint_to_json_value).unwrap_or(serde_json::Value::Null));
    }
}

#[cfg(test)]
mod legacy_fill_contracts {
    use super::*;

    #[derive(serde::Serialize, serde::Deserialize)]
    struct Holder {
        #[serde(default, deserialize_with = "legacy_fill::deserialize")]
        fill: Option<Paint>,
    }

    fn load(text: &str) -> Option<Paint> {
        serde_json::from_str::<Holder>(text).unwrap().fill
    }

    #[test]
    fn old_hex_string_loads_as_solid() {
        assert_eq!(load(r##"{"fill":"#102030"}"##), Some(Paint::Solid { colour: Colour::new(16, 32, 48, 255) }));
        assert_eq!(load(r##"{"fill":"#10203080"}"##), Some(Paint::Solid { colour: Colour::new(16, 32, 48, 128) }));
        assert_eq!(load(r##"{"fill":"#fff"}"##), Some(Paint::Solid { colour: Colour::new(255, 255, 255, 255) }));
    }

    #[test]
    fn old_colour_name_loads_as_solid() {
        assert_eq!(load(r#"{"fill":"red"}"#), Some(Paint::Solid { colour: Colour::new(255, 0, 0, 255) }));
        assert_eq!(load(r#"{"fill":"none"}"#), Some(Paint::None));
    }

    #[test]
    fn old_json_shapes_load() {
        assert_eq!(load(r#"{"fill":{"r":1,"g":2,"b":3,"a":4}}"#), Some(Paint::Solid { colour: Colour::new(1, 2, 3, 4) }));
        assert_eq!(load(r#"{"fill":[9,8,7]}"#), Some(Paint::Solid { colour: Colour::new(9, 8, 7, 255) }));
        assert_eq!(load(r##"{"fill":{"colour":"#000000"}}"##), Some(Paint::Solid { colour: Colour::new(0, 0, 0, 255) }));
    }

    #[test]
    fn missing_null_and_unknown_load_as_none() {
        assert_eq!(load("{}"), None);
        assert_eq!(load(r#"{"fill":null}"#), None);
        assert_eq!(load(r#"{"fill":"not-a-colour"}"#), None);
    }

    #[test]
    fn typed_paint_round_trips() {
        let paint = Paint::Solid { colour: Colour::new(10, 20, 30, 255) };
        let text = serde_json::to_string(&Holder { fill: Some(paint.clone()) }).unwrap();
        assert!(text.contains("\"type\""));
        assert_eq!(load(&text), Some(paint));
        let none = serde_json::to_string(&Holder { fill: Some(Paint::None) }).unwrap();
        assert_eq!(load(&none), Some(Paint::None));
    }

    #[test]
    fn legacy_accessor_and_ranges() {
        assert_eq!(paint_to_legacy_str(&Paint::Solid { colour: Colour::new(16, 32, 48, 255) }).as_deref(), Some("#102030"));
        let mut range = serde_json::json!({ "start": 0, "end": 3, "fill": "#ff0000" });
        normalize_range_fill(&mut range);
        assert_eq!(range["fill"]["type"], serde_json::json!("solid"));
        assert_eq!(range_fill(&range), Some(Paint::Solid { colour: Colour::new(255, 0, 0, 255) }));
        set_range_fill(&mut range, None);
        assert_eq!(range_fill(&range), None);
    }

    #[test]
    fn story_ranges_load_old_fill_as_typed() {
        let text = r##"{"id":1,"text":"abcd","ranges":[{"start":0,"end":2,"fill":"#00ff00"},{"start":2,"end":4,"fill":{"type":"solid","colour":{"r":1,"g":2,"b":3,"a":255}}},{"start":4,"end":4}]}"##;
        let story: crate::document::Story = serde_json::from_str(text).unwrap();
        assert_eq!(range_fill(&story.ranges[0]), Some(Paint::Solid { colour: Colour::new(0, 255, 0, 255) }));
        assert_eq!(story.ranges[0]["fill"]["type"], serde_json::json!("solid"));
        assert_eq!(range_fill(&story.ranges[1]), Some(Paint::Solid { colour: Colour::new(1, 2, 3, 255) }));
        assert_eq!(range_fill(&story.ranges[2]), None);
        let again: crate::document::Story = serde_json::from_str(&serde_json::to_string(&story).unwrap()).unwrap();
        assert_eq!(again.ranges, story.ranges);
    }

    #[test]
    fn edge_and_frame_paint_load_old_strings() {
        let edge: crate::table::model::EdgeStyle = serde_json::from_str(r##"{"paint":"#ff0000","width_pt":1.0}"##).unwrap();
        assert_eq!(edge.paint, Paint::Solid { colour: Colour::new(255, 0, 0, 255) });
        assert_eq!(edge.paint_str(), "#ff0000");
        let edge: crate::table::types::EdgeStyle = serde_json::from_str(r#"{"paint":{"type":"none"},"width_pt":1.0}"#).unwrap();
        assert_eq!(edge.paint, Paint::None);
        let mut frame: crate::table::model::TableFrameFormat = serde_json::from_str(r#"{"fill":"blue","edges":[null,null,null,null],"insets":{"left":0.0,"top":0.0,"right":0.0,"bottom":0.0,"linked":false},"ignore_baseline_grid":false,"baseline":null}"#).unwrap();
        assert_eq!(frame.fill, Some(Paint::Solid { colour: Colour::new(0, 0, 255, 255) }));
        frame.set_fill_str(Some("#00000080"));
        let again: crate::table::model::TableFrameFormat = serde_json::from_str(&serde_json::to_string(&frame).unwrap()).unwrap();
        assert_eq!(again.fill, Some(Paint::Solid { colour: Colour::new(0, 0, 0, 128) }));
    }

    #[test]
    fn cell_paint_owner_is_atomic() {
        let mut owner = TableCell::new();
        let red = Paint::Solid { colour: Colour::new(255, 0, 0, 255) };
        assert!(owner.try_set_fill(red.clone()).is_ok());
        let bad = Paint::Gradient { kind: GradientKind::Linear, stops: Vec::new(), geom: GradientGeom::default() };
        assert!(owner.try_set_fill(bad).is_err());
        assert_eq!(owner.fill, red);
        let bad_stroke = StrokeStyle { width: -1.0, ..StrokeStyle::default() };
        assert!(owner.try_set(Paint::None, bad_stroke).is_err());
        assert_eq!(owner.fill, red);
        assert_eq!(owner.stroke, StrokeStyle::default());
    }

    #[test]
    fn real_cell_takes_owner_paint_and_keeps_old_on_failure() {
        let mut cell: crate::table::model::TableCell = serde_json::from_value(serde_json::json!({"id":5,"row":0,"col":0,"row_span":1,"col_span":1,"merged_into":null,"story":1,"local":{"fill":"#00ff00","edges":[null,null,null,null],"insets":{"left":0.0,"top":0.0,"right":0.0,"bottom":0.0,"linked":false},"vertical_align":"Top","rotation":"R0","initial_advance":"Default","minimum_pt":0.0,"paragraph_style":null},"mask":0})).unwrap();
        let green = Paint::Solid { colour: Colour::new(0, 255, 0, 255) };
        assert!(set_cell_stroke(&mut cell, StrokeStyle { width: f32::NAN, ..StrokeStyle::default() }).is_err());
        assert_eq!(cell.local.fill, Some(green.clone()));
        assert_eq!(cell.mask.0, 0);
        let blue = Paint::Solid { colour: Colour::new(0, 0, 255, 255) };
        assert!(set_cell_stroke(&mut cell, StrokeStyle { paint: blue.clone(), width: 1.0, ..StrokeStyle::default() }).is_ok());
        assert_eq!(cell.local.fill, Some(green));
        assert_eq!(TableCell::from_cell(&cell).stroke.paint, blue);
        assert_eq!(cell.mask.0 & crate::table::resolve::CELL_FILL, 0);
        assert!(set_cell_fill(&mut cell, Paint::None).is_ok());
        assert_eq!(TableCell::from_cell(&cell).fill, Paint::None);
        assert_ne!(cell.mask.0 & crate::table::resolve::CELL_FILL, 0);
    }

    #[test]
    fn role_projection_uses_owner_paint_or_none() {
        let red = Paint::Solid { colour: Colour::new(255, 0, 0, 255) };
        let mut object = crate::object::Object::default();
        assert_eq!(project_object_paint(PaintRole::SourceObject, &object), Paint::None);
        object.fill = red.clone();
        assert_eq!(project_object_paint(PaintRole::SourceObject, &object), red);
        assert_eq!(project_object_paint(PaintRole::Cell, &object), Paint::None);
        let mut cell = crate::table::model::CellFormat::default();
        assert_eq!(project_table_paint(PaintRole::Cell, None, Some(&cell), None, None), Paint::None);
        cell.fill = Some(red.clone());
        assert_eq!(project_table_paint(PaintRole::Cell, None, Some(&cell), None, None), red);
        let edge = crate::table::model::EdgeStyle { paint: red.clone(), width_pt: 1.0, line_style: LineStyle::Solid, align: StrokeAlign::Centre };
        assert_eq!(project_table_paint(PaintRole::Edge, None, None, Some(&edge), None), red);
        assert_eq!(project_table_paint(PaintRole::Glyph, None, Some(&cell), Some(&edge), None), Paint::None);
        assert_eq!(PaintRole::from_display_role("TableEdge"), PaintRole::Edge);
        assert_eq!(PaintRole::from_display_role("TableFrame"), PaintRole::PictureFrame);
    }
}
