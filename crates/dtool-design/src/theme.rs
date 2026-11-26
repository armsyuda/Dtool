use std::sync::atomic::{AtomicU8, Ordering};

use egui::{Color32, Visuals};

use crate::tokens::{color_in, stroke_width};

static STYLE: AtomicU8 = AtomicU8::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeStyle {
    Dark,
    Light,
}

pub fn set_style(style: ThemeStyle) {
    STYLE.store(
        match style {
            ThemeStyle::Dark => 0,
            ThemeStyle::Light => 1,
        },
        Ordering::Relaxed,
    );
}

pub fn current_style() -> ThemeStyle {
    if STYLE.load(Ordering::Relaxed) == 1 {
        ThemeStyle::Light
    } else {
        ThemeStyle::Dark
    }
}

pub fn style_name(style: ThemeStyle) -> &'static str {
    match style {
        ThemeStyle::Dark => "dark",
        ThemeStyle::Light => "light",
    }
}

pub fn current_style_name() -> &'static str {
    style_name(current_style())
}

#[derive(Clone, Debug)]
pub struct Palette {
    pub style: ThemeStyle,
    pub menu_bar: Color32,
    pub bars: Color32,
    pub context_bar: Color32,
    pub tab_row: Color32,
    pub pasteboard: Color32,
    pub panel: Color32,
    pub dialog: Color32,
    pub text: Color32,
    pub accent: Color32,
    pub high_contrast: bool,
}

fn palette_from_style(style: ThemeStyle) -> Palette {
    let s = style_name(style);
    Palette {
        style,
        menu_bar: color_in(s, "menu_bar"),
        bars: color_in(s, "bars"),
        context_bar: color_in(s, "context_bar"),
        tab_row: color_in(s, "tab_row"),
        pasteboard: color_in(s, "pasteboard"),
        panel: color_in(s, "panel"),
        dialog: color_in(s, "dialog.bg"),
        text: color_in(s, "text"),
        accent: color_in(s, "accent"),
        high_contrast: false,
    }
}

pub fn dark() -> Palette {
    palette_from_style(ThemeStyle::Dark)
}

pub fn light() -> Palette {
    palette_from_style(ThemeStyle::Light)
}

pub struct ThemeAdjust {
    pub style: ThemeStyle,
    pub background_grey: f32,
    pub ui_brightness: f32,
    pub text_contrast: f32,
    pub high_contrast: bool,
}

fn pasteboard_from_grey(style: ThemeStyle, background_grey: f32) -> Color32 {
    let base = color_in(style_name(style), "pasteboard");
    let base_v = (base.r() as f32 + base.g() as f32 + base.b() as f32) / (3.0 * 255.0);
    let grey_factor = (base_v + (background_grey - 0.25) * 0.5).clamp(0.0, 1.0);
    let v = (grey_factor * 255.0).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(v, v, v)
}

pub fn adjust(p: &Palette, settings: &ThemeAdjust) -> Palette {
    let s = style_name(settings.style);
    let brightness_factor = 0.6 + settings.ui_brightness * 0.8;
    let pasteboard = pasteboard_from_grey(settings.style, settings.background_grey);

    let scale_color = |c: Color32| {
        if settings.style == ThemeStyle::Light && brightness_factor > 1.0 {
            return c.lerp_to_gamma(Color32::WHITE, brightness_factor - 1.0);
        }
        let r = (c.r() as f32 * brightness_factor).clamp(0.0, 255.0) as u8;
        let g = (c.g() as f32 * brightness_factor).clamp(0.0, 255.0) as u8;
        let b = (c.b() as f32 * brightness_factor).clamp(0.0, 255.0) as u8;
        Color32::from_rgb(r, g, b)
    };

    let text_color = if settings.high_contrast {
        match settings.style {
            ThemeStyle::Dark => color_in(s, "color.high_contrast.dark_text"),
            ThemeStyle::Light => color_in(s, "color.high_contrast.light_text"),
        }
    } else {
        let grey = color_in(s, "color.contrast.mid");
        let full_text = p.text;
        let t = settings.text_contrast.clamp(0.0, 1.0);
        let r = (grey.r() as f32 * (1.0 - t) + full_text.r() as f32 * t).clamp(0.0, 255.0) as u8;
        let g = (grey.g() as f32 * (1.0 - t) + full_text.g() as f32 * t).clamp(0.0, 255.0) as u8;
        let b = (grey.b() as f32 * (1.0 - t) + full_text.b() as f32 * t).clamp(0.0, 255.0) as u8;
        Color32::from_rgb(r, g, b)
    };

    Palette {
        style: settings.style,
        menu_bar: scale_color(p.menu_bar),
        bars: scale_color(p.bars),
        context_bar: scale_color(p.context_bar),
        tab_row: scale_color(p.tab_row),
        pasteboard,
        panel: scale_color(p.panel),
        dialog: scale_color(p.dialog),
        text: text_color,
        accent: p.accent,
        high_contrast: settings.high_contrast,
    }
}

pub fn palette_for(settings: &ThemeAdjust) -> Palette {
    let base = match settings.style {
        ThemeStyle::Dark => dark(),
        ThemeStyle::Light => light(),
    };
    adjust(&base, settings)
}

pub fn widget_token(name: &str) -> Option<Color32> {
    widget_token_for(current_style(), name)
}

pub fn widget_token_for(style: ThemeStyle, name: &str) -> Option<Color32> {
    let key = format!("color.{}.widget.{name}", style_name(style));
    crate::tokens::has(&key).then(|| color_in(style_name(style), &key))
}

fn apply_widget_tokens(visuals: &mut Visuals, style: ThemeStyle) {
    if let Some(fill) = widget_token_for(style, "bg") {
        visuals.widgets.inactive.bg_fill = fill;
        visuals.widgets.inactive.weak_bg_fill = fill;
    }
    if let Some(fill) = widget_token_for(style, "bg_hover") {
        visuals.widgets.hovered.bg_fill = fill;
        visuals.widgets.hovered.weak_bg_fill = fill;
    }
    if let Some(fill) = widget_token_for(style, "bg_active") {
        visuals.widgets.active.bg_fill = fill;
        visuals.widgets.active.weak_bg_fill = fill;
    }
    if let Some(fill) = widget_token_for(style, "bg_disabled") {
        visuals.widgets.noninteractive.weak_bg_fill = fill;
    }
}

pub fn surface(ctx: &egui::Context, key: &str) -> Color32 {
    let applied = ctx.data(|data| data.get_temp::<Palette>(egui::Id::NULL));
    if let Some(palette) = applied {
        match key {
            "menu_bar" => return palette.menu_bar,
            "bars" => return palette.bars,
            "context_bar" => return palette.context_bar,
            "tab_row" => return palette.tab_row,
            "pasteboard" => return palette.pasteboard,
            "panel.bg" => return palette.panel,
            "dialog.bg" => return palette.dialog,
            _ => {}
        }
    }
    crate::tokens::color(key)
}

/// Keep tooltip text within the viewport, including the frame and its margins.
pub fn fit_tooltips_to_viewport(ctx: &egui::Context) {
    let viewport_width = (ctx.content_rect().width() - 24.0).max(1.0);
    let width = egui::Spacing::default().tooltip_width.min(viewport_width);
    if ctx.global_style().spacing.tooltip_width != width {
        ctx.global_style_mut(|style| style.spacing.tooltip_width = width);
    }
}

pub fn apply(ctx: &egui::Context, palette: &Palette) {
    set_style(palette.style);
    ctx.data_mut(|data| data.insert_temp(egui::Id::NULL, palette.clone()));
    let mut visuals = match palette.style {
        ThemeStyle::Dark => Visuals::dark(),
        ThemeStyle::Light => Visuals::light(),
    };
    visuals.panel_fill = palette.panel;
    visuals.override_text_color = Some(palette.text);
    visuals.selection.bg_fill = palette.accent;
    let rgb = egui::Rgba::from(palette.accent);
    let luminance = 0.2126 * rgb.r() + 0.7152 * rgb.g() + 0.0722 * rgb.b();
    let selected_text = if luminance > 0.179 { Color32::BLACK } else { Color32::WHITE };
    visuals.selection.stroke = egui::Stroke::new(stroke_width("thin"), selected_text);
    apply_widget_tokens(&mut visuals, palette.style);
    if crate::tokens::has(&format!("color.{}.input", style_name(palette.style))) {
        visuals.extreme_bg_color = color_in(style_name(palette.style), "input");
    }
    visuals.text_edit_bg_color = Some(visuals.extreme_bg_color);
    visuals.window_fill = palette.dialog;
    visuals.window_stroke = egui::Stroke::new(stroke_width("thin"), color_in(style_name(palette.style), "border"));
    visuals.window_corner_radius = egui::CornerRadius::same(crate::tokens::radius("md") as u8);
    visuals.menu_corner_radius = egui::CornerRadius::same(crate::tokens::radius("sm") as u8);
    for widget in [&mut visuals.widgets.noninteractive, &mut visuals.widgets.inactive, &mut visuals.widgets.hovered, &mut visuals.widgets.active, &mut visuals.widgets.open] {
        widget.corner_radius = egui::CornerRadius::same(crate::tokens::size("radius.button").round() as u8);
        widget.bg_stroke = egui::Stroke::new(stroke_width("thin"), color_in(style_name(palette.style), if palette.high_contrast { "color.border.high_contrast" } else { "border" }));
    }
    let focus = widget_token_for(palette.style, "focus").unwrap_or(palette.accent);
    visuals.widgets.active.bg_stroke = egui::Stroke::new(stroke_width("thin"), focus);
    if let Some(disabled) = widget_token_for(palette.style, "fg_disabled") {
        visuals.widgets.noninteractive.fg_stroke.color = disabled;
    }
    ctx.set_visuals(visuals);
    ctx.style_mut_of(ctx.theme(), |style| {
        let input = crate::tokens::size("size.input");
        style.spacing.interact_size = egui::vec2(input, input);
        style.spacing.item_spacing = egui::vec2(crate::tokens::space("sm"), crate::tokens::space("sm"));
        style.spacing.button_padding = egui::vec2(crate::tokens::space("sm"), crate::tokens::space("xs"));
        style.spacing.menu_margin = egui::Margin::symmetric(crate::tokens::space("sm") as i8, crate::tokens::space("xs") as i8);
        style.spacing.slider_rail_height = crate::tokens::size("slider.rail.height");
        style.spacing.icon_width = crate::tokens::size("icon.size.sm");
        for text_style in [egui::TextStyle::Body, egui::TextStyle::Button, egui::TextStyle::Monospace] {
            let family = if text_style == egui::TextStyle::Monospace { egui::FontFamily::Monospace } else { egui::FontFamily::Proportional };
            style.text_styles.insert(text_style, egui::FontId::new(crate::tokens::font_size("ui"), family));
        }
        style.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(crate::tokens::font_size("small")));
        style.text_styles.insert(egui::TextStyle::Heading, egui::FontId::proportional(crate::tokens::font_size("title")));
    });
}
