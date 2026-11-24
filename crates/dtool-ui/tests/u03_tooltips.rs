use dtool_strings::{format, has, text};
use dtool_ui::persona_bar::pick;
use egui::{Context, Event, Pos2, RawInput, Rect, Shape};

const SOURCES: [(&str, &str); 2] = [
    ("persona_bar.rs", include_str!("../src/persona_bar.rs")),
    ("panels/swatches.rs", include_str!("../src/panels/swatches.rs")),
];

fn texts(shape: &Shape, out: &mut Vec<String>) {
    match shape {
        Shape::Text(t) => out.push(t.galley.text().to_string()),
        Shape::Vec(list) => list.iter().for_each(|s| texts(s, out)),
        _ => {}
    }
}

fn frame(ctx: &Context, time: f64, events: Vec<Event>) -> Vec<String> {
    let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 200.0))), time: Some(time), events, ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| {
        pick(ui, None);
    });
    out.textures_delta.clear();
    let mut found = Vec::new();
    for clipped in &out.shapes {
        texts(&clipped.shape, &mut found);
    }
    found
}

#[test]
fn tooltips_go_through_design_helper() {
    for (name, src) in SOURCES {
        assert!(!src.contains(".on_hover_text("), "{name}");
        assert!(src.contains("dtool_design::tooltip("), "{name}");
    }
}

#[test]
fn persona_tooltip_keys_exist_and_format_without_placeholder() {
    for key in ["persona.home", "persona.tooltip", "persona.vector", "persona.pixel", "persona.layout"] {
        assert!(has(key), "{key}");
        assert_ne!(text(key), key);
    }
    let name = text("persona.vector");
    let tip = format("persona.tooltip", &[("name", name)]);
    assert!(tip.contains(name));
    assert!(!tip.contains("{name}"));
    assert!(!tip.contains("persona."));
}

#[test]
fn missing_key_lookup_returns_key_so_callers_must_check_has() {
    let key = "u03.tooltip.key_that_does_not_exist";
    assert!(!has(key));
    assert_eq!(text(key), key);
}

#[test]
fn hovering_persona_button_shows_localized_tooltip() {
    let ctx = Context::default();
    ctx.all_styles_mut(|s| {
        s.interaction.tooltip_delay = 0.0;
        s.interaction.show_tooltips_only_when_still = false;
    });
    let name = text("persona.vector").to_string();
    let tip = format("persona.tooltip", &[("name", name.as_str())]);
    let first = frame(&ctx, 0.0, vec![]);
    assert!(first.iter().any(|t| t == &name), "{first:?}");
    let mut seen = Vec::new();
    let mut shown = false;
    for x in (10..800).step_by(20) {
        for step in 0..3 {
            let found = frame(&ctx, 1.0 + x as f64 + step as f64 * 0.2, vec![Event::PointerMoved(Pos2::new(x as f32, 12.0))]);
            if found.iter().any(|t| t == &tip) {
                shown = true;
            }
            seen.extend(found.into_iter().filter(|t| t.contains("persona.") || t.contains("ui.persona")));
        }
        if shown {
            break;
        }
    }
    assert!(shown, "tooltip {tip:?} never shown");
    assert!(seen.is_empty(), "raw key or command id shown: {seen:?}");
}