use crate::icon_cache::IconCache;
use crate::menu_tree::{self, MenuItem, MenuTree};
use crate::shortcut_commands::{key_name, modifiers_from};
use crate::shortcuts::{Focus, Modifiers, ShortcutMap};
use dtool_core::{Command, ContextId, Session};
use dtool_strings::text;
use egui::{Event, Key, PointerButton};
use serde::{Deserialize, Serialize};
use std::{
    f32::consts::{FRAC_PI_2, FRAC_PI_4, FRAC_PI_8, TAU},
    path::{Path, PathBuf},
};

const ALIGN: [(&str, &str, &str, &str); 8] = [
    (
        "layer.arrange.align.top",
        "Align top",
        "위로 정렬",
        "tool-align-top",
    ),
    (
        "layer.arrange.align.centre",
        "Align horizontal centre",
        "가로 가운데",
        "tool-align-center-h",
    ),
    (
        "layer.arrange.align.right",
        "Align right",
        "우측 정렬",
        "tool-align-right",
    ),
    (
        "layer.arrange.align.middle",
        "Align vertical centre",
        "세로 가운데",
        "tool-align-center-v",
    ),
    (
        "layer.arrange.align.bottom",
        "Align bottom",
        "하단 정렬",
        "tool-align-bottom",
    ),
    (
        "layer.arrange.distribute.h",
        "Distribute horizontally",
        "좌우 균등",
        "tool-distribute-h",
    ),
    (
        "layer.arrange.align.left",
        "Align left",
        "좌측 정렬",
        "tool-align-left",
    ),
    (
        "layer.arrange.distribute.v",
        "Distribute vertically",
        "상하 균등",
        "tool-distribute-v",
    ),
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Trigger {
    key: String,
    modifiers: Modifiers,
}

impl Trigger {
    fn display(&self) -> String {
        crate::shortcuts_page::display_shortcut(&crate::shortcuts::Shortcut {
            command: String::new(),
            key: self.key.clone(),
            modifiers: self.modifiers,
        })
    }

    fn valid(&self) -> bool {
        matches!(self.key.as_str(), "Mouse 4" | "Mouse 5")
            || (Key::ALL
                .iter()
                .any(|&key| key_name(key).as_deref() == Some(self.key.as_str()))
                && !crate::shortcut_commands::is_reserved(&self.key, self.modifiers)
                && !(self.modifiers.alt && self.key == "F4"))
    }

    fn allowed_in_text(&self) -> bool {
        (self.modifiers.ctrl ^ self.modifiers.alt)
            || self.key.starts_with('F') && self.key.len() > 1
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Menu {
    name: String,
    enabled: bool,
    trigger: Option<Trigger>,
    actions: [Option<String>; 8],
}

#[derive(Clone, Serialize, Deserialize)]
struct Config {
    version: u32,
    menus: Vec<Menu>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            menus: vec![Menu {
                name: "정렬".into(),
                enabled: true,
                trigger: Some(Trigger {
                    key: "A".into(),
                    modifiers: Modifiers::parse("Shift"),
                }),
                actions: ALIGN.map(|(id, _, _, _)| Some(id.into())),
            }],
        }
    }
}

impl Config {
    fn read(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
        let result: Self = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if result.version != 1
            || result.menus.is_empty()
            || result.menus.len() > 128
            || result.menus.iter().enumerate().any(|(index, menu)| {
                menu.name.trim().is_empty()
                    || menu.trigger.as_ref().is_some_and(|trigger| {
                        !trigger.valid()
                            || result.menus[..index]
                                .iter()
                                .any(|other| other.trigger.as_ref() == Some(trigger))
                    })
            })
        {
            return Err("올바른 파이메뉴 백업 파일이 아닙니다.".into());
        }
        Ok(result)
    }

    fn write(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        dtool_export::write_atomic(path, &bytes).map_err(|error| error.to_string())
    }
}

struct Action {
    id: &'static str,
    label: String,
    sort: String,
    group: String,
    tab: usize,
    icon: Option<&'static str>,
    command: Option<&'static str>,
}

fn action_list() -> Vec<Action> {
    let mut actions: Vec<_> = ALIGN
        .into_iter()
        .map(|(id, sort, label, icon)| Action {
            id,
            label: label.into(),
            sort: sort.into(),
            group: "정렬 및 간격".into(),
            tab: 0,
            icon: Some(icon),
            command: Some(id),
        })
        .collect();
    let catalog = dtool_protocol::CommandCatalog::new();
    for root in MenuTree::for_persona("vector").menus {
        let tab = match root.id.0 {
            "vector" => 1,
            "pixel" => 2,
            "document" | "text" => 3,
            _ => 0,
        };
        collect_actions(&root, text(root.key), tab, &catalog, &mut actions);
    }
    actions.sort_by(|a, b| {
        (a.tab, &a.group, a.sort.to_ascii_lowercase()).cmp(&(
            b.tab,
            &b.group,
            b.sort.to_ascii_lowercase(),
        ))
    });
    actions
}

fn collect_actions(
    item: &MenuItem,
    group: &str,
    tab: usize,
    catalog: &dtool_protocol::CommandCatalog,
    actions: &mut Vec<Action>,
) {
    if !item.children.is_empty() {
        let group = if item.id.0.contains('.') {
            text(item.key)
        } else {
            group
        };
        for child in &item.children {
            collect_actions(child, group, tab, catalog, actions);
        }
        return;
    }
    if item.separator || item.planned || item.id.0.starts_with("layer.arrange.align.") {
        return;
    }
    let direct = item.command.filter(|id| {
        catalog
            .resolve(id)
            .is_some_and(|spec| spec.implemented && spec.input_schema.is_none())
    });
    if menu_tree::menu_binding(item.id.0).is_some() || direct.is_some() {
        actions.push(Action {
            id: item.id.0,
            label: text(item.key).into(),
            sort: item
                .id
                .0
                .rsplit('.')
                .next()
                .unwrap_or(item.id.0)
                .replace('_', " "),
            group: group.into(),
            tab,
            icon: None,
            command: direct,
        });
    }
}

pub(crate) struct Invocation {
    pub owner: ContextId,
    pub id: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Held {
    Key(Key),
    Mouse(PointerButton),
}

struct OpenPie {
    menu: usize,
    owner: ContextId,
    center: egui::Pos2,
    radius: f32,
    held: Held,
}

struct Pending {
    trigger: Trigger,
    conflicts: String,
}

pub(crate) struct PieMenus {
    config: Config,
    path: PathBuf,
    actions: Vec<Action>,
    overlay: Option<OpenPie>,
    held: Option<Held>,
    swallowed: Vec<PointerButton>,
    pointer: Option<egui::Pos2>,
    pub(crate) editor: bool,
    menu: usize,
    direction: usize,
    tab: usize,
    search: String,
    rename: String,
    capture: bool,
    pending: Option<Pending>,
    deleting: bool,
    restore: Option<Config>,
    message: String,
    icons: IconCache,
}

impl Default for PieMenus {
    fn default() -> Self {
        let path = crate::settings::UiSettings::default_path().with_file_name("pie_menus.json");
        let (config, message) = if path.exists() {
            match Config::read(&path) {
                Ok(config) => (config, String::new()),
                Err(error) => {
                    let mut config = Config::default();
                    config.menus[0].enabled = false;
                    (config, error)
                }
            }
        } else {
            (Config::default(), String::new())
        };
        let rename = config.menus[0].name.clone();
        Self {
            config,
            path,
            actions: action_list(),
            overlay: None,
            held: None,
            swallowed: Vec::new(),
            pointer: None,
            editor: false,
            menu: 0,
            direction: 0,
            tab: 0,
            search: String::new(),
            rename,
            capture: false,
            pending: None,
            deleting: false,
            restore: None,
            message,
            icons: IconCache::new(),
        }
    }
}

impl PieMenus {
    pub(crate) fn reserved_chords(&self) -> Vec<(String, Modifiers)> {
        self.config
            .menus
            .iter()
            .filter(|menu| menu.enabled)
            .filter_map(|menu| menu.trigger.as_ref())
            .map(|trigger| (trigger.key.clone(), trigger.modifiers))
            .collect()
    }

    pub(crate) fn remove_conflicts(&mut self, map: &mut ShortcutMap) -> bool {
        let mut changed = false;
        for trigger in self
            .config
            .menus
            .iter()
            .filter(|menu| menu.enabled)
            .filter_map(|menu| menu.trigger.as_ref())
        {
            changed |= map.remove_chord(&trigger.key, trigger.modifiers);
        }
        changed
    }

    pub(crate) fn command(
        &self,
        id: &str,
        session: &Session,
        size: [f32; 2],
        read_only: bool,
    ) -> Option<Command> {
        let action = self.actions.iter().find(|action| action.id == id)?;
        let command = if let Some(binding) = menu_tree::menu_binding(id) {
            binding.command(session, size)
        } else {
            Command::with_params(action.command?, serde_json::json!({}))
        };
        if read_only
            && !session
                .catalog
                .resolve(&command.name)
                .is_some_and(|spec| !spec.mutating)
        {
            return None;
        }
        session.can_execute(&command).then_some(command)
    }

    pub(crate) fn intercept(
        &mut self,
        ctx: &egui::Context,
        input: &mut egui::RawInput,
        canvas: Option<(ContextId, egui::Rect)>,
        text: bool,
    ) -> Option<Invocation> {
        if !input.focused {
            self.overlay = None;
            self.held = None;
            self.pointer = None;
            self.swallowed.clear();
            return None;
        }
        let composing = input
            .events
            .iter()
            .any(|event| matches!(event, Event::Ime(_)));
        if canvas
            .is_none_or(|(owner, _)| self.overlay.as_ref().is_some_and(|pie| owner != pie.owner))
            || composing
        {
            self.overlay = None;
        }
        let mut output = None;
        let mut consume_text = false;
        let mut prior_pointer_press = false;
        input.events.retain(|event| {
            if matches!(event, Event::WindowFocused(false)) {
                self.overlay = None;
                self.held = None;
                self.swallowed.clear();
                self.pointer = None;
                output = None;
            }
            match event {
                Event::PointerMoved(pos) | Event::PointerButton { pos, .. } => {
                    self.pointer = Some(*pos)
                }
                Event::PointerGone => self.pointer = None,
                _ => {}
            }
            if matches!(
                event,
                Event::Key {
                    key: Key::Escape,
                    pressed: true,
                    ..
                }
            ) && self.overlay.is_some()
            {
                self.overlay = None;
                return false;
            }
            if matches!(event, Event::Text(_)) && consume_text {
                return false;
            }
            let (button, pressed, repeat, trigger) = match event {
                Event::Key {
                    key,
                    pressed,
                    repeat,
                    modifiers,
                    ..
                } => {
                    consume_text = false;
                    (
                        Held::Key(*key),
                        *pressed,
                        *repeat,
                        key_name(*key).map(|key| Trigger {
                            key,
                            modifiers: modifiers_from(*modifiers),
                        }),
                    )
                }
                Event::PointerButton {
                    button,
                    pressed,
                    modifiers,
                    ..
                } => {
                    let key = match button {
                        PointerButton::Extra1 => Some("Mouse 4"),
                        PointerButton::Extra2 => Some("Mouse 5"),
                        _ => None,
                    };
                    (
                        Held::Mouse(*button),
                        *pressed,
                        false,
                        key.map(|key| Trigger {
                            key: key.into(),
                            modifiers: modifiers_from(*modifiers),
                        }),
                    )
                }
                Event::MouseWheel { .. } | Event::Zoom(_) | Event::Rotate(_)
                    if self.overlay.is_some() =>
                {
                    return false;
                }
                Event::Text(_) | Event::Copy | Event::Cut | Event::Paste(_)
                    if self.overlay.is_some() =>
                {
                    return false;
                }
                _ => return true,
            };
            if self.held == Some(button) {
                consume_text = pressed;
                if !pressed {
                    self.held = None;
                    if let Some(pie) = self.overlay.take()
                        && let Some(direction) = self
                            .pointer
                            .and_then(|position| sector(pie.center, position, pie.radius))
                        && let Some(id) = self.config.menus[pie.menu].actions[direction].clone()
                    {
                        output = Some(Invocation {
                            owner: pie.owner,
                            id,
                        });
                    }
                }
                return false;
            }
            if let Held::Mouse(button) = button {
                if self.swallowed.contains(&button) || self.overlay.is_some() {
                    if pressed && !self.swallowed.contains(&button) {
                        self.swallowed.push(button);
                    } else if !pressed {
                        self.swallowed.retain(|held| *held != button);
                    }
                    return false;
                }
            }
            if self.overlay.is_some() {
                return false;
            }
            if let (Some((owner, bounds)), Some(trigger), Some(center)) =
                (canvas, trigger, self.pointer)
                && !composing
                && !prior_pointer_press
                && pressed
                && !repeat
                && bounds.contains(center)
                && !matches!(event, Event::Key { modifiers, .. } if modifiers.mac_cmd)
                && (!text || trigger.allowed_in_text())
                && let Some(menu) = self
                    .config
                    .menus
                    .iter()
                    .position(|menu| menu.enabled && menu.trigger.as_ref() == Some(&trigger))
            {
                let radius = 136.0_f32.min(ctx.content_rect().size().min_elem() * 0.45);
                if radius >= 36.0 {
                    self.held = Some(button);
                    self.overlay = Some(OpenPie {
                        menu,
                        owner,
                        center,
                        radius,
                        held: button,
                    });
                    consume_text = true;
                    output = None;
                    return false;
                }
            }
            prior_pointer_press |= pressed && matches!(button, Held::Mouse(_));
            true
        });
        output
    }

    pub(crate) fn draw(
        &mut self,
        ctx: &egui::Context,
        session: &Session,
        size: [f32; 2],
        read_only: bool,
    ) {
        let Some(pie) = self.overlay.as_ref() else {
            return;
        };
        let (menu, center, radius, held) = (pie.menu, pie.center, pie.radius, pie.held);
        let selected = self.pointer.and_then(|point| sector(center, point, radius));
        let enabled = std::array::from_fn(|n| {
            self.config.menus[menu].actions[n]
                .as_deref()
                .is_some_and(|id| self.command(id, session, size, read_only).is_some())
        });
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("dtool.pie.overlay"),
        ));
        self.draw_ring(&painter, menu, center, radius, selected, enabled);
        let hint = match held {
            Held::Key(_) => "키를 놓아 실행",
            Held::Mouse(_) => "버튼을 놓아 실행",
        };
        painter.text(
            center,
            egui::Align2::CENTER_CENTER,
            hint,
            egui::FontId::proportional(9.0),
            ctx.global_style().visuals.text_color(),
        );
    }

    fn draw_ring(
        &mut self,
        painter: &egui::Painter,
        menu: usize,
        center: egui::Pos2,
        radius: f32,
        selected: Option<usize>,
        enabled: [bool; 8],
    ) {
        let ctx = painter.ctx();
        let visuals = &ctx.global_style().visuals;
        let inner = radius * 0.31;
        painter.circle_filled(center, inner, visuals.extreme_bg_color);
        for (n, active) in enabled.into_iter().enumerate() {
            let angle = FRAC_PI_4 * n as f32 - FRAC_PI_2;
            let highlighted = selected == Some(n) && active;
            let fill = if highlighted {
                visuals.selection.bg_fill
            } else {
                visuals.widgets.inactive.bg_fill
            };
            let mut mesh = egui::Mesh::default();
            for step in 0..=12u32 {
                let arc = angle - FRAC_PI_8 + FRAC_PI_4 * step as f32 / 12.0;
                let dir = egui::vec2(arc.cos(), arc.sin());
                mesh.colored_vertex(center + dir * inner, fill);
                mesh.colored_vertex(center + dir * radius, fill);
                if step != 0 {
                    let i = (step - 1) * 2;
                    mesh.add_triangle(i, i + 1, i + 2);
                    mesh.add_triangle(i + 2, i + 1, i + 3);
                }
            }
            painter.add(egui::Shape::mesh(mesh));
            let edge = egui::Vec2::angled(angle - FRAC_PI_8);
            painter.line_segment(
                [center + edge * inner, center + edge * radius],
                visuals.widgets.inactive.bg_stroke,
            );
            let pos = center + egui::Vec2::angled(angle) * (radius * 0.70);
            let id = self.config.menus[menu].actions[n].as_deref();
            let action = id.and_then(|id| self.actions.iter().find(|action| action.id == id));
            let colour = if highlighted {
                visuals.selection.stroke.color
            } else if active {
                visuals.text_color()
            } else {
                visuals.weak_text_color()
            };
            let icon = action
                .and_then(|action| action.icon)
                .and_then(|icon| self.icons.texture(ctx, icon, colour, 24));
            if let Some(icon) = &icon {
                painter.image(
                    icon.id(),
                    egui::Rect::from_center_size(
                        pos - egui::vec2(0.0, 9.0),
                        egui::Vec2::splat(19.0),
                    ),
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            let label = action.map_or(
                if id.is_some() {
                    "재지정 필요"
                } else {
                    "+"
                },
                |action| &action.label,
            );
            let mut abbreviated: String = label.chars().take(9).collect();
            if label.chars().count() > 9 {
                abbreviated.push('…');
            }
            painter.text(
                pos + egui::vec2(0.0, if icon.is_some() { 9.0 } else { 0.0 }),
                egui::Align2::CENTER_CENTER,
                abbreviated,
                egui::FontId::proportional(11.0),
                colour,
            );
        }
    }

    fn save(&mut self) {
        self.message = match self.config.write(&self.path) {
            Ok(()) => String::new(),
            Err(error) => error,
        };
    }

    fn assign(&mut self, trigger: Trigger, shortcuts: &mut ShortcutMap) {
        let mut next = self.config.clone();
        for menu in &mut next.menus {
            if menu.trigger.as_ref() == Some(&trigger) {
                menu.trigger = None;
            }
        }
        next.menus[self.menu].trigger = Some(trigger.clone());
        next.menus[self.menu].enabled = true;
        match next.write(&self.path) {
            Ok(()) => {
                self.config = next;
                shortcuts.remove_chord(&trigger.key, trigger.modifiers);
                self.message = shortcuts.save_checked().err().unwrap_or_default();
                crate::shortcuts::publish_map(shortcuts);
            }
            Err(error) => self.message = error,
        }
        self.pending = None;
    }

    fn capture_trigger(&mut self, ctx: &egui::Context, shortcuts: &mut ShortcutMap) {
        let event = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                Event::Key {
                    key: Key::Escape,
                    pressed: true,
                    ..
                } => Some(None),
                Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } => key_name(*key).map(|key| {
                    Some(Trigger {
                        key,
                        modifiers: modifiers_from(*modifiers),
                    })
                }),
                Event::PointerButton {
                    button,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let key = match button {
                        PointerButton::Extra1 => "Mouse 4",
                        PointerButton::Extra2 => "Mouse 5",
                        _ => return None,
                    };
                    Some(Some(Trigger {
                        key: key.into(),
                        modifiers: modifiers_from(*modifiers),
                    }))
                }
                _ => None,
            })
        });
        let Some(trigger) = event else {
            return;
        };
        self.capture = false;
        let Some(trigger) = trigger else {
            return;
        };
        if !trigger.valid() {
            self.message = "이 조합은 디툴에서 예약되어 있습니다.".into();
            return;
        }
        if self.config.menus[self.menu].trigger.as_ref() == Some(&trigger) {
            return;
        }
        let mut conflicts: Vec<_> = shortcuts
            .dispatch_all(Focus::Canvas, &trigger.key, trigger.modifiers)
            .iter()
            .map(|name| crate::shortcuts_page::command_label(name))
            .collect();
        conflicts.extend(
            self.config
                .menus
                .iter()
                .filter(|menu| menu.trigger.as_ref() == Some(&trigger))
                .map(|menu| menu.name.clone()),
        );
        if conflicts.is_empty() {
            self.assign(trigger, shortcuts);
        } else {
            self.pending = Some(Pending {
                trigger,
                conflicts: conflicts.join(", "),
            });
        }
    }

    pub(crate) fn show(&mut self, ctx: &egui::Context, shortcuts: &mut ShortcutMap) {
        if !self.editor {
            return;
        }
        if self.capture {
            self.capture_trigger(ctx, shortcuts);
        }
        let mut open = self.editor;
        let available = ctx.content_rect().size();
        egui::Window::new("파이메뉴").id(egui::Id::new("dtool.pie.settings")).open(&mut open)
            .default_size(egui::vec2(1050.0, 580.0).min(available * 0.88))
            .max_size(available * 0.94).resizable(true).collapsible(false)
            .show(ctx, |ui| {
                if let Some(pending) = self.pending.as_ref() {
                    ui.label(format!("{} 사용 중 · {}", pending.trigger.display(), pending.conflicts));
                    ui.horizontal(|ui| {
                        if ui.button("변경").clicked() && let Some(pending) = self.pending.take() {
                            self.assign(pending.trigger, shortcuts);
                        }
                        if ui.button("취소").clicked() { self.pending = None; }
                    });
                    ui.separator();
                }
                if self.restore.is_some() {
                    ui.label("백업으로 교체합니다. 복원된 호출키와 기존 단축키가 겹치면 파이메뉴에 배정됩니다.");
                    ui.horizontal(|ui| {
                        if ui.button("복원 및 변경").clicked() && let Some(next) = self.restore.take() {
                            self.restore_config(next);
                        }
                        if ui.button("취소").clicked() { self.restore = None; }
                    });
                    ui.separator();
                }
                if !self.message.is_empty() { ui.label(&self.message); }
                if ui.available_width() >= 665.0 {
                    ui.columns(3, |columns| {
                        self.menu_list(&mut columns[0]);
                        self.preview(&mut columns[1]);
                        self.command_list(&mut columns[2]);
                    });
                } else {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        self.menu_list(ui);
                        ui.separator();
                        self.preview(ui);
                        ui.separator();
                        self.command_list(ui);
                    });
                }
            });
        self.editor = open;
        if !open {
            self.capture = false;
            self.pending = None;
            self.restore = None;
            self.deleting = false;
        }
    }

    fn menu_list(&mut self, ui: &mut egui::Ui) {
        ui.heading("메뉴");
        egui::ScrollArea::vertical()
            .id_salt("pie-menu-list")
            .max_height(230.0)
            .show(ui, |ui| {
                for (index, menu) in self.config.menus.iter().enumerate() {
                    if ui
                        .selectable_label(index == self.menu, &menu.name)
                        .clicked()
                    {
                        self.menu = index;
                        self.rename.clone_from(&menu.name);
                        self.pending = None;
                        self.deleting = false;
                    }
                }
            });
        let rename = ui.text_edit_singleline(&mut self.rename);
        let enter = rename.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
        if (ui.button("이름 저장").clicked() || enter) && !self.rename.trim().is_empty() {
            self.config.menus[self.menu].name = self.rename.trim().into();
            self.save();
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(self.config.menus.len() < 128, egui::Button::new("새 메뉴"))
                .clicked()
            {
                self.rename = format!("메뉴 {}", self.config.menus.len() + 1);
                self.config.menus.push(Menu {
                    name: self.rename.clone(),
                    enabled: true,
                    trigger: None,
                    actions: Default::default(),
                });
                self.menu = self.config.menus.len() - 1;
                self.pending = None;
                self.deleting = false;
                self.save();
            }
            if ui
                .add_enabled(self.config.menus.len() > 1, egui::Button::new("삭제"))
                .clicked()
            {
                self.deleting = true;
            }
        });
        if self.deleting {
            ui.horizontal_wrapped(|ui| {
                if ui.button("메뉴 삭제 확인").clicked() {
                    self.config.menus.remove(self.menu);
                    self.menu = self.menu.min(self.config.menus.len() - 1);
                    self.rename.clone_from(&self.config.menus[self.menu].name);
                    self.pending = None;
                    self.deleting = false;
                    self.save();
                }
                if ui.button("취소").clicked() {
                    self.deleting = false;
                }
            });
        }
        #[cfg(windows)]
        self.backup_controls(ui);
    }

    #[cfg(windows)]
    fn backup_controls(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        if ui.button("백업").clicked() {
            self.message = (|| {
                if let Some(path) = crate::dialogs::pick_file_path(
                    "파이메뉴 백업",
                    "파이메뉴",
                    "json",
                    Some("pie-menus.json"),
                )
                .map_err(|error| error.to_string())?
                {
                    self.config.write(&path)?;
                }
                Ok::<_, String>(())
            })()
            .err()
            .unwrap_or_default();
        }
        if ui.button("복원").clicked() {
            self.message = (|| {
                if let Some(path) =
                    crate::dialogs::pick_file_path("파이메뉴 복원", "파이메뉴", "json", None)
                        .map_err(|error| error.to_string())?
                {
                    self.restore = Some(Config::read(&path)?);
                }
                Ok::<_, String>(())
            })()
            .err()
            .unwrap_or_default();
        }
    }

    fn restore_config(&mut self, next: Config) {
        self.message = match self
            .config
            .write(&self.path.with_file_name("pie_menus.previous.json"))
            .and_then(|()| next.write(&self.path))
        {
            Ok(()) => {
                self.config = next;
                self.menu = 0;
                self.rename.clone_from(&self.config.menus[0].name);
                self.pending = None;
                String::new()
            }
            Err(error) => error,
        };
    }

    fn preview(&mut self, ui: &mut egui::Ui) {
        ui.heading("8방향 미리보기");
        let span = ui.available_width().clamp(100.0, 340.0);
        let (response, painter) =
            ui.allocate_painter(egui::Vec2::splat(span), egui::Sense::click());
        let center = response.rect.center();
        let radius = span * 0.48;
        if response.clicked() {
            response.request_focus();
            if let Some(n) = response
                .interact_pointer_pos()
                .and_then(|point| sector(center, point, radius))
            {
                self.direction = n;
            }
        }
        if response.has_focus() {
            let keys = [
                (Key::ArrowUp, 0),
                (Key::Num8, 0),
                (Key::Num9, 1),
                (Key::ArrowRight, 2),
                (Key::Num6, 2),
                (Key::Num3, 3),
                (Key::ArrowDown, 4),
                (Key::Num2, 4),
                (Key::Num1, 5),
                (Key::ArrowLeft, 6),
                (Key::Num4, 6),
                (Key::Num7, 7),
            ];
            if let Some((_, n)) = keys
                .into_iter()
                .find(|(key, _)| ui.input(|input| input.key_pressed(*key)))
            {
                self.direction = n;
            }
        }
        self.draw_ring(
            &painter,
            self.menu,
            center,
            radius,
            Some(self.direction),
            [true; 8],
        );
        let menu = &mut self.config.menus[self.menu];
        let caption = if self.capture {
            "키 또는 Mouse 4/5를 누르세요".into()
        } else {
            menu.trigger
                .as_ref()
                .map_or("호출 단축키 지정".into(), Trigger::display)
        };
        if ui.button(caption).clicked() {
            self.capture = true;
            self.pending = None;
        }
        if ui.checkbox(&mut menu.enabled, "활성").changed() {
            self.save();
        }
    }

    fn command_list(&mut self, ui: &mut egui::Ui) {
        ui.heading(format!("{}번 조각에 명령 지정", self.direction + 1));
        ui.horizontal_wrapped(|ui| {
            for (n, name) in ["공통", "벡터", "픽셀", "레이아웃"].into_iter().enumerate()
            {
                ui.selectable_value(&mut self.tab, n, name);
            }
        });
        ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("명령 검색"));
        let query = self.search.trim().to_lowercase();
        let mut choice = None;
        egui::ScrollArea::vertical()
            .id_salt(("pie-commands", self.tab))
            .max_height(ui.available_height().clamp(140.0, 490.0) - 30.0)
            .show(ui, |ui| {
                let mut group = "";
                let current = self.config.menus[self.menu].actions[self.direction].as_deref();
                for action in self.actions.iter().filter(|action| {
                    action.tab == self.tab
                        && (query.is_empty()
                            || [&action.label, &action.sort, &action.group]
                                .iter()
                                .any(|value| value.to_lowercase().contains(&query))
                            || action.id.contains(&query))
                }) {
                    if group != action.group {
                        if !group.is_empty() {
                            ui.separator();
                        }
                        group = &action.group;
                        ui.strong(group);
                    }
                    if ui
                        .selectable_label(current == Some(action.id), &action.label)
                        .on_hover_text(&action.sort)
                        .clicked()
                    {
                        choice = Some(Some(action.id.to_string()));
                    }
                }
                if group.is_empty() {
                    ui.label("검색 결과 없음");
                }
            });
        if ui.button("이 방향 비우기").clicked() {
            choice = Some(None);
        }
        if let Some(choice) = choice {
            self.config.menus[self.menu].actions[self.direction] = choice;
            self.save();
        }
    }
}

fn sector(center: egui::Pos2, point: egui::Pos2, radius: f32) -> Option<usize> {
    let delta = point - center;
    let distance = delta.length();
    if !distance.is_finite() || distance < radius * 0.31 || distance > radius {
        return None;
    }
    let angle = (delta.y.atan2(delta.x) + FRAC_PI_2 + FRAC_PI_8).rem_euclid(TAU);
    Some((angle / FRAC_PI_4).floor() as usize % 8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reassigning_a_chord_preserves_other_shortcuts_for_its_command() {
        let mut map = ShortcutMap::affinity_default();
        let shift = Modifiers::parse("Shift");
        let ctrl = Modifiers::parse("Control");
        assert!(!map.dispatch_all(Focus::Canvas, "A", shift).is_empty());
        let selection = map.dispatch_all(Focus::Canvas, "A", ctrl);
        assert!(map.remove_chord("A", shift));
        assert!(map.dispatch_all(Focus::Canvas, "A", shift).is_empty());
        assert_eq!(map.dispatch_all(Focus::Canvas, "A", ctrl), selection);
        assert!(!map.remove_chord("A", shift));
    }

    #[test]
    fn opens_on_press_and_uses_pointer_at_release() {
        let ctx = egui::Context::default();
        let mut pie = PieMenus {
            config: Config::default(),
            ..Default::default()
        };
        let owner = ContextId(42);
        let center = egui::pos2(200.0, 200.0);
        let canvas = Some((
            owner,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::splat(600.0)),
        ));
        let key = |pressed, repeat, modifiers| Event::Key {
            key: Key::A,
            physical_key: Some(Key::A),
            pressed,
            repeat,
            modifiers,
        };
        let mut send = |events, text| {
            let mut input = egui::RawInput {
                focused: true,
                events,
                ..Default::default()
            };
            let invocation = pie.intercept(&ctx, &mut input, canvas, text);
            (invocation.map(|run| run.id), input.events)
        };

        let (invoked, events) = send(
            vec![
                Event::PointerMoved(center),
                key(true, false, egui::Modifiers::SHIFT),
                Event::Text("A".into()),
            ],
            false,
        );
        assert!(invoked.is_none());
        assert_eq!(events.len(), 1);
        let (invoked, _) = send(
            vec![
                Event::PointerMoved(center - egui::vec2(0.0, 90.0)),
                key(true, true, egui::Modifiers::SHIFT),
            ],
            false,
        );
        assert!(invoked.is_none());
        let (invoked, _) = send(vec![key(false, false, egui::Modifiers::NONE)], false);
        assert_eq!(invoked.as_deref(), Some(ALIGN[0].0));

        let (invoked, _) = send(
            vec![
                Event::PointerMoved(center),
                key(true, false, egui::Modifiers::SHIFT),
                Event::PointerMoved(center + egui::vec2(0.0, 90.0)),
                Event::PointerMoved(center + egui::vec2(0.0, 200.0)),
                key(false, false, egui::Modifiers::SHIFT),
            ],
            false,
        );
        assert!(invoked.is_none());
        let (invoked, events) = send(
            vec![
                Event::PointerMoved(center),
                key(true, false, egui::Modifiers::SHIFT),
                Event::Text("A".into()),
            ],
            true,
        );
        assert!(invoked.is_none());
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn alignment_layout_and_visible_ring_agree() {
        let center = egui::Pos2::ZERO;
        for (index, (id, _, _, _)) in ALIGN.into_iter().enumerate() {
            let direction = egui::Vec2::angled(index as f32 * FRAC_PI_4 - FRAC_PI_2);
            assert_eq!(
                sector(center, center + direction * 100.0, 136.0),
                Some(index)
            );
            assert_eq!(
                Config::default().menus[0].actions[index].as_deref(),
                Some(id)
            );
        }
        assert_eq!(sector(center, center, 136.0), None);
        assert_eq!(sector(center, egui::pos2(0.0, -137.0), 136.0), None);
    }
}
