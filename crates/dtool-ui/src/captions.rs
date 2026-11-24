use dtool_core::document::Units;
use dtool_strings::{has, text};

pub trait Caption {
    fn identity(&self) -> &'static str;
    fn caption_group(&self) -> &'static str;

    fn caption_key(&self) -> String {
        format!("caption.{}.{}", self.caption_group(), self.identity())
    }

    fn caption(&self) -> String {
        text(&self.caption_key()).to_string()
    }
}

impl Caption for Units {
    fn identity(&self) -> &'static str {
        units_identity(*self)
    }

    fn caption_group(&self) -> &'static str {
        "units"
    }
}

pub const ALL_UNITS: [Units; 5] = [Units::Px, Units::Mm, Units::Cm, Units::In, Units::Pt];

pub fn units_identity(units: Units) -> &'static str {
    match units {
        Units::Px => "px",
        Units::Mm => "mm",
        Units::Cm => "cm",
        Units::In => "in",
        Units::Pt => "pt",
    }
}

pub fn units_from_identity(identity: &str) -> Option<Units> {
    ALL_UNITS
        .iter()
        .copied()
        .find(|units| units_identity(*units) == identity)
}

pub fn units_caption(units: Units) -> String {
    units.caption()
}

pub fn units_options() -> Vec<(Units, String)> {
    ALL_UNITS
        .iter()
        .map(|units| (*units, units.caption()))
        .collect()
}

pub fn caption_for(group: &str, identity: &str) -> String {
    let key = format!("caption.{}.{}", group, identity);
    if has(&key) {
        text(&key).to_string()
    } else {
        key
    }
}

pub fn category_caption(category_id: &str) -> String {
    caption_for("category", category_id)
}

pub fn format_caption(format_id: &str) -> String {
    caption_for("format", format_id)
}

pub fn enum_caption(enum_name: &str, variant_id: &str) -> String {
    caption_for(enum_name, variant_id)
}

pub fn caption_options<'a>(group: &str, identities: &[&'a str]) -> Vec<(&'a str, String)> {
    identities
        .iter()
        .map(|identity| (*identity, caption_for(group, identity)))
        .collect()
}
