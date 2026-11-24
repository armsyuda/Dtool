use dtool_design::icons::tool_icon_for;
use dtool_strings::text;
use dtool_ui::icon_cache::IconCache;
use dtool_ui::tools_panel::{tool_column, tool_column_icons, tool_column_ids};
use egui::{Context, Pos2, RawInput, Rect, Shape, TextureId};

fn frame(ctx: &Context, mut add: impl FnMut(&mut egui::Ui)) -> (Vec<TextureId>, Vec<String>) {
    let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(300.0, 4000.0))), ..Default::default() };
    let mut full = ctx.run_ui(input, |ui| add(ui));
    full.textures_delta.clear();
    let (mut ids, mut texts) = (Vec::new(), Vec::new());
    for clipped in full.shapes {
        collect(&clipped.shape, &mut ids, &mut texts);
    }
    (ids, texts)
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

#[test]
fn tools_column_draws_icons_for_move_and_knife() {
    assert!(tool_icon_for("tool.move").is_some());
    assert!(tool_icon_for("tool.knife").is_some());
    let ctx = Context::default();
    let mut icons = IconCache::new();
    let mut shown = (Vec::new(), Vec::new());
    for _ in 0..2 {
        shown = frame(&ctx, |ui| {
            tool_column_icons(ui, "tool.move", &mut icons);
        });
    }
    let (ids, texts) = shown;
    let colour = ctx.style_of(ctx.theme()).visuals.selection.stroke.color;
    let px = (dtool_design::size("icon.size.md") * ctx.pixels_per_point()).ceil() as u32;
    let move_texture = icons.tool_texture(&ctx, "tool.move", colour, px).expect("move icon texture");
    assert!(ids.contains(&move_texture.id()), "{ids:?}");
    assert!(!texts.iter().any(|t| t == text("tool.knife")), "{texts:?}");
    assert!(!texts.iter().any(|t| t == text("tool.move")), "{texts:?}");
    let mapped = tool_column_ids("tool.move").iter().filter(|id| tool_icon_for(id).is_some()).count();
    assert_eq!(icons.len(), mapped);
}

#[test]
fn icon_cache_renders_each_tool_once_across_frames() {
    let ctx = Context::default();
    let mut icons = IconCache::new();
    frame(&ctx, |ui| {
        tool_column_icons(ui, "tool.move", &mut icons);
    });
    let first = icons.renders();
    frame(&ctx, |ui| {
        tool_column_icons(ui, "tool.move", &mut icons);
    });
    assert!(first > 0);
    assert_eq!(icons.renders(), first);
}

#[test]
fn plain_tool_column_keeps_text_labels() {
    let ctx = Context::default();
    let (ids, texts) = frame(&ctx, |ui| {
        tool_column(ui, "tool.move");
    });
    assert!(texts.iter().any(|t| t == text("tool.move")), "{texts:?}");
    assert!(ids.iter().all(|id| *id == TextureId::default()), "{ids:?}");
}
