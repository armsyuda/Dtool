use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use parley::{FontContext, LayoutContext};

static FONT_GENERATION: AtomicU64 = AtomicU64::new(0);

pub fn font_generation() -> u64 {
    FONT_GENERATION.load(Ordering::SeqCst)
}

pub(crate) fn bump_font_generation() -> u64 {
    let generation = FONT_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    dtool_core::text_lines::publish_font_generation(generation);
    generation
}

pub fn note_font_change(before: u64, after: u64) -> u64 {
    if before == after { font_generation() } else { bump_font_generation() }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FontId(pub u32);

#[derive(Debug, Clone)]
pub struct FontFace {
    pub id: FontId,
    pub family: String,
    pub style: String,
    pub weight: u16,
    pub italic: bool,
    pub path: PathBuf,
    pub index: u32,
    pub stretch: f32,
}

pub const UNMAPPED_FONT: FontId = FontId(u32::MAX);

pub fn style_name(weight: u16, italic: bool) -> String {
    let base = match weight {
        0..=149 => "Thin",
        150..=249 => "ExtraLight",
        250..=349 => "Light",
        350..=449 => "Regular",
        450..=549 => "Medium",
        550..=649 => "SemiBold",
        650..=749 => "Bold",
        750..=849 => "ExtraBold",
        _ => "Black",
    };
    match (base, italic) {
        ("Regular", true) => "Italic".to_string(),
        (base, true) => format!("{base} Italic"),
        (base, false) => base.to_string(),
    }
}

pub fn parse_style(style: &str) -> Option<(u16, bool)> {
    let key: String = style.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    let italic = key.contains("italic") || key.contains("oblique");
    let rest = key.replace("italic", "").replace("oblique", "");
    let weight = match rest.as_str() {
        "thin" | "hairline" => 100,
        "extralight" | "ultralight" => 200,
        "light" => 300,
        "" | "regular" | "normal" | "book" | "roman" => 400,
        "medium" => 500,
        "semibold" | "demibold" => 600,
        "bold" => 700,
        "extrabold" | "ultrabold" => 800,
        "black" | "heavy" => 900,
        digits if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) => digits.parse().ok()?,
        _ => return None,
    };
    Some((weight, italic))
}

pub struct FontEngine {
    pub fonts: FontContext,
    pub layout: LayoutContext<()>,
    pub ids: HashMap<(u64, u32), FontId>,
}

type GlyphContours = Arc<[Vec<[f32; 2]>]>;

#[derive(Default)]
struct GlyphContourCache {
    entries: HashMap<(FontId, u16, u32), GlyphContours>,
    points: usize,
}

pub struct FontLibrary {
    pub(crate) engine: Arc<Mutex<FontEngine>>,
    faces: Arc<Vec<FontFace>>,
    families: Arc<Vec<String>>,
    font_data: Arc<Vec<Option<parley::fontique::Blob<u8>>>>,
    contours: Arc<Mutex<GlyphContourCache>>,
}

impl Clone for FontLibrary {
    fn clone(&self) -> Self {
        Self {
            engine: self.engine.clone(),
            faces: self.faces.clone(),
            families: self.families.clone(),
            font_data: self.font_data.clone(),
            contours: self.contours.clone(),
        }
    }
}

impl FontLibrary {
    pub fn system() -> Self {
        static SYSTEM: OnceLock<FontLibrary> = OnceLock::new();
        SYSTEM.get_or_init(Self::scan).clone()
    }

    pub fn shared() -> Arc<Self> {
        static SHARED: OnceLock<Arc<FontLibrary>> = OnceLock::new();
        SHARED.get_or_init(|| Arc::new(Self::system())).clone()
    }

    pub fn from_fixtures<P: AsRef<std::path::Path>>(paths: &[P]) -> std::io::Result<Self> {
        let mut fonts = FontContext {
            collection: parley::fontique::Collection::new(parley::fontique::CollectionOptions {
                system_fonts: false,
                ..Default::default()
            }),
            source_cache: Default::default(),
        };
        for path in paths {
            let path = path.as_ref();
            let data = std::fs::read(path)?;
            let added = fonts.collection.register_fonts(parley::fontique::Blob::new(Arc::new(data)), None);
            if added.is_empty() {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("Invalid font fixture: {}", path.display())));
            }
        }
        Ok(Self::from_context(fonts))
    }

    fn scan() -> Self {
        let mut fonts = FontContext::new();
        fonts.collection.load_system_fonts();
        Self::from_context(fonts)
    }

    fn from_context(mut fonts: FontContext) -> Self {
        let mut families: Vec<String> = fonts.collection.family_names().map(str::to_owned).collect();
        families.sort_by_key(|s| s.to_lowercase());
        families.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        let mut faces = Vec::new();
        let mut ids = HashMap::new();
        let mut font_data = Vec::new();
        // Keep the blobs registered in the shared backing cache so shaping and
        // outline drawing use the same IDs after Parley prunes its local cache.
        fonts.source_cache.make_shared();
        for name in &families {
            if let Some(family) = fonts.collection.family_by_name(name) {
                for info in family.fonts() {
                    let id = FontId(faces.len() as u32);
                    let italic = info.style() != parley::FontStyle::Normal;
                    let weight = info.weight().value() as u16;
                    let style = style_name(weight, italic);
                    let stretch = info.width().ratio();
                    let path = match info.source().kind() {
                        parley::fontique::SourceKind::Path(path) => path.as_ref().to_path_buf(),
                        _ => PathBuf::new(),
                    };
                    let data = info.load(Some(&mut fonts.source_cache));
                    if let Some(blob) = &data {
                        ids.insert((blob.id(), info.index()), id);
                    }
                    font_data.push(data);
                    faces.push(FontFace {
                        id,
                        family: name.clone(),
                        style,
                        weight,
                        italic,
                        path,
                        index: info.index(),
                        stretch,
                    });
                }
            }
        }
        let library = Self {
            engine: Arc::new(Mutex::new(FontEngine {
                fonts,
                layout: LayoutContext::new(),
                ids,
            })),
            faces: Arc::new(faces),
            families: Arc::new(families),
            font_data: Arc::new(font_data),
            contours: Arc::default(),
        };
        bump_font_generation();
        library
    }

    pub fn signature(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for face in self.faces.iter() {
            (&face.family, &face.style, face.weight, face.italic, &face.path, face.index, face.stretch.to_bits()).hash(&mut hasher);
        }
        hasher.finish()
    }

    pub fn families(&self) -> Vec<&str> {
        self.families.iter().map(String::as_str).collect()
    }

    pub fn faces(&self, family: &str) -> Vec<&FontFace> {
        self.faces
            .iter()
            .filter(|f| f.family.eq_ignore_ascii_case(family))
            .collect()
    }

    pub fn face(&self, id: FontId) -> Option<&FontFace> {
        self.faces.get(id.0 as usize)
    }

    pub fn resolve(&self, family: &str, style: &str) -> Option<FontId> {
        let faces = self.faces(family);
        if let Some(face) = faces.iter().find(|f| f.style.eq_ignore_ascii_case(style)) {
            return Some(face.id);
        }
        let (weight, italic) = parse_style(style)?;
        faces
            .iter()
            .filter(|f| f.weight == weight && f.italic == italic)
            .min_by(|a, b| (a.stretch - 1.0).abs().total_cmp(&(b.stretch - 1.0).abs()))
            .map(|f| f.id)
    }

    pub fn resolve_nearest(&self, family: &str, weight: u16, italic: bool) -> Option<FontId> {
        self.faces(family)
            .into_iter()
            .min_by_key(|f| (f.italic != italic, (i32::from(f.weight) - i32::from(weight)).abs()))
            .map(|f| f.id)
    }

    fn with_font<R>(&self, id: FontId, read: impl FnOnce(&skrifa::FontRef) -> R) -> Option<R> {
        let face = self.face(id)?;
        let data = self.font_data.get(id.0 as usize)?.as_ref()?;
        let font = skrifa::FontRef::from_index(data.data(), face.index).ok()?;
        Some(read(&font))
    }

    pub fn has_glyph(&self, id: FontId, ch: char) -> bool {
        use skrifa::MetadataProvider;
        self.with_font(id, |font| font.charmap().map(ch).is_some()).unwrap_or(false)
    }

    pub(crate) fn vertical_alternate(&self, id: FontId, gid: u16) -> Option<u16> {
        use skrifa::raw::tables::gsub::{SingleSubst, SubstitutionSubtables};
        use skrifa::raw::TableProvider;
        use skrifa::{GlyphId16, Tag};

        self.with_font(id, |font| {
            let table = font.gsub().ok()?;
            let features = table.feature_list().ok()?;
            let lookups = table.lookup_list().ok()?.lookups();
            for record in features.feature_records().iter().filter(|record| record.feature_tag() == Tag::new(b"vert")) {
                let Ok(feature) = record.feature(features.offset_data()) else { continue; };
                let mut current = gid;
                for index in feature.lookup_list_indices() {
                    let Ok(lookup) = lookups.get(usize::from(index.get())) else { continue; };
                    let Ok(SubstitutionSubtables::Single(subtables)) = lookup.subtables() else { continue; };
                    let input = GlyphId16::new(current);
                    let substitute = subtables.iter().flatten().find_map(|table| match table {
                        SingleSubst::Format1(table) => {
                            table.coverage().ok()?.get(input)?;
                            Some(current.wrapping_add_signed(table.delta_glyph_id()))
                        }
                        SingleSubst::Format2(table) => {
                            let index = usize::from(table.coverage().ok()?.get(input)?);
                            Some(table.substitute_glyph_ids().get(index)?.get().to_u16())
                        }
                    });
                    if let Some(next) = substitute { current = next; }
                }
                if current != gid && current != 0 { return Some(current); }
            }
            None
        }).flatten()
    }

    pub(crate) fn unsupported_chars(&self, id: FontId, chars: &std::collections::BTreeSet<char>) -> Vec<char> {
        use skrifa::MetadataProvider;
        self.with_font(id, |font| {
            let map = font.charmap();
            chars.iter().copied().filter(|&ch| {
                // Formatting and shaping controls need no standalone glyph.
                let formatting = matches!(ch, '\u{ad}' | '\u{34f}' | '\u{61c}' | '\u{180b}'..='\u{180f}'
                    | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{206f}'
                    | '\u{fe00}'..='\u{fe0f}' | '\u{feff}' | '\u{e0000}'..='\u{e01ef}');
                !ch.is_control() && !ch.is_whitespace() && !formatting && map.map(ch).is_none()
            }).collect()
        }).unwrap_or_default()
    }
}

pub const FALLBACK_FAMILIES: [&str; 3] = ["Malgun Gothic", "Segoe UI", "Segoe UI Symbol"];

pub fn is_hangul(ch: char) -> bool {
    matches!(ch, '\u{1100}'..='\u{11ff}' | '\u{3130}'..='\u{318f}' | '\u{a960}'..='\u{a97f}' | '\u{ac00}'..='\u{d7af}' | '\u{d7b0}'..='\u{d7ff}')
}

fn quoted_stack<'a>(names: impl Iterator<Item = &'a str>) -> String {
    let mut out: Vec<String> = Vec::new();
    for name in names {
        let name = name.replace(['"', '\\'], "");
        let name = name.trim();
        if !name.is_empty() && !out.iter().any(|item| item.eq_ignore_ascii_case(name)) {
            out.push(name.to_string());
        }
    }
    out.iter().map(|name| format!("\"{name}\"")).collect::<Vec<_>>().join(", ")
}

pub fn family_stack(primary: &str) -> String {
    quoted_stack(std::iter::once(primary).chain(FALLBACK_FAMILIES))
}

pub const HANGUL_FALLBACK_FAMILIES: [&str; 7] = ["Malgun Gothic", "Apple SD Gothic Neo", "Noto Sans CJK KR", "Noto Sans KR", "Source Han Sans K", "NanumGothic", "Pretendard"];
pub const GENERAL_FALLBACK_FAMILIES: [&str; 8] = ["Segoe UI", "Segoe UI Symbol", "Helvetica Neue", "Noto Sans", "DejaVu Sans", "Noto Sans Symbols", "Apple Symbols", "Inter"];

pub fn fallback_families(ch: char) -> Vec<&'static str> {
    let general = GENERAL_FALLBACK_FAMILIES.iter().copied();
    if is_hangul(ch) {
        HANGUL_FALLBACK_FAMILIES.iter().copied().chain(general).collect()
    } else {
        general.collect()
    }
}

pub fn fallback_chain(lib: &FontLibrary, primary: FontId, ch: char) -> Vec<FontId> {
    let (weight, italic) = lib.face(primary).map_or((400, false), |face| (face.weight, face.italic));
    let mut listed: Vec<FontId> = Vec::new();
    for family in fallback_families(ch) {
        if let Some(id) = lib.resolve_nearest(family, weight, italic) {
            if id != primary && !listed.contains(&id) {
                listed.push(id);
            }
        }
    }
    let (mut covering, rest): (Vec<FontId>, Vec<FontId>) = listed.into_iter().partition(|id| lib.has_glyph(*id, ch));
    if covering.is_empty() {
        if let Some(face) = lib.faces.iter().filter(|face| face.id != primary && !rest.contains(&face.id)).find(|face| lib.has_glyph(face.id, ch)) {
            covering.push(face.id);
        }
    }
    std::iter::once(primary).chain(covering).chain(rest).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackRun {
    pub start: usize,
    pub end: usize,
    pub font: Option<FontId>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FallbackSplit {
    pub runs: Vec<FallbackRun>,
    pub missing_chars: Vec<usize>,
    pub missing_font: bool,
}

fn fallback_candidates(lib: &FontLibrary, primary: Option<FontId>, ch: char) -> Vec<FontId> {
    if let Some(primary) = primary {
        return fallback_chain(lib, primary, ch);
    }
    let mut out: Vec<FontId> = Vec::new();
    for family in fallback_families(ch) {
        if let Some(id) = lib.resolve(family, "Regular") {
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

pub fn split_by_fallback(lib: &FontLibrary, family: &str, style: &str, text: &str) -> FallbackSplit {
    let primary = lib.resolve(family, style).or_else(|| lib.faces(family).first().map(|face| face.id));
    let mut split = FallbackSplit { missing_font: primary.is_none(), ..FallbackSplit::default() };
    let mut found_for: HashMap<char, Option<FontId>> = HashMap::new();
    let mut previous = primary;
    for (index, ch) in text.chars().enumerate() {
        let font = if ch.is_control() || ch.is_whitespace() {
            previous
        } else {
            let found = *found_for.entry(ch).or_insert_with(|| fallback_candidates(lib, primary, ch).into_iter().find(|id| lib.has_glyph(*id, ch)));
            if found.is_none() {
                split.missing_chars.push(index);
            }
            found.or(primary)
        };
        previous = font;
        match split.runs.last_mut() {
            Some(run) if run.font == font && run.end == index => run.end = index + 1,
            _ => split.runs.push(FallbackRun { start: index, end: index + 1, font }),
        }
    }
    split
}

pub fn family_stack_for(lib: &FontLibrary, primary: FontId, text: &str) -> String {
    let Some(face) = lib.face(primary) else {
        return family_stack("");
    };
    let mut seen: Vec<char> = Vec::new();
    let mut extra: Vec<String> = Vec::new();
    for ch in text.chars() {
        if ch.is_control() || ch.is_whitespace() || seen.contains(&ch) {
            continue;
        }
        seen.push(ch);
        if lib.has_glyph(primary, ch) {
            continue;
        }
        let chain = fallback_chain(lib, primary, ch);
        if let Some(found) = chain.iter().skip(1).find(|id| lib.has_glyph(**id, ch)).and_then(|id| lib.face(*id)) {
            if !extra.iter().any(|name| name.eq_ignore_ascii_case(&found.family)) {
                extra.push(found.family.clone());
            }
        }
    }
    quoted_stack(std::iter::once(face.family.as_str()).chain(extra.iter().map(String::as_str)).chain(FALLBACK_FAMILIES))
}

struct ContourPen {
    contours: Vec<Vec<[f32; 2]>>,
    current: Vec<[f32; 2]>,
    last: [f32; 2],
}

impl ContourPen {
    fn finish(&mut self) {
        if self.current.len() >= 3 {
            self.contours.push(std::mem::take(&mut self.current));
        } else {
            self.current.clear();
        }
    }
}

const CURVE_STEPS: u32 = 8;

impl skrifa::outline::OutlinePen for ContourPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.finish();
        self.current.push([x, y]);
        self.last = [x, y];
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.current.push([x, y]);
        self.last = [x, y];
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let p0 = self.last;
        for step in 1..=CURVE_STEPS {
            let t = step as f32 / CURVE_STEPS as f32;
            let u = 1.0 - t;
            self.current.push([u * u * p0[0] + 2.0 * u * t * cx0 + t * t * x, u * u * p0[1] + 2.0 * u * t * cy0 + t * t * y]);
        }
        self.last = [x, y];
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let p0 = self.last;
        for step in 1..=CURVE_STEPS {
            let t = step as f32 / CURVE_STEPS as f32;
            let u = 1.0 - t;
            let a = u * u * u;
            let b = 3.0 * u * u * t;
            let c = 3.0 * u * t * t;
            let d = t * t * t;
            self.current.push([a * p0[0] + b * cx0 + c * cx1 + d * x, a * p0[1] + b * cy0 + c * cy1 + d * y]);
        }
        self.last = [x, y];
    }

    fn close(&mut self) {
        self.finish();
    }
}

impl FontLibrary {
    pub fn glyph_contours(&self, id: FontId, gid: u16, size_pt: f32) -> Vec<Vec<[f32; 2]>> {
        self.shared_glyph_contours(id, gid, size_pt).to_vec()
    }

    pub(crate) fn shared_glyph_contours(&self, id: FontId, gid: u16, size_pt: f32) -> GlyphContours {
        use skrifa::MetadataProvider;
        const MAX_ENTRIES: usize = 4_096;
        const MAX_POINTS: usize = 512 * 1024;

        if !size_pt.is_finite() || size_pt <= 0.0 {
            return Arc::default();
        }
        // This library's faces and font bytes are immutable. Store the path
        // at its exact size before any placement or object transform.
        let key = (id, gid, size_pt.to_bits());
        if let Some(cached) = self.contours.lock().ok().and_then(|cache| cache.entries.get(&key).cloned()) {
            return cached;
        }
        let contours: GlyphContours = self.with_font(id, |font| {
            let mut pen = ContourPen { contours: Vec::new(), current: Vec::new(), last: [0.0, 0.0] };
            let outlines = font.outline_glyphs();
            if let Some(glyph) = outlines.get(skrifa::GlyphId::new(u32::from(gid))) {
                let settings = skrifa::outline::DrawSettings::unhinted(skrifa::instance::Size::new(size_pt), skrifa::instance::LocationRef::default());
                if glyph.draw(settings, &mut pen).is_err() {
                    pen.current.clear();
                }
            }
            pen.finish();
            pen.contours
        })
        .unwrap_or_default()
        .into();
        let points = contours.iter().map(Vec::len).sum::<usize>();
        if points <= MAX_POINTS && let Ok(mut cache) = self.contours.lock() {
            if let Some(cached) = cache.entries.get(&key) {
                return cached.clone();
            }
            if cache.entries.len() >= MAX_ENTRIES || cache.points + points > MAX_POINTS {
                cache.entries.clear();
                cache.points = 0;
            }
            cache.entries.insert(key, contours.clone());
            cache.points += points;
        }
        contours
    }
}
