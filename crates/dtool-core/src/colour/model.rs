use serde::{Serialize,Deserialize};

#[derive(Debug,Clone,Copy,PartialEq,Eq,Hash,Serialize,Deserialize)]
pub enum ColourModel {Rgb,Cmyk,Lab,Grey}
#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)]
pub enum BitDepth {U8,U16,F32}
#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)]
pub struct ColourFormat {pub model:ColourModel,pub depth:BitDepth}
impl std::fmt::Display for ColourFormat {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{
        write!(f,"{}/{}",match self.model{ColourModel::Rgb=>"RGBA",ColourModel::Cmyk=>"CMYK",ColourModel::Lab=>"LAB",ColourModel::Grey=>"Grey"},match self.depth{BitDepth::U8=>"8",BitDepth::U16=>"16",BitDepth::F32=>"32"})
    }
}
#[derive(Debug,Clone,PartialEq,Serialize,Deserialize)]
pub struct Colour {pub model:ColourModel,pub values:[f32;4],pub alpha:f32}
#[derive(Debug,Clone,Copy,Default,PartialEq,Eq,Hash,Serialize,Deserialize)]
pub enum RenderingIntent {Perceptual,#[default]RelativeColorimetric,Saturation,AbsoluteColorimetric}
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct ConvertCtx {pub from:String,pub to:String,pub intent:RenderingIntent,pub bpc:bool}
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct DocColour {pub format:ColourFormat,pub profile:String}
impl Default for DocColour {fn default()->Self{Self{format:ColourFormat{model:ColourModel::Rgb,depth:BitDepth::U8},profile:"sRGB IEC61966-2.1".into()}}}
impl DocColour {
    pub fn for_format(format: ColourFormat, prefs: &ColourPrefs) -> Self {
        Self { format, profile: format.default_profile(prefs) }
    }
}

#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct ColourPrefs {
    pub rgb:String,pub rgb32:String,pub cmyk:String,pub grey:String,pub lab:String,
    pub intent:RenderingIntent,pub bpc:bool,pub convert_on_open:bool,pub warn_on_convert:bool,
    pub warn_assign_unprofiled:bool,pub hdr_default:bool,pub ocio_config:Option<std::path::PathBuf>,pub ocio_by_filename:bool,pub ocio_warn:bool,
}
impl ColourPrefs {
    pub const CMYK_PREF: &str = "ColourPrefs.cmyk";
    pub const LAB_PREF: &str = "ColourPrefs.lab";
    pub const GREY_PREF: &str = "ColourPrefs.grey";
}
impl Default for ColourPrefs {
    fn default()->Self{Self{rgb:super::ProfileStore::SRGB.into(),rgb32:super::ProfileStore::SRGB.into(),cmyk:super::ProfileStore::FOGRA39L.into(),grey:String::new(),lab:super::ProfileStore::LAB_D50.into(),intent:Default::default(),bpc:true,convert_on_open:false,warn_on_convert:true,warn_assign_unprofiled:true,hdr_default:false,ocio_config:None,ocio_by_filename:false,ocio_warn:true}}
}

#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct ObjectIcc {pub profile:Option<String>,pub intent:Option<RenderingIntent>}
impl ObjectIcc {
    pub const INTENT: &str = "ObjectIcc.intent";
    pub fn effective_intent(&self, document_intent: RenderingIntent) -> RenderingIntent {
        let _ = Self::INTENT;
        self.intent.unwrap_or(document_intent)
    }
}
impl RenderingIntent {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "Perceptual" => Some(Self::Perceptual),
            "RelativeColorimetric" => Some(Self::RelativeColorimetric),
            "Saturation" => Some(Self::Saturation),
            "AbsoluteColorimetric" => Some(Self::AbsoluteColorimetric),
            _ => None,
        }
    }
}

impl ColourFormat {
    pub const RGBA_8: Self = Self { model: ColourModel::Rgb, depth: BitDepth::U8 };
    pub const RGBA_16: Self = Self { model: ColourModel::Rgb, depth: BitDepth::U16 };
    pub const RGBA_32: Self = Self { model: ColourModel::Rgb, depth: BitDepth::F32 };
    pub const CMYK_8: Self = Self { model: ColourModel::Cmyk, depth: BitDepth::U8 };
    pub const LAB_16: Self = Self { model: ColourModel::Lab, depth: BitDepth::U16 };
    pub const GREY_8: Self = Self { model: ColourModel::Grey, depth: BitDepth::U8 };
    pub const GREY_16: Self = Self { model: ColourModel::Grey, depth: BitDepth::U16 };
    pub const LIST: [ColourFormat; 7] = [Self::RGBA_8, Self::RGBA_16, Self::RGBA_32, Self::CMYK_8, Self::LAB_16, Self::GREY_8, Self::GREY_16];
    pub fn colour_format_list() -> &'static [ColourFormat] { &Self::LIST }
    pub fn parse(label: &str) -> Option<Self> {
        Self::LIST.iter().copied().find(|format| format.to_string() == label)
    }
    pub fn default_profile(&self, prefs: &ColourPrefs) -> String {
        match (self.model, self.depth) {
            (ColourModel::Rgb, BitDepth::F32) => prefs.rgb32.clone(),
            (ColourModel::Rgb, _) => prefs.rgb.clone(),
            (ColourModel::Cmyk, _) => { let _ = ColourPrefs::CMYK_PREF; prefs.cmyk.clone() },
            (ColourModel::Lab, _) => { let _ = ColourPrefs::LAB_PREF; prefs.lab.clone() },
            (ColourModel::Grey, _) => { let _ = ColourPrefs::GREY_PREF; prefs.grey.clone() },
        }
    }
}

impl Colour {
    pub fn rgb(r: f32, g: f32, b: f32, alpha: f32) -> Self {
        Self { model: ColourModel::Rgb, values: [r, g, b, 0.0], alpha }
    }
    pub fn cmyk(c: f32, m: f32, y: f32, k: f32, alpha: f32) -> Self {
        let mut colour = Self { model: ColourModel::Cmyk, values: [c, m, y, k], alpha };
        colour.clamp_on_write();
        colour
    }
    pub fn lab(l: f32, a: f32, b: f32, alpha: f32) -> Self {
        let mut colour = Self { model: ColourModel::Lab, values: [l, a, b, 0.0], alpha };
        colour.clamp_on_write();
        colour
    }
    pub fn grey(value: f32, alpha: f32) -> Self {
        let mut colour = Self { model: ColourModel::Grey, values: [value, 0.0, 0.0, 0.0], alpha };
        colour.clamp_on_write();
        colour
    }
    pub fn clamp_on_write(&mut self) {
        match self.model {
            ColourModel::Rgb => {
                self.values[0] = self.values[0].clamp(0.0, 1.0);
                self.values[1] = self.values[1].clamp(0.0, 1.0);
                self.values[2] = self.values[2].clamp(0.0, 1.0);
            }
            ColourModel::Cmyk => {
                self.values[0] = self.values[0].clamp(0.0, 100.0);
                self.values[1] = self.values[1].clamp(0.0, 100.0);
                self.values[2] = self.values[2].clamp(0.0, 100.0);
                self.values[3] = self.values[3].clamp(0.0, 100.0);
            }
            ColourModel::Lab => {
                self.values[0] = self.values[0].clamp(0.0, 100.0);
                self.values[1] = self.values[1].clamp(-128.0, 127.0);
                self.values[2] = self.values[2].clamp(-128.0, 127.0);
            }
            ColourModel::Grey => {
                self.values[0] = self.values[0].clamp(0.0, 100.0);
                self.values[1] = 0.0;
                self.values[2] = 0.0;
                self.values[3] = 0.0;
            }
        }
        if !self.alpha.is_finite() { self.alpha = 1.0; }
        self.alpha = self.alpha.clamp(0.0, 1.0);
    }
    pub fn write_for_document(self, format: ColourFormat, prefs: &ColourPrefs) -> Self {
        self.clone().try_write_for_document(format, prefs).unwrap_or(self)
    }
    pub fn lab_preview_srgb(&self) -> Option<[f32; 3]> {
        (self.model == ColourModel::Lab).then(|| {
            let (r, g, b, _) = lab_to_srgb(self.values[0], self.values[1], self.values[2], self.alpha);
            [r, g, b]
        })
    }
    pub fn try_write_for_document(self, format: ColourFormat, prefs: &ColourPrefs) -> Result<Self, crate::CommandError> {
        let mut out = if self.model == format.model { self } else { convert_on_write(self, format, prefs, None, None, prefs.intent)? };
        out.model = format.model;
        out.clamp_on_write();
        Ok(out)
    }
    pub fn write_for_object(self, format: ColourFormat, prefs: &ColourPrefs, icc: Option<&ObjectIcc>) -> Self {
        self.clone().try_write_for_object(format, prefs, icc).unwrap_or(self)
    }
    pub fn try_write_for_object(self, format: ColourFormat, prefs: &ColourPrefs, icc: Option<&ObjectIcc>) -> Result<Self, crate::CommandError> {
        let intent = icc.map(|item| item.effective_intent(prefs.intent)).unwrap_or(prefs.intent);
        let from = icc.and_then(|item| item.profile.clone()).filter(|name| !name.is_empty());
        let to = format.default_profile(prefs);
        let default_from = default_source_profile(self.model, format, prefs);
        let source_profile = from.unwrap_or(default_from);
        let same_profile = source_profile == to;
        let mut out = if self.model == format.model && same_profile {
            self
        } else {
            convert_on_write(self, format, prefs, Some(source_profile), Some(to), intent)?
        };
        out.model = format.model;
        out.clamp_on_write();
        Ok(out)
    }
}

fn default_source_profile(model: ColourModel, format: ColourFormat, prefs: &ColourPrefs) -> String {
    match model {
        ColourModel::Rgb if format.depth == BitDepth::F32 => prefs.rgb32.clone(),
        ColourModel::Rgb => prefs.rgb.clone(),
        ColourModel::Cmyk => prefs.cmyk.clone(),
        ColourModel::Lab => prefs.lab.clone(),
        ColourModel::Grey => prefs.grey.clone(),
    }
}

fn convert_on_write(source: Colour, format: ColourFormat, prefs: &ColourPrefs, from: Option<String>, to: Option<String>, intent: RenderingIntent) -> Result<Colour, crate::CommandError> {
    let from = from.unwrap_or_else(|| default_source_profile(source.model, format, prefs));
    let to = to.unwrap_or_else(|| format.default_profile(prefs));
    let services = super::services().read().map_err(|_| crate::CommandError::new("COLOUR_PROFILE_MISSING"))?;
    if services.profiles.find(&from).is_none() || services.profiles.find(&to).is_none() {
        return Err(crate::CommandError::new("COLOUR_PROFILE_MISSING"));
    }
    let ctx = ConvertCtx { from, to, intent, bpc: prefs.bpc };
    super::convert::convert(&source, format.model, &ctx, &services.profiles)
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

fn lab_to_srgb(l: f32, a: f32, b: f32, alpha: f32) -> (f32, f32, f32, f32) {
    let fy = (l + 16.0) / 116.0;
    let fx = a / 500.0 + fy;
    let fz = fy - b / 200.0;
    let inv = |t: f32| { let t3 = t * t * t; if t3 > 0.008856 { t3 } else { (t - 16.0 / 116.0) / 7.787 } };
    let x = 0.95047 * inv(fx);
    let y = inv(fy);
    let z = 1.08883 * inv(fz);
    let r =  3.2404542 * x - 1.5371385 * y - 0.4985314 * z;
    let g = -0.9692660 * x + 1.8760108 * y + 0.0415560 * z;
    let b =  0.0556434 * x - 0.2040259 * y + 1.0572252 * z;
    (linear_to_srgb(r).clamp(0.0, 1.0), linear_to_srgb(g).clamp(0.0, 1.0), linear_to_srgb(b).clamp(0.0, 1.0), alpha)
}
