#[path = "support/test_home.rs"] mod test_home;
use dtool_ui::{dock::{DockSide, PanelId}, panels::layers::LayersPanel, presets::{default_layout, WorkspacePersona}, shell::AppShell};
use serde_json::json;

fn texts(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
    match shape {
        egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.visual_bounding_rect())),
        egui::Shape::Vec(items) => for item in items { texts(item, out); },
        _ => {}
    }
}

#[test]
fn no_document_shell_retains_header_tools_and_every_studio_title() {
    let _home = test_home::enter("common_studio_no_doc");
    let ctx = egui::Context::default();
    let mut shell = AppShell::new();
    dtool_design::apply(&ctx, &dtool_design::dark());
    let mut labels = Vec::new();
    for _ in 0..2 {
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1536.0, 864.0))), ..Default::default() }, |ui| {
            dtool_ui::menu_tree::show_queried_menu_bar_recent(ui, None, "vector", &[], &[]);
            shell.show_persona_header(ui);
            shell.show_context_region(ui);
            shell.show_tools_column(ui);
            shell.show_dock(ui);
        });
        labels.clear();
        for shape in &output.shapes { texts(&shape.shape, &mut labels); }
        output.textures_delta.clear();
    }
    assert!(shell.workspace.session().is_none());
    assert!(shell.tools_column_visible());
    assert!(!shell.icon_cache.is_empty());
    let at = |key: &str| labels.iter().find(|(s,_)| s == dtool_strings::text(key)).unwrap_or_else(|| panic!("missing {key}: {labels:?}")).1;
    let menu = at("menu.file");
    let persona = labels.iter().find(|(s,r)| s == dtool_strings::text("persona.vector") && r.top() > menu.bottom()).expect("persona chip below menus").1;
    let swatches = at("panel.swatches");
    assert!(menu.top() < persona.top() && persona.bottom() < swatches.top(), "menu={menu:?} persona={persona:?} swatches={swatches:?}");
    for key in ["panel.colour", "panel.layers", "panel.stroke", "panel.character", "panel.paragraph", "panel.assets", "panel.styles"] { at(key); }
    assert!(labels.iter().all(|(s,_)| !s.starts_with("panel.")), "raw panel id: {labels:?}");
}

#[test]
fn default_studios_are_split_and_saving_retains_the_groups() {
    use egui_tiles::{Container, Tile};
    for persona in [WorkspacePersona::Vector, WorkspacePersona::Pixel, WorkspacePersona::Layout] {
        let layout = default_layout(persona);
        assert_eq!(layout.side_of(&PanelId::Layers), Some(DockSide::Right));
        let mut tree = layout.tree(DockSide::Right).unwrap();
        let Some(Tile::Container(Container::Linear(root))) = tree.tiles.get(tree.root.unwrap()) else { panic!("no column split"); };
        assert_eq!(root.children.len(), 2);
        for column in &root.children {
            let Some(Tile::Container(Container::Linear(groups))) = tree.tiles.get(*column) else { panic!("no studio groups"); };
            assert!(groups.children.len() >= 3);
        }
        let ctx = egui::Context::default();
        let mut registry = dtool_ui::panels::PanelRegistry::with_existing_bodies();
        let mut release = None;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| { tree.ui(&mut dtool_ui::panels::DockBehavior { registry: &mut registry, header_release: &mut release }, ui); });
        output.textures_delta.clear();
        for (_, tile) in tree.tiles.iter() {
            if let Tile::Pane(pane) = tile {
                assert!(tree.tiles.iter().any(|(_,parent)| matches!(parent, Tile::Container(Container::Tabs(t)) if t.children.iter().any(|id| matches!(tree.tiles.get(*id), Some(Tile::Pane(p)) if p == pane)))));
            }
        }
    }
}

#[test]
fn layer_visibility_and_lock_buttons_change_live_nodes_with_one_undo() {
    let _home = test_home::enter("common_layer_buttons");
    let mut shell = AppShell::new();
    shell.workspace.new_document("Layers");
    let s = shell.workspace.session_mut().unwrap();
    s.run_named("object.create", json!({"name":"Layer A", "pos":[10,10], "size":[40,30]})).unwrap();
    let id = s.document.objects.last().unwrap().id.0;
    let mut panel = LayersPanel::default();
    for lock in [false, true] {
        let before = shell.workspace.session().unwrap().document.clone();
        let depth = shell.workspace.session().unwrap().undo.applied_steps();
        assert!(if lock { panel.set_locked(&mut shell.workspace,id,true) } else { panel.set_visible(&mut shell.workspace,id,false) });
        let s = shell.workspace.session_mut().unwrap();
        assert_eq!(s.undo.applied_steps(), depth + 1);
        if lock { assert!(s.document.objects.last().unwrap().locked); } else { assert!(!s.document.objects.last().unwrap().pickable); }
        s.run_named("edit.undo",json!({})).unwrap();
        assert_eq!(serde_json::to_value(&s.document.objects).unwrap(), serde_json::to_value(&before.objects).unwrap());
    }
}

#[test]
fn layer_rows_draw_and_reuse_real_image_thumbnails() {
    let _home = test_home::enter("common_layer_thumbnails");
    dtool_render::preview::install();
    let mut shell = AppShell::new(); shell.workspace.new_document("Preview");
    shell.workspace.session_mut().unwrap().run_named("object.create", json!({"name":"Red", "pos":[10,10], "size":[40,30], "fill":"#FF0000"})).unwrap();
    let ctx = egui::Context::default();
    let mut panel = LayersPanel::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| panel.show(ui, &mut shell.workspace));
    assert_eq!(panel.thumbnails.len(),1);
    let first = panel.thumbnails.values().next().unwrap().as_ref().expect("real thumbnail").id();
    let uploaded = out.textures_delta.set.get(&first).and_then(|updates| updates.last()).expect("thumbnail pixels uploaded");
    let egui::ImageData::Color(image) = &uploaded.image;
    assert!(image.pixels.iter().any(|p| p.r() > 200 && p.g() < 30 && p.b() < 30 && p.a() > 200), "thumbnail must contain the real red shape");
    out.textures_delta.clear();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| panel.show(ui, &mut shell.workspace)); out.textures_delta.clear();
    assert_eq!(panel.thumbnails.values().next().unwrap().as_ref().unwrap().id(), first);
}
