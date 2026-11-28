use dtool_core::Session;
use dtool_protocol::{Command, CommandError, CommandResult};
use dtool_strings::text;
use egui;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolId(pub &'static str);

#[derive(Clone, Debug)]
pub struct ToolFlyout {
    pub parent: ToolId,
    pub children: Vec<ToolId>,
}

pub const PICTURE_FRAME_KEYS: [&str; 6] = [
    "tool.picture_frame",
    "tool.picture_frame.rect",
    "tool.picture_frame.ellipse",
    "tooltip.tool.picture_frame",
    "tooltip.tool.picture_frame.rect",
    "tooltip.tool.picture_frame.ellipse",
];

pub fn picture_frame_tools_listed() -> bool {
    PICTURE_FRAME_KEYS.iter().all(|key| dtool_strings::has(key))
}

pub fn default_flyouts() -> Vec<ToolFlyout> {
    let mut flyouts = base_flyouts();
    if picture_frame_tools_listed() {
        let at = flyouts.iter().position(|flyout| flyout.parent.0 == "tool.shapes").map_or(flyouts.len(), |index| index + 1);
        flyouts.insert(at, ToolFlyout {
            parent: ToolId("tool.picture_frame"),
            children: vec![
                ToolId("tool.picture_frame.rect"),
                ToolId("tool.picture_frame.ellipse"),
            ],
        });
    }
    flyouts
}

fn base_flyouts() -> Vec<ToolFlyout> {
    vec![
        ToolFlyout {
            parent: ToolId("tool.view"),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId("tool.zoom"),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId("tool.move"),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId("tool.select.object"),
            children: vec![ToolId("tool.selection_brush"), ToolId("tool.magic_wand")],
        },
        ToolFlyout {
            parent: ToolId("tool.marquee"),
            children: vec![
                ToolId("tool.marquee.rectangle"),
                ToolId("tool.marquee.ellipse"),
                ToolId("tool.marquee.row"),
                ToolId("tool.marquee.column"),
                ToolId("tool.marquee.freehand"),
            ],
        },
        ToolFlyout {
            parent: ToolId("tool.artboard"),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId("tool.shapes"),
            children: vec![
                ToolId("tool.shape.rectangle"),
                ToolId("tool.shape.triangle"),
                ToolId("tool.shape.ellipse"),
                ToolId("tool.shape.rounded_rectangle"),
                ToolId("tool.shape.line"),
                ToolId("tool.shape.curve"),
            ].into_iter().chain(dtool_core::context_toolbar::TOOL_IDS.iter().copied()
                .filter(|id| dtool_core::context_toolbar::is_advanced_shape_tool(id)).map(ToolId)).collect(),
        },
        ToolFlyout {
            parent: ToolId("tool.qr"),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId("tool.text"),
            children: vec![
                ToolId("tool.text.artistic"),
                ToolId("tool.text.frame"),
            ],
        },
        ToolFlyout {
            parent: ToolId("tool.table"),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId("tool.pen"),
            children: vec![
                ToolId("tool.pen.pen"),
                ToolId("tool.pen.pencil"),
                ToolId("tool.pen.spline"),
                ToolId("tool.pen.smart"),
                ToolId("tool.pen.b_spline"),
                ToolId("tool.pen.polyline"),
            ],
        },
        ToolFlyout {
            parent: ToolId("tool.stroke_width"),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId("tool.node"),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId(dtool_core::commands::curve_nodes::corner::TOOL),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId("tool.transform"),
            children: vec![
                ToolId("tool.transform.rotate"),
                ToolId("tool.transform.scale"),
                ToolId("tool.transform.shear"),
                ToolId("tool.transform.flip"),
                ToolId("tool.point_transform"),
            ],
        },
        ToolFlyout {
            parent: ToolId("tool.crop"),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId("tool.vector_crop"),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId("tool.slice"),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId("tool.eyedropper"),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId("tool.style_picker"),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId("tool.measure"),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId("tool.area"),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId(dtool_core::commands::fill::TOOL),
            children: vec![ToolId("tool.flood_fill")],
        },
        ToolFlyout {
            parent: ToolId("tool.gradient"),
            children: vec![
                ToolId("tool.gradient.linear"),
                ToolId("tool.gradient.radial"),
                ToolId("tool.gradient.conical"),
            ],
        },
        ToolFlyout {
            parent: ToolId("tool.transparency"),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId("tool.mesh"),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId("tool.blend"),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId("tool.contour"),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId("tool.boolean"),
            children: vec![
                ToolId("tool.boolean.add"),
                ToolId("tool.boolean.subtract"),
                ToolId("tool.boolean.intersect"),
                ToolId("tool.boolean.xor"),
                ToolId("tool.boolean.divide"),
            ],
        },
        ToolFlyout {
            parent: ToolId("tool.knife"),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId("tool.shape_builder"),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId(dtool_core::context_toolbar::BLOB_BRUSH),
            children: vec![ToolId(dtool_core::context_toolbar::BLOB_ERASE)],
        },
        ToolFlyout {
            parent: ToolId(dtool_core::context_toolbar::VECTOR_FLOOD),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId("tool.eraser"),
            children: vec![
                ToolId("tool.eraser.erase"),
                ToolId("tool.eraser.background"),
                ToolId("tool.eraser.flood"),
                ToolId("tool.eraser.smudge"),
            ],
        },
        ToolFlyout {
            parent: ToolId("tool.paint_brush"),
            children: vec![
                ToolId("tool.paint.brush"),
                ToolId("tool.paint.mixer"),
                ToolId("tool.paint.colour_replace"),
                ToolId("tool.paint.pencil"),
                ToolId("tool.paint.pixel"),
                ToolId("tool.paint.vector_brush"),
                ToolId("tool.paint.calligraphy"),
            ],
        },
        ToolFlyout {
            parent: ToolId("tool.paint.adjustment"),
            children: vec![ToolId("tool.paint.tone"), ToolId("tool.dodge"), ToolId("tool.burn"), ToolId("tool.paint.sponge")],
        },
        ToolFlyout {
            parent: ToolId("tool.paint.filter"),
            children: vec![
                ToolId("tool.paint.blur"), ToolId("tool.sharpen"), ToolId("tool.paint.median"), ToolId("tool.paint.smudge"),
            ],
        },
        ToolFlyout {
            parent: ToolId(dtool_core::context_toolbar::CLONE_TOOL),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId(dtool_core::context_toolbar::HEAL_TOOL),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId(dtool_core::context_toolbar::UNDO_BRUSH_TOOL),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId(dtool_core::commands::patch::TOOL),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId(dtool_core::commands::inpaint::TOOL),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId(dtool_core::commands::blemish::TOOL),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId(dtool_core::commands::red_eye::TOOL),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId("tool.symmetry"),
            children: vec![],
        },
        ToolFlyout {
            parent: ToolId("tool.align"),
            children: vec![
                ToolId("tool.align.left"),
                ToolId("tool.align.center_h"),
                ToolId("tool.align.right"),
                ToolId("tool.align.top"),
                ToolId("tool.align.center_v"),
                ToolId("tool.align.bottom"),
            ],
        },
        ToolFlyout {
            parent: ToolId("tool.distribute"),
            children: vec![
                ToolId("tool.distribute.h"),
                ToolId("tool.distribute.v"),
            ],
        },
    ]
}

pub const P0_SHAPE_TOOLS: &[&str] = &[
    "tool.shape.rectangle",
    "tool.shape.triangle",
    "tool.shape.ellipse",
    "tool.shape.rounded_rectangle",
];

pub fn is_p0_shape_tool(id: &str) -> bool {
    P0_SHAPE_TOOLS.contains(&id)
}

pub(crate) fn boolean_tool_command(id: &str) -> Option<&'static str> {
    use dtool_core::boolean::{BOOLEAN_ADD, BOOLEAN_DIVIDE, BOOLEAN_INTERSECT, BOOLEAN_SUBTRACT, BOOLEAN_XOR};
    match id {
        "tool.boolean.add" => Some(BOOLEAN_ADD),
        "tool.boolean.subtract" => Some(BOOLEAN_SUBTRACT),
        "tool.boolean.intersect" => Some(BOOLEAN_INTERSECT),
        "tool.boolean.xor" => Some(BOOLEAN_XOR),
        "tool.boolean.divide" => Some(BOOLEAN_DIVIDE),
        _ => None,
    }
}

pub fn is_held_shape_tool(id: &str) -> bool {
    id.starts_with("tool.shape.") && dtool_core::context_toolbar::canonical_tool(id).is_none()
}

pub fn listed_tool_ids() -> Vec<&'static str> {
    let mut ids = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for flyout in default_flyouts() {
        for tool in std::iter::once(flyout.parent).chain(flyout.children) {
            if seen.insert(tool.0) {
                ids.push(tool.0);
            }
        }
    }
    for id in dtool_core::context_toolbar::TOOL_IDS {
        if seen.insert(*id) {
            ids.push(*id);
        }
    }
    for id in P0_SHAPE_TOOLS {
        if seen.insert(*id) {
            ids.push(*id);
        }
    }
    ids
}

fn resolve_tool_id(id: &str) -> Result<&str, CommandError> {
    if is_held_shape_tool(id) {
        return Err(CommandError::not_implemented());
    }
    if let Some(id) = dtool_core::context_toolbar::canonical_tool(id) {
        return Ok(id);
    }
    if is_p0_shape_tool(id) {
        return Ok(id);
    }
    Err(CommandError::invalid_params())
}

pub fn handle_tool_command(session: &mut Session, cmd: &Command) -> CommandResult {
    match cmd.name.as_str() {
        "ui.tool.set" => {
            let tool_id = cmd.params.get("id").and_then(|v| v.as_str()).ok_or_else(|| CommandError::invalid_params())?;
            let tool_id = resolve_tool_id(tool_id)?;
            session.panel_colour_pick_previous = None;
            session.active_tool = tool_id.to_string();
            Ok(serde_json::json!({"id":session.active_tool}))
        }
        "ui.tool.query" => {
            let active = session.active_tool.clone();
            Ok(serde_json::json!({ "id": active }))
        }
        "ui.tools.list" => Ok(serde_json::json!({ "tools": listed_tool_ids() })),
        _ => Err(CommandError::not_implemented()),
    }
}

pub fn tool_column_ids(active: &str) -> Vec<&'static str> {
    let mut ids = Vec::new();
    for flyout in default_flyouts() {
        ids.push(flyout.parent.0);
        let open = flyout.parent.0 == active || flyout.children.iter().any(|child| child.0 == active);
        if open {
            ids.extend(flyout.children.iter().map(|child| child.0));
        }
    }
    ids
}

pub fn tool_column(ui: &mut egui::Ui, active: &str) -> Option<&'static str> {
    column(ui, active, None, None, (1, &[]), &mut Vec::new())
}

pub fn tool_column_icons(ui: &mut egui::Ui, active: &str, icons: &mut crate::icon_cache::IconCache) -> Option<&'static str> {
    column(ui, active, Some(icons), None, (1, &[]), &mut Vec::new())
}

pub fn persona_tool_column_icons(ui: &mut egui::Ui, active: &str, icons: &mut crate::icon_cache::IconCache, tools: &[ToolId]) -> Option<&'static str> {
    column(ui, active, Some(icons), Some(tools), (1, &[]), &mut Vec::new())
}

pub(crate) fn editable_column(ui: &mut egui::Ui, active: &str, icons: &mut crate::icon_cache::IconCache,
    tools: Option<&[ToolId]>, layout: (usize, &[ToolId]), rows: &mut Vec<(ToolId, egui::Rect)>) -> Option<&'static str> {
    column(ui, active, Some(icons), tools, layout, rows)
}

fn column(ui: &mut egui::Ui, active: &str, mut icons: Option<&mut crate::icon_cache::IconCache>,
    tools: Option<&[ToolId]>, (columns, standalone): (usize, &[ToolId]), rows: &mut Vec<(ToolId, egui::Rect)>) -> Option<&'static str> {
    let mut picked = None;
    let groups = default_flyouts();
    let default_tools: Vec<_> = groups.iter().flat_map(|group| std::iter::once(group.parent.clone()).chain(group.children.iter().cloned())).collect();
    let tools = tools.unwrap_or(&default_tools);
    let included = |id| tools.iter().any(|tool| tool.0 == id);
    let visible: Vec<_> = tools.iter().filter(|ToolId(id)| {
        standalone.contains(&ToolId(id)) || !groups.iter().any(|group| included(group.parent.0) && group.children.iter().any(|child| child.0 == *id))
    }).collect();
    ui.vertical_centered(|ui| {
        for row in visible.chunks(columns.max(1)) {
          let mut draw = |ui: &mut egui::Ui| {
            for &&ToolId(id) in row {
            let children = groups.iter().find(|group| group.parent.0 == id).into_iter()
                .flat_map(|group| &group.children)
                .filter(|child| included(child.0) && !standalone.contains(child))
                .map(|child| child.0).collect::<Vec<_>>();
            if children.is_empty() {
                let badge = tool_button(ui, &mut icons, id, id == active, false, false);
                rows.push((ToolId(id), badge.rect.intersect(ui.clip_rect())));
                if badge.clicked() {
                    picked = Some(id);
                }
                continue;
            }
            let selected_child = children.iter().copied().find(|child| *child == active);
            let selected = selected_child.is_some() || id == active;
            let badge = tool_button(ui, &mut icons, selected_child.unwrap_or(id), selected, false, false);
            rows.push((ToolId(id), badge.rect.intersect(ui.clip_rect())));
            let c = badge.rect.right_bottom() - egui::vec2(3.0, 3.0);
            let colour = if selected { dtool_design::selected_icon_ink(ui) } else { dtool_design::icons::current_icon_colour() };
            ui.painter().add(egui::Shape::convex_polygon(vec![c, c - egui::vec2(5.0, 0.0), c - egui::vec2(0.0, 5.0)], colour, egui::Stroke::NONE));
            egui::Popup::menu(&badge).align(egui::RectAlign::RIGHT_START).show(|ui| {
                if id == "tool.shapes" {
                    egui::Grid::new("shape_flyout").num_columns(2).show(ui, |ui| {
                        for (index, child) in children.iter().copied().enumerate() {
                            if tool_button(ui, &mut icons, child, child == active, false, false).clicked() {
                                picked = Some(child);
                                ui.close();
                            }
                            if index % 2 == 1 { ui.end_row(); }
                        }
                    });
                    return;
                }
                let selectable_parent = matches!(id, "tool.select.object"
                    | dtool_core::context_toolbar::ADJUSTMENT_TOOL | dtool_core::context_toolbar::FILTER_BRUSH_TOOL
                    | dtool_core::context_toolbar::BLOB_BRUSH | dtool_core::commands::fill::TOOL)
                    .then_some(id);
                for child in selectable_parent.into_iter().chain(children) {
                    if tool_button(ui, &mut icons, child, child == active, true, false).clicked() {
                        picked = Some(child);
                        ui.close();
                    }
                }
            });
            }
          };
          if columns == 1 {
              ui.scope(draw);
          } else {
              ui.horizontal(|ui| {
                  let contents = 30.0 * row.len() as f32 + ui.spacing().item_spacing.x * row.len().saturating_sub(1) as f32;
                  let padding = ((ui.available_width() - contents) * 0.5 - ui.spacing().item_spacing.x).max(0.0);
                  if padding > 0.0 { ui.add_space(padding); }
                  draw(ui);
              });
          }
        }
    });
    picked
}

pub(crate) fn scroll_dragged_tool(ui: &egui::Ui, viewport: egui::Rect, start: egui::Pos2) {
    let (pointer, elapsed) = ui.input(|input| {
        let pointer = input.pointer.hover_pos().filter(|point| input.pointer.primary_down() && point.distance(start) > 5.0);
        (pointer, input.stable_dt.min(0.05))
    });
    let clip = ui.clip_rect();
    let Some(point) = pointer.filter(|point| clip.contains(*point)) else { return };
    let band = 22.0_f32.min(clip.height() * 0.5);
    if band <= 0.0 { return; }
    let edge = |distance: f32| (1.0 - distance / band).clamp(0.0, 1.0);
    let speed = 420.0 * (edge(clip.bottom() - point.y) - edge(point.y - clip.top()));
    let remaining = (ui.min_rect().height() - viewport.bottom()).max(0.0);
    let delta = (speed * elapsed).clamp(-viewport.top().max(0.0), remaining);
    if delta != 0.0 {
        ui.scroll_with_delta_animation(egui::vec2(0.0, -delta), egui::style::ScrollAnimation::none());
        ui.ctx().request_repaint();
    }
}

fn tool_button(ui: &mut egui::Ui, icons: &mut Option<&mut crate::icon_cache::IconCache>, id: &str, selected: bool, labeled: bool, placed: bool) -> egui::Response {
    let colour = if selected { dtool_design::selected_icon_ink(ui) } else { dtool_design::icons::current_icon_colour() };
    let texture = icons.as_deref_mut().and_then(|cache| cache.tool_texture(ui.ctx(), id, colour, dtool_design::icon_texture_px(ui)));
    let label = egui::RichText::new(text(id)).color(colour);
    let button = match texture {
        Some(texture) => {
            let image = egui::Image::new(&texture).fit_to_exact_size(egui::Vec2::splat(18.0));
            if labeled { egui::Button::image_and_text(image, label) } else { egui::Button::image(image) }
        }
        None => egui::Button::new(label),
    };
    let enabled = !placed && !is_held_shape_tool(id);
    let response = dtool_design::with_icon_selection(ui, |ui| {
        ui.add_enabled(enabled, button.selected(selected)
            .min_size(egui::vec2(30.0, 28.0)).frame(labeled || selected))
    });
    if !labeled {
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, enabled, selected, text(id)));
    }
    response.on_hover_text(crate::tooltips::tool_tooltip(id))
}

pub(crate) fn palette_button(ui: &mut egui::Ui, icons: &mut crate::icon_cache::IconCache, id: &str, placed: bool) -> egui::Response {
    let button = if let Some(texture) = icons.tool_texture(ui.ctx(), id, dtool_design::icons::current_icon_colour(), dtool_design::icon_texture_px(ui)) {
        egui::Button::image_and_text(
            egui::Image::new(&texture).fit_to_exact_size(egui::Vec2::splat(17.0)), text(id))
    } else {
        egui::Button::new(text(id))
    };
    let response = ui.add_enabled(!placed && !is_held_shape_tool(id), button.truncate()
        .min_size(egui::vec2(ui.available_width(), dtool_design::size("control.hit_size"))));
    let tooltip = crate::tooltips::tool_tooltip(id);
    if placed {
        response.on_disabled_hover_text(format!("{tooltip}\n{}", text("tools.customise.placed")))
    } else {
        dtool_design::tooltip(response, tooltip)
    }
}

pub fn render_tools_panel(ctx: &egui::Context, session: &Session) {
    let flyouts = default_flyouts();
    egui::Area::new(egui::Id::new("tools_panel")).show(ctx, |ui| {
        for flyout in &flyouts {
            let parent_key = flyout.parent.0.to_string();
            let label = text(&parent_key);

            if flyout.children.is_empty() {
                ui.add_enabled_ui(Some(session.active_tool.as_str()) == Some(flyout.parent.0), |ui| {
                    dtool_design::button(ui, label);
                });
            } else {
                let is_active = Some(session.active_tool.as_str()).map_or(false, |t| {
                    t == flyout.parent.0 || flyout.children.iter().any(|c| c.0 == t)
                });

                ui.add_enabled_ui(is_active, |ui| {
                    dtool_design::button(ui, label);
                });

                if is_active {
                    for child in &flyout.children {
                        let child_key = child.0.to_string();
                        let child_label = text(&child_key);
                        ui.add_enabled_ui(Some(session.active_tool.as_str()) == Some(child.0), |ui| {
                            dtool_design::button(ui, child_label);
                        });
                    }
                }
            }
        }
    });
}
