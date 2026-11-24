#[path = "support/test_home.rs"]
mod test_home;
use std::sync::Arc;
use std::time::Duration;

use dtool_core::document::Units;
use dtool_core::workspace::{host_channel, ContextId, HostHandle, HostInbox, HostReply, Scope};
use dtool_core::Command;
use dtool_ui::shell::AppShell;
use serde_json::{json, Value};

struct Live {
    shell: AppShell,
    handle: HostHandle,
    inbox: HostInbox,
}

impl Live {
    fn new() -> Self {
        let (handle, inbox) = host_channel(Arc::new(|| {}));
        Live { shell: AppShell::new(), handle, inbox }
    }

    fn mcp(&mut self, scope: Scope, name: &str, params: Value) -> HostReply {
        let handle = self.handle.clone();
        let command = Command::with_params(name, params);
        let worker = std::thread::spawn(move || handle.request(scope, command, None, Duration::from_secs(20)));
        while !worker.is_finished() {
            self.inbox.drain(&mut self.shell.workspace, 32);
            std::thread::yield_now();
        }
        worker.join().unwrap()
    }

    fn new_doc(&mut self, width: f64, height: f64) -> ContextId {
        let reply = self.mcp(Scope::active(), "doc.new", json!({ "width": width, "height": height, "units": Units::Px, "dpi": 72.0, "pages": 1 }));
        assert!(reply.result.is_ok(), "doc.new: {:?}", reply.result.err());
        let id = reply.context.expect("doc.new context");
        assert_eq!(self.shell.workspace.active(), Some(id));
        id
    }
}
#[allow(dead_code)]
#[path = "../src/icon_cache.rs"]
mod icon_cache;

use dtool_design::icons::{colour_hex, icon_colour, themed_svg, tool_icon_for};
use dtool_design::theme::ThemeStyle;
use icon_cache::{rasterize_icon, rasterize_svg};

#[test]
fn live_mcp_tool_set_resolves_a_rendered_icon_for_the_scoped_document() {
    let _home = test_home::enter("live_mcp_tool_set_resolves_a_rendered_icon_for_the_scoped_document");
    let mut live = Live::new();
    let a = live.new_doc(200.0, 100.0);
    let b = live.new_doc(300.0, 300.0);
    let b_tool = live.shell.workspace.get(b).unwrap().session.active_tool.clone();
    let colour = dtool_design::icons::current_icon_colour();
    let mut mapped = 0;
    let mut fallback = Vec::new();
    for tool in dtool_core::commands::tools::TOOLS {
        let reply = live.mcp(Scope::of(a), "ui.tool.set", json!({ "id": tool }));
        assert_eq!(reply.context, Some(a));
        let set = reply.result.unwrap_or_else(|e| panic!("{tool}: {}", e.code));
        let active = live.shell.workspace.get(a).unwrap().session.active_tool.clone();
        assert_eq!(set["id"], active.as_str());
        let query = live.mcp(Scope::of(a), "ui.tool.query", json!({})).result.unwrap();
        assert_eq!(query["id"], active.as_str());
        match tool_icon_for(&active) {
            Some(icon) => {
                let raster = rasterize_icon(icon, colour, 24).unwrap_or_else(|| panic!("{tool} -> {icon} did not render"));
                assert!(raster.covered_pixels() > 0, "{tool} -> {icon} rendered blank");
                mapped += 1;
            }
            None => fallback.push(active),
        }
    }
    assert_eq!(live.shell.workspace.get(b).unwrap().session.active_tool, b_tool);
    assert_eq!(live.shell.workspace.active(), Some(b));
    assert!(mapped >= 39, "mapped {mapped}, fallback {fallback:?}");
    eprintln!("live tools mapped={mapped} text_fallback={}", fallback.len());
}

fn ink(rgba: &[u8]) -> Option<[u8; 3]> {
    rgba.chunks(4).filter(|p| p[3] >= 250).map(|p| [p[0], p[1], p[2]]).next()
}

#[test]
fn live_tool_icon_takes_the_theme_text_colour() {
    let _home = test_home::enter("live_tool_icon_takes_the_theme_text_colour");
    let mut live = Live::new();
    let a = live.new_doc(200.0, 100.0);
    assert!(live.mcp(Scope::of(a), "ui.tool.set", json!({ "id": "tool.pen" })).result.is_ok());
    let active = live.shell.workspace.get(a).unwrap().session.active_tool.clone();
    let icon = tool_icon_for(&active).expect("pen icon");
    for style in [ThemeStyle::Dark, ThemeStyle::Light] {
        let colour = icon_colour(style);
        let svg = themed_svg(icon, colour).unwrap();
        assert!(!svg.contains("currentColor"));
        assert!(svg.contains(&colour_hex(colour)));
        let raster = rasterize_svg(&svg, 48).expect("raster");
        let px = ink(&raster.rgba).expect("opaque ink");
        let want = [colour.r(), colour.g(), colour.b()];
        assert!(px.iter().zip(want).all(|(a, b)| (*a as i32 - b as i32).abs() <= 2), "{style:?} ink {px:?} want {want:?}");
    }
}

#[test]
fn live_tools_panel_draws_cached_icon_textures() {
    let _home = test_home::enter("live_tools_panel_draws_cached_icon_textures");
    let lib = include_str!("../src/lib.rs");
    assert!(lib.contains("pub mod icon_cache;"), "icon_cache not exported; PR-U02-ICONS-1 part A");
    let components = include_str!("../../dtool-design/src/components.rs");
    assert!(components.contains("fn icon_image_button"), "PR-U02-ICONS-1 part B");
    let tools_panel = include_str!("../src/tools_panel.rs");
    assert!(tools_panel.contains("Button::image") && tools_panel.contains("tool_texture"), "PR-U02-ICONS-1 part C");
    let shell = include_str!("../src/shell.rs");
    assert!(shell.contains("crate::tools_panel::editable_column(") && shell.contains("pick_strip_icons"), "PR-U02-LIVE-ICONS shell wiring");
    assert!(shell.contains("toolbar_icons"), "PR-U02-LIVE-ICONS persona controls wiring");
}