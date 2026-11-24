use std::path::Path;
use std::sync::Arc;

pub const PRETENDARD: &[u8] = include_bytes!("../assets/fonts/pretendard/Pretendard-Regular.ttf");
pub const INTER: &[u8] = include_bytes!("../assets/fonts/inter/Inter-Regular.ttf");
pub const PRETENDARD_ID: &str = "pretendard";
pub const INTER_ID: &str = "inter";
pub const MALGUN_ID: &str = "malgun";

pub fn ui_families() -> Vec<String> {
    vec![dtool_design::font_family("ui_latin"), dtool_design::font_family("ui"), dtool_design::font_family("ui_fallback")]
}

pub fn bundled_font(family: &str) -> Option<(&'static str, &'static [u8])> {
    if family.eq_ignore_ascii_case(PRETENDARD_ID) {
        Some((PRETENDARD_ID, PRETENDARD))
    } else if family.eq_ignore_ascii_case(INTER_ID) {
        Some((INTER_ID, INTER))
    } else {
        None
    }
}

pub fn token_font_definitions(families: &[String], fallback: Option<Vec<u8>>) -> (egui::FontDefinitions, usize) {
    let mut fonts = egui::FontDefinitions::default();
    let mut order: Vec<String> = Vec::new();
    let mut fallback = fallback;
    for family in families {
        if let Some((id, bytes)) = bundled_font(family) {
            if !order.iter().any(|known| known == id) {
                fonts.font_data.insert(id.to_owned(), Arc::new(egui::FontData::from_static(bytes)));
                order.push(id.to_owned());
            }
        } else if family.eq_ignore_ascii_case(MALGUN_ID) {
            if let Some(data) = fallback.take() {
                fonts.font_data.insert(MALGUN_ID.to_owned(), Arc::new(egui::FontData::from_owned(data)));
                order.push(MALGUN_ID.to_owned());
            }
        }
    }
    if let Some(data) = fallback.take() {
        fonts.font_data.insert(MALGUN_ID.to_owned(), Arc::new(egui::FontData::from_owned(data)));
        order.push(MALGUN_ID.to_owned());
    }
    let proportional = fonts.families.entry(egui::FontFamily::Proportional).or_default();
    for (index, id) in order.iter().enumerate() {
        proportional.insert(index, id.clone());
    }
    let monospace = fonts.families.entry(egui::FontFamily::Monospace).or_default();
    for id in order.iter().filter(|id| id.as_str() != INTER_ID) {
        monospace.push(id.clone());
    }
    let loaded = order.len();
    (fonts, loaded)
}

pub fn ui_font_definitions(malgun: Option<Vec<u8>>) -> egui::FontDefinitions {
    token_font_definitions(&ui_families(), malgun).0
}

pub fn install_ui_fonts(ctx: &egui::Context, fallback_path: &Path) -> bool {
    let fallback = std::fs::read(fallback_path).ok();
    let (fonts, loaded) = token_font_definitions(&ui_families(), fallback);
    if loaded == 0 {
        return false;
    }
    ctx.set_fonts(fonts);
    true
}

pub fn install_korean_font(ctx: &egui::Context) -> bool {
    install_ui_fonts(ctx, Path::new("C:\\Windows\\Fonts\\malgun.ttf"))
}
