use std::collections::HashMap;

use skrifa::raw::types::GlyphId16;
use skrifa::raw::{FontRef, TableProvider};
use skrifa::MetadataProvider;

use crate::fonts::FontLibrary;
use dtool_core::CommandError;

#[derive(Clone, Debug)]
pub struct GlyphInfo {
    pub gid: u16,
    pub unicode: Option<char>,
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Subset {
    All,
    Recent,
    Block(String),
}

#[derive(Clone, Debug)]
pub struct GlyphQuery {
    pub font: crate::fonts::FontId,
    pub search: Option<String>,
    pub subset: Subset,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Clone, Debug)]
pub struct GlyphPage {
    pub total: usize,
    pub items: Vec<GlyphInfo>,
}

#[derive(Clone, Debug)]
pub struct RecentGlyphs {
    pub items: Vec<(crate::fonts::FontId, u16)>,
}

impl RecentGlyphs {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn push(&mut self, font: crate::fonts::FontId, gid: u16) {
        self.items.retain(|(f, g)| *f != font || *g != gid);
        self.items.insert(0, (font, gid));
        if self.items.len() > 20 {
            self.items.truncate(20);
        }
    }
}

fn unicode_blocks_present(font_ref: &FontRef) -> Vec<String> {
    let mut blocks = std::collections::HashSet::new();
    for (unicode, gid) in font_ref.charmap().mappings() {
        if gid.to_u32() != 0 {
            if let Some(block_name) = unicode_block_name(unicode) {
                blocks.insert(block_name);
            }
        }
    }

    let mut result: Vec<String> = blocks.into_iter().collect();
    result.sort();
    result
}

fn unicode_block_name(codepoint: u32) -> Option<String> {
    if codepoint <= 0x7F {
        return Some("Basic Latin".to_string());
    }
    if codepoint <= 0xFF {
        return Some("Latin-1 Supplement".to_string());
    }
    if codepoint <= 0x17F {
        return Some("Latin Extended-A".to_string());
    }
    if codepoint <= 0x24F {
        return Some("Latin Extended-B".to_string());
    }
    if codepoint <= 0x2AF {
        return Some("IPA Extensions".to_string());
    }
    if codepoint <= 0x2FF {
        return Some("Spacing Modifier Letters".to_string());
    }
    if codepoint <= 0x36F {
        return Some("Combining Diacritical Marks".to_string());
    }
    if codepoint <= 0x3FF {
        return Some("Greek and Coptic".to_string());
    }
    if codepoint <= 0x4FF {
        return Some("Cyrillic".to_string());
    }
    if codepoint <= 0x52F {
        return Some("Cyrillic Supplement".to_string());
    }
    if codepoint <= 0x58F {
        return Some("Armenian".to_string());
    }
    if codepoint <= 0x5FF {
        return Some("Hebrew".to_string());
    }
    if codepoint <= 0x6FF {
        return Some("Arabic".to_string());
    }
    if codepoint <= 0x74F {
        return Some("Syriac".to_string());
    }
    if codepoint <= 0x76F {
        return Some("Arabic Supplement".to_string());
    }
    if codepoint <= 0x7BF {
        return Some("Thaana".to_string());
    }
    if codepoint <= 0x7FF {
        return Some("NKo".to_string());
    }
    if codepoint <= 0x85F {
        return Some("Samaritan".to_string());
    }
    if codepoint <= 0x89F {
        return Some("Mandaic".to_string());
    }
    if codepoint <= 0x8BF {
        return Some("Devanagari Extended".to_string());
    }
    if codepoint <= 0x8FF {
        return Some("Bengali".to_string());
    }
    if codepoint <= 0x97F {
        return Some("Gurmukhi".to_string());
    }
    if codepoint <= 0x9FF {
        return Some("Gujarati".to_string());
    }
    if codepoint <= 0xA7F {
        return Some("Oriya".to_string());
    }
    if codepoint <= 0xAFF {
        return Some("Tamil".to_string());
    }
    if codepoint <= 0xB7F {
        return Some("Telugu".to_string());
    }
    if codepoint <= 0xBFF {
        return Some("Kannada".to_string());
    }
    if codepoint <= 0xC7F {
        return Some("Malayalam".to_string());
    }
    if codepoint <= 0xCFF {
        return Some("Sinhala".to_string());
    }
    if codepoint <= 0xD7F {
        return Some("Thai".to_string());
    }
    if codepoint <= 0xDFF {
        return Some("Lao".to_string());
    }
    if codepoint <= 0xE7F {
        return Some("Tibetan".to_string());
    }
    if codepoint <= 0xEFF {
        return Some("Myanmar".to_string());
    }
    if codepoint <= 0xF7F {
        return Some("Georgian".to_string());
    }
    if codepoint <= 0xFFF {
        return Some("Hangul Jamo Extended-A".to_string());
    }
    if codepoint <= 0x10FF {
        return Some("Ethiopic".to_string());
    }
    if codepoint <= 0x137F {
        return Some("Ethiopic Supplement".to_string());
    }
    if codepoint <= 0x139F {
        return Some("Cherokee".to_string());
    }
    if codepoint <= 0x13FF {
        return Some("Unified Canadian Aboriginal Syllabics".to_string());
    }
    if codepoint <= 0x167F {
        return Some("Ogham".to_string());
    }
    if codepoint <= 0x169F {
        return Some("Runic".to_string());
    }
    if codepoint <= 0x16FF {
        return Some("Tagalog".to_string());
    }
    if codepoint <= 0x173F {
        return Some("Hanunoo".to_string());
    }
    if codepoint <= 0x175F {
        return Some("Buhid".to_string());
    }
    if codepoint <= 0x177F {
        return Some("Tagbanwa".to_string());
    }
    if codepoint <= 0x17FF {
        return Some("Khmer".to_string());
    }
    if codepoint <= 0x18FF {
        return Some("Mongolian".to_string());
    }
    if codepoint <= 0x194F {
        return Some("Limbu".to_string());
    }
    if codepoint <= 0x197F {
        return Some("Tai Le".to_string());
    }
    if codepoint <= 0x19FF {
        return Some("New Tai Lue".to_string());
    }
    if codepoint <= 0x1A5F {
        return Some("Khmer Symbols".to_string());
    }
    if codepoint <= 0x1AFF {
        return Some("Buginese".to_string());
    }
    if codepoint <= 0x1B7F {
        return Some("Balinese".to_string());
    }
    if codepoint <= 0x1BBF {
        return Some("Sundanese".to_string());
    }
    if codepoint <= 0x1C4F {
        return Some("Lepcha".to_string());
    }
    if codepoint <= 0x1C7F {
        return Some("Ol Chiki".to_string());
    }
    if codepoint <= 0x1CCF {
        return Some("Sundanese Supplement".to_string());
    }
    if codepoint <= 0x1D7F {
        return Some("Vedic Extensions".to_string());
    }
    if codepoint <= 0x1DFF {
        return Some("Phonetic Extensions".to_string());
    }
    if codepoint <= 0x1EFF {
        return Some("Phonetic Extensions Supplement".to_string());
    }
    if codepoint <= 0x20FF {
        return Some("General Punctuation".to_string());
    }
    if codepoint <= 0x218F {
        return Some("Superscripts and Subscripts".to_string());
    }
    if codepoint <= 0x23FF {
        return Some("Currency Symbols".to_string());
    }
    if codepoint <= 0x24FF {
        return Some("Combining Diacritical Marks for Symbols".to_string());
    }
    if codepoint <= 0x257F {
        return Some("Letterlike Symbols".to_string());
    }
    if codepoint <= 0x267F {
        return Some("Number Forms".to_string());
    }
    if codepoint <= 0x273F {
        return Some("Arrows".to_string());
    }
    if codepoint <= 0x27BF {
        return Some("Mathematical Operators".to_string());
    }
    if codepoint <= 0x28FF {
        return Some("Miscellaneous Technical".to_string());
    }
    if codepoint <= 0x29FF {
        return Some("Control Pictures".to_string());
    }
    if codepoint <= 0x2B7F {
        return Some("Optical Character Recognition".to_string());
    }
    if codepoint <= 0x2BFF {
        return Some("Enclosed Alphanumerics".to_string());
    }
    if codepoint <= 0x2C7F {
        return Some("Box Drawing".to_string());
    }
    if codepoint <= 0x2D7F {
        return Some("Block Elements".to_string());
    }
    if codepoint <= 0x2E7F {
        return Some("Geometric Shapes".to_string());
    }
    if codepoint <= 0x2FFF {
        return Some("Miscellaneous Symbols".to_string());
    }
    if codepoint <= 0x31FF {
        return Some("Dingbats".to_string());
    }
    if codepoint <= 0x32FF {
        return Some("Miscellaneous Mathematical Symbols-A".to_string());
    }
    if codepoint <= 0x337F {
        return Some("Supplemental Arrows-A".to_string());
    }
    if codepoint <= 0x33BF {
        return Some("Braille Patterns".to_string());
    }
    if codepoint <= 0x33FF {
        return Some("Supplemental Arrows-B".to_string());
    }
    if codepoint <= 0x34FF {
        return Some("Miscellaneous Mathematical Symbols-B".to_string());
    }
    if codepoint <= 0x35FF {
        return Some("Supplemental Mathematical Operators".to_string());
    }
    if codepoint <= 0x367F {
        return Some("Miscellaneous Symbols and Arrows".to_string());
    }
    if codepoint <= 0x4DBF {
        return Some("CJK Radicals Supplement".to_string());
    }
    if codepoint <= 0x9FFF {
        return Some("CJK Unified Ideographs".to_string());
    }
    if codepoint <= 0xA4CF {
        return Some("Yi Syllables".to_string());
    }
    if codepoint <= 0xA6FF {
        return Some("Yi Radicals".to_string());
    }
    if codepoint <= 0xA71F {
        return Some("Modifier Tone Letters".to_string());
    }
    if codepoint <= 0xA82F {
        return Some("Latin Extended-D".to_string());
    }
    if codepoint <= 0xA87F {
        return Some("Syloti Nagri".to_string());
    }
    if codepoint <= 0xA95F {
        return Some("Phags-pa".to_string());
    }
    if codepoint <= 0xA97F {
        return Some("Hangul Jamo Extended-B".to_string());
    }
    if codepoint <= 0xACFF {
        return Some("Hangul Syllables".to_string());
    }
    if codepoint <= 0xD7FF {
        return Some("Hangul Jamo Extended-A".to_string());
    }
    if codepoint <= 0xDB7F {
        return Some("High Surrogates".to_string());
    }
    if codepoint <= 0xDCFF {
        return Some("High Private Use Surrogates".to_string());
    }
    if codepoint <= 0xDFFF {
        return Some("Low Surrogates".to_string());
    }
    if codepoint <= 0xEFFF {
        return Some("Private Use Area".to_string());
    }
    if codepoint <= 0xF8FF {
        return Some("CJK Compatibility Ideographs".to_string());
    }
    if codepoint <= 0xFB7F {
        return Some("Alphabetic Presentation Forms".to_string());
    }
    if codepoint <= 0xFEFF {
        return Some("Arabic Presentation Forms-A".to_string());
    }
    if codepoint <= 0xFFEF {
        return Some("Variation Selectors".to_string());
    }
    if codepoint <= 0xFFFF {
        return Some("Vertical Forms".to_string());
    }
    None
}

fn load_font_data(lib: &FontLibrary, font_id: crate::fonts::FontId) -> Option<(Vec<u8>, u32)> {
    let face = lib.face(font_id)?;
    Some((std::fs::read(&face.path).ok()?, face.index))
}

pub fn query_glyphs(
    lib: &FontLibrary,
    q: &GlyphQuery,
    recent: &RecentGlyphs,
) -> Result<GlyphPage, CommandError> {
    let (data, index) = load_font_data(lib, q.font).ok_or_else(|| {
        CommandError::validation("FONT_NOT_FOUND")
    })?;

    let font_ref = FontRef::from_index(&data, index).map_err(|_| {
        CommandError::validation("INVALID_FONT")
    })?;

    let num_glyphs = font_ref.maxp().map(|m| m.num_glyphs()).unwrap_or(0);

    let mut all_glyphs: Vec<GlyphInfo> = Vec::new();

    let encoded = encoded_chars(&font_ref);

    for gid in 0..num_glyphs {
        let char_val = encoded.get(&(gid as u32)).copied();

        let name = get_glyph_name(&font_ref, gid);

        all_glyphs.push(GlyphInfo {
            gid: gid as u16,
            unicode: char_val,
            name,
        });
    }

    let filtered: Vec<GlyphInfo> = match &q.subset {
        Subset::All => all_glyphs.clone(),
        Subset::Recent => {
            let recent_set: std::collections::HashSet<(crate::fonts::FontId, u16)> =
                recent.items.iter().cloned().collect();
            all_glyphs
                .into_iter()
                .filter(|g| recent_set.contains(&(q.font, g.gid)))
                .collect()
        }
        Subset::Block(block_name) => {
            let block_lower = block_name.to_lowercase();
            all_glyphs
                .into_iter()
                .filter(|g| {
                    if let Some(ch) = g.unicode {
                        unicode_block_name(ch as u32)
                            .map(|b| b.to_lowercase() == block_lower)
                            .unwrap_or(false)
                    } else {
                        false
                    }
                })
                .collect()
        }
    };

    let search_filtered: Vec<GlyphInfo> = if let Some(ref search_str) = q.search {
        let search_lower = search_str.to_lowercase();
        filtered
            .into_iter()
            .filter(|g| {
                if let Some(ch) = g.unicode {
                    let cp_hex = format!("U+{:04X}", ch as u32);
                    if cp_hex.eq_ignore_ascii_case(search_str) {
                        return true;
                    }
                    if search_codepoint(search_str) == Some(ch) {
                        return true;
                    }
                }
                if let Some(ref name) = g.name {
                    if name.to_lowercase().contains(&search_lower) {
                        return true;
                    }
                }
                false
            })
            .collect()
    } else {
        filtered
    };

    let mut search_filtered = search_filtered;
    if let Some(target) = q.search.as_deref().and_then(search_codepoint) {
        search_filtered.sort_by_key(|g| g.unicode != Some(target));
    }

    let total = search_filtered.len();
    let start = q.offset.min(total);
    let end = (start + q.limit).min(total);
    let items: Vec<GlyphInfo> = search_filtered[start..end].to_vec();
    register_encoded_glyphs(q.font, &items);

    Ok(GlyphPage { total, items })
}

fn encoded_chars(font_ref: &FontRef) -> HashMap<u32, char> {
    let mut map: HashMap<u32, char> = HashMap::new();
    for (unicode, gid) in font_ref.charmap().mappings() {
        let Some(ch) = std::char::from_u32(unicode) else {
            continue;
        };
        map.entry(gid.to_u32()).and_modify(|current| if ch < *current { *current = ch }).or_insert(ch);
    }
    map
}

fn search_codepoint(search: &str) -> Option<char> {
    let trimmed = search.trim();
    let hex = trimmed.strip_prefix("U+").or_else(|| trimmed.strip_prefix("u+"));
    if let Some(hex) = hex {
        return u32::from_str_radix(hex, 16).ok().and_then(std::char::from_u32);
    }
    let mut chars = trimmed.chars();
    let first = chars.next()?;
    chars.next().is_none().then_some(first)
}

pub fn register_encoded_glyphs(font: crate::fonts::FontId, items: &[GlyphInfo]) {
    for item in items {
        if let Some(ch) = item.unicode {
            dtool_core::text_cmd::register_glyph_char(u64::from(font.0), item.gid, ch);
        }
    }
}

pub fn glyph_insert_command(story: u64, at: usize, font: crate::fonts::FontId, glyph: &GlyphInfo) -> dtool_core::Command {
    let mut command = dtool_core::Command::new("text.glyph.insert");
    command.params["story"] = story.into();
    command.params["at"] = at.into();
    command.params["font"] = u64::from(font.0).into();
    command.params["gid"] = glyph.gid.into();
    if let Some(ch) = glyph.unicode {
        command.params["char"] = ch.to_string().into();
    }
    command
}

fn get_glyph_name(font_ref: &FontRef, gid: u16) -> Option<String> {
    let post = font_ref.post().ok()?;
    post.glyph_name(GlyphId16::new(gid)).map(str::to_string)
}

pub fn get_subsets(lib: &FontLibrary, font_id: crate::fonts::FontId) -> Vec<String> {
    let (data, index) = match load_font_data(lib, font_id) {
        Some(d) => d,
        None => return vec![],
    };

    let font_ref = match FontRef::from_index(&data, index) {
        Ok(f) => f,
        Err(_) => return vec![],
    };

    unicode_blocks_present(&font_ref)
}

pub fn resolve_glyph_char(lib: &FontLibrary, font_id: crate::fonts::FontId, gid: u16) -> Option<char> {
    let face = lib.face(font_id)?;
    let data = std::fs::read(&face.path).ok()?;
    let font_ref = FontRef::from_index(&data, face.index).ok()?;
    encoded_chars(&font_ref).get(&u32::from(gid)).copied()
}
