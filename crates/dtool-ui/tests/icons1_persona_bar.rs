use dtool_core::document::{Document, Units};
use dtool_core::Session;
use dtool_strings::text;
use dtool_ui::icon_cache::IconCache;
use dtool_ui::persona_bar::{persona_icon_for, pick, pick_icons, show_restored_controls, show_restored_controls_icons};
use dtool_ui::tools_panel::ToolId;
use egui::{Context, Pos2, RawInput, Rect, Shape, TextureId};
use serde_json::json;

fn frame(ctx: &Context, mut add: impl FnMut(&mut egui::Ui)) -> (Vec<TextureId>, Vec<String>) {
    let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1200.0, 400.0))), ..Default::default() };
    let mut full = ctx.run_ui(input, |ui| add(ui));
    full.textures_delta.clear();
    let (mut ids, mut texts) = (Vec::new(), Vec::new());
    for clipped in full.shapes {
        collect(&clipped.shape, &mut ids, &mut texts);
    }
    (ids, texts)
}

fn selected_ink(ctx: &Context) -> egui::Color32 {
    match ctx.theme() {
        egui::Theme::Dark => dtool_design::theme::dark().text,
        egui::Theme::Light => dtool_design::theme::light().text,
    }
}

fn collect(shape: &Shape, ids: &mut Vec<TextureId>, texts: &mut Vec<String>) {
    match shape {
        Shape::Mesh(mesh) => ids.push(mesh.texture_id),
        Shape::Rect(rect) => {
            if let Some(brush) = &rect.brush {
                ids.push(brush.fill_texture_id);
            }
        }
        Shape::Text(t) => texts.push(t.galley.text().to_string()),
        Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, ids, texts)),
        _ => {}
    }
}

fn texture(icons: &mut IconCache, ctx: &Context, id: &str, selected: bool) -> TextureId {
    let px = (dtool_design::size("icon.size.md") * ctx.pixels_per_point()).ceil() as u32;
    let colour = selected.then(|| selected_ink(ctx)).unwrap_or_else(dtool_design::icons::current_icon_colour);
    icons.texture(ctx, id, colour, px).expect("icon texture").id()
}

fn session() -> Session {
    let mut s = Session::new(Document::new([800.0, 600.0], Units::Px, 96.0, 1));
    s.active_tool = "tool.move".to_string();
    s
}

#[test]
fn persona_icons_map_builtins_and_home() {
    assert_eq!(persona_icon_for("vector"), Some("persona-vector"));
    assert_eq!(persona_icon_for("pixel"), Some("persona-pixel"));
    assert_eq!(persona_icon_for("layout"), Some("persona-layout"));
    assert_eq!(persona_icon_for("home"), Some("persona-home"));
}

#[test]
fn persona_bar_draws_icons_for_personas_and_home() {
    let ctx = Context::default();
    let mut icons = IconCache::new();
    let current = json!({"id": "vector", "in_sub_persona": false});
    let (ids, texts) = frame(&ctx, |ui| {
        pick_icons(ui, Some(&current), &mut icons);
    });
    for key in ["vector", "pixel", "layout", "slice"] {
        let id = texture(&mut icons, &ctx, &format!("persona-{key}"), key == "vector");
        assert!(ids.contains(&id), "{key} {ids:?}");
        assert!(texts.iter().any(|t| *t == text(&format!("persona.{key}"))), "{key} {texts:?}");
    }
    assert!(ids.contains(&texture(&mut icons, &ctx, "app-logo", false)), "{ids:?}");
    assert!(!texts.iter().any(|t| *t == text("persona.home")), "{texts:?}");
}

#[test]
fn restored_tools_draw_icons_for_move_and_knife() {
    let ctx = Context::default();
    let mut icons = IconCache::new();
    let s = session();
    let tools = [ToolId("tool.move"), ToolId("tool.knife")];
    let (ids, texts) = frame(&ctx, |ui| {
        show_restored_controls_icons(ui, &s, &tools, &[], &mut icons);
    });
    let colour = selected_ink(&ctx);
    let px = (dtool_design::size("icon.size.md") * ctx.pixels_per_point()).ceil() as u32;
    let move_id = icons.tool_texture(&ctx, "tool.move", colour, px).unwrap().id();
    assert!(ids.contains(&move_id), "{ids:?}");
    assert!(!texts.iter().any(|t| *t == text("tool.knife")), "{texts:?}");
    assert!(!texts.iter().any(|t| *t == text("tool.move")), "{texts:?}");
}

#[test]
fn cache_is_reused_across_frames() {
    let ctx = Context::default();
    let mut icons = IconCache::new();
    let s = session();
    let tools = [ToolId("tool.move"), ToolId("tool.knife")];
    let current = json!({"id": "pixel"});
    let draw = |icons: &mut IconCache| {
        frame(&ctx, |ui| {
            pick_icons(ui, Some(&current), icons);
            show_restored_controls_icons(ui, &s, &tools, &[], icons);
        });
    };
    draw(&mut icons);
    let (renders, len) = (icons.renders(), icons.len());
    draw(&mut icons);
    draw(&mut icons);
    assert_eq!(len, 9);
    assert_eq!(icons.renders(), renders);
    assert_eq!(icons.len(), len);
}

#[test]
fn old_functions_stay_text_only() {
    let ctx = Context::default();
    let s = session();
    let tools = [ToolId("tool.move")];
    let (ids, texts) = frame(&ctx, |ui| {
        pick(ui, None);
        show_restored_controls(ui, &s, &tools, &[]);
    });
    assert!(texts.iter().any(|t| *t == text("persona.vector")), "{texts:?}");
    assert!(texts.iter().any(|t| *t == text("tool.move")), "{texts:?}");
    assert!(ids.iter().all(|id| *id == TextureId::default()), "{ids:?}");
}
