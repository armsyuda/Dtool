use dtool_strings::text;
use egui::Ui;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Category {
    #[default]
    All,
    Common,
    Vector,
    Pixel,
    Layout,
}

impl Category {
    pub const SECTIONS: [Self; 4] = [Self::Common, Self::Vector, Self::Pixel, Self::Layout];

    pub fn label(self) -> &'static str {
        text(match self {
            Self::All => "toolbar.catalog.all",
            Self::Common => "toolbar.catalog.common",
            Self::Vector => "persona.vector",
            Self::Pixel => "persona.pixel",
            Self::Layout => "persona.layout",
        })
    }

    pub fn includes(self, category: Self) -> bool {
        self == Self::All || self == category
    }
}

pub(crate) fn filters(ui: &mut Ui, id: &str) -> (Category, String, bool) {
    let category_id = ui.make_persistent_id((id, "category"));
    let search_id = ui.make_persistent_id((id, "search"));
    let (before, mut search) = ui.data(|data| {
        (data.get_temp::<Category>(category_id).unwrap_or_default(),
         data.get_temp::<String>(search_id).unwrap_or_default())
    });
    let mut category = before;
    ui.horizontal_wrapped(|ui| {
        for option in std::iter::once(Category::All).chain(Category::SECTIONS) {
            let response = dtool_design::with_icon_selection(ui, |ui| {
                ui.add(egui::Button::new(option.label())
                    .selected(category == option).frame_when_inactive(true)
                    .min_size(egui::vec2(48.0, dtool_design::size("control.hit_size"))))
            });
            if response.clicked() { category = option; }
        }
    });
    let changed = ui.add(egui::TextEdit::singleline(&mut search)
        .hint_text(text("pref.search")).desired_width(ui.available_width().min(320.0))).changed();
    let filter = search.trim().to_lowercase();
    ui.data_mut(|data| {
        if category != before { data.insert_temp(category_id, category); }
        if changed { data.insert_temp(search_id, search); }
    });
    (category, filter, changed || category != before)
}

pub(crate) fn window_width(ctx: &egui::Context, id: egui::Id, limit: f32, default: f32) -> f32 {
    ctx.data(|data| data.get_temp::<f32>(id)).unwrap_or(default)
        .clamp(limit.min(245.0), limit)
}

pub(crate) fn resize_grip(ui: &mut Ui, id: egui::Id, width: f32, limit: f32) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(24.0, 13.0), egui::Sense::drag());
        let colour = ui.style().interact(&response).fg_stroke.color;
        for line in 0..3 {
            let offset = 3.0 + line as f32 * 3.0;
            ui.painter().line_segment(
                [rect.right_bottom() - egui::vec2(offset, 2.0), rect.right_bottom() - egui::vec2(2.0, offset)],
                egui::Stroke::new(1.0, colour),
            );
        }
        let gesture_id = id.with("grip.drag");
        let (press, release) = crate::input::primary_button_positions(ui);
        let (pointer, holding) = ui.input(|input| (input.pointer.interact_pos(), input.pointer.primary_down()));
        let mut start = ui.data(|data| data.get_temp::<(f32, f32)>(gesture_id));
        if let Some(press) = press {
            start = (rect.contains(press) && ui.ctx().layer_id_at(press) == Some(ui.layer_id()))
                .then_some((press.x, width));
        }
        if let Some((origin, initial_width)) = start
            && let Some(point) = release.or_else(|| holding.then_some(pointer).flatten())
        {
            let next = (initial_width + point.x - origin).clamp(limit.min(245.0), limit);
            ui.data_mut(|data| data.insert_temp(id, next));
        }
        ui.data_mut(|data| {
            if holding && release.is_none() && let Some(start) = start {
                data.insert_temp(gesture_id, start);
            } else {
                data.remove::<(f32, f32)>(gesture_id);
            }
        });
        dtool_design::tooltip(response.on_hover_and_drag_cursor(egui::CursorIcon::ResizeHorizontal),
            text("toolbar.catalog.resize"));
    });
}

pub(crate) fn matches(id: &str, label: &str, filter: &str) -> bool {
    filter.is_empty() || id.to_lowercase().contains(filter) || label.to_lowercase().contains(filter)
}

pub(crate) fn columns(ui: &Ui, minimum: f32) -> usize {
    dtool_design::panel_column_count(ui, ui.available_width(), 4, minimum)
}

pub(crate) fn grid(ui: &mut Ui, count: usize, minimum: f32, mut cell: impl FnMut(&mut Ui, usize)) {
    let columns = columns(ui, minimum);
    for start in (0..count).step_by(columns) {
        ui.columns(columns, |cells| {
            for (index, ui) in (start..count).zip(cells) {
                cell(ui, index);
            }
        });
    }
}

pub(crate) fn tool_category(id: &str) -> Category {
    if id.starts_with("tool.picture_frame") || matches!(id, "tool.table" | "tool.text.frame") {
        Category::Layout
    } else if id != "tool.paint.vector_brush" && (
        id.starts_with("tool.paint") || id.starts_with("tool.eraser") || id.starts_with("tool.marquee")
        || matches!(id, "tool.crop" | "tool.selection_brush" | "tool.magic_wand" | "tool.flood_fill" | dtool_core::commands::fill::TOOL
            | "tool.undo_brush" | "tool.healing_brush" | "tool.patch" | "tool.blemish_removal" | "tool.retouch" | "tool.inpaint" | "tool.red_eye"
            | "tool.dodge" | "tool.burn" | "tool.sharpen" | "tool.perspective" | "tool.deform"))
    {
        Category::Pixel
    } else if matches!(id, "tool.view" | "tool.zoom" | "tool.move" | "tool.select.object" | "tool.text"
        | "tool.eyedropper" | "tool.measure" | "tool.slice" | "tool.artboard")
    {
        Category::Common
    } else {
        Category::Vector
    }
}

pub(crate) fn action_category(id: &str) -> Category {
    match id {
        "layer.pixel.add" | "selection.invert" | "selection.none" => Category::Pixel,
        "page.add" => Category::Layout,
        "toolbar.group.boolean" | "vector.convert_to_curves" => Category::Vector,
        id if id.starts_with("boolean.") => Category::Vector,
        _ => Category::Common,
    }
}
