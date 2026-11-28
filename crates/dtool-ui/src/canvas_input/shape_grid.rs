use dtool_core::Command;
use egui::{Align2, Event, Key, Painter, PointerButton, Rect, TextStyle, Ui, vec2};
use serde_json::json;

const MAX_CELLS: usize = 1024;

#[derive(Clone, Debug)]
pub(super) struct ShapeGrid {
    counts: [usize; 2],
    gaps: [f32; 2],
}

impl Default for ShapeGrid {
    fn default() -> Self {
        Self { counts: [1, 1], gaps: [0.0; 2] }
    }
}

pub(super) struct Layout {
    pos: [f32; 2],
    counts: [usize; 2],
    pitch: [f32; 2],
    pub(super) size: [f32; 2],
}

impl Layout {
    pub(super) fn positions(&self) -> impl Iterator<Item = [f32; 2]> + '_ {
        (0..self.counts[1]).flat_map(move |row| {
            (0..self.counts[0]).map(move |col| [
                self.pos[0] + self.pitch[0] * col as f32,
                self.pos[1] + self.pitch[1] * row as f32,
            ])
        })
    }
}

impl ShapeGrid {
    pub(super) fn handle_keys(&mut self, ui: &mut Ui, zoom: f32, starting: bool) {
        ui.input_mut(|input| {
            let mut held = !starting;
            input.events.retain(|event| {
                if let Event::PointerButton { button: PointerButton::Primary, pressed, .. } = event {
                    held = *pressed;
                    return true;
                }
                if !held {
                    return true;
                }
                let Event::Key { key, pressed: true, repeat, modifiers, .. } = event else { return true; };
                let (axis, increase) = match key {
                    Key::ArrowRight => (0, true),
                    Key::ArrowLeft => (0, false),
                    Key::ArrowDown => (1, true),
                    Key::ArrowUp => (1, false),
                    _ => return true,
                };
                if *repeat {
                    if self.counts[axis] > 1 {
                        let step = if modifiers.shift { 10.0 } else { 1.0 } / zoom.max(1e-4);
                        let delta = if increase { step } else { -step };
                        self.gaps[axis] = (self.gaps[axis] + delta).max(0.0);
                    }
                } else if increase {
                    self.counts[axis] = (self.counts[axis] + 1).min(MAX_CELLS / self.counts[1 - axis]);
                } else {
                    self.counts[axis] = self.counts[axis].saturating_sub(1).max(1);
                    if self.counts[axis] == 1 {
                        self.gaps[axis] = 0.0;
                    }
                }
                false
            });
        });
    }

    pub(super) fn layout(&self, pos: [f32; 2], extent: [f32; 2]) -> Layout {
        let gaps = std::array::from_fn::<_, 2, _>(|axis| {
            let count = self.counts[axis] as f32;
            // Keep every cell positive when the drag box is made smaller.
            self.gaps[axis].min(extent[axis] / count)
        });
        let size = std::array::from_fn(|axis| {
            let count = self.counts[axis] as f32;
            (extent[axis] - gaps[axis] * (count - 1.0)) / count
        });
        Layout {
            pos, counts: self.counts, size,
            pitch: std::array::from_fn(|axis| size[axis] + gaps[axis]),
        }
    }

    pub(super) fn paint_size(&self, ui: &Ui, painter: &Painter, rect: Rect) {
        if self.counts == [1, 1] {
            return;
        }
        let [columns, rows] = self.counts.map(|count| count.to_string());
        let label = dtool_strings::format("tool.shape_grid.size", &[("columns", &columns), ("rows", &rows)]);
        let ink = ui.visuals().text_color();
        let galley = painter.layout_no_wrap(label, TextStyle::Small.resolve(ui.style()), ink);
        let padding = vec2(7.0, 4.0);
        let size = galley.size() + padding * 2.0;
        if size.x > rect.width() - 16.0 || size.y > rect.height() - 16.0 {
            return;
        }
        let badge = Align2::RIGHT_BOTTOM.anchor_size(rect.right_bottom() - vec2(8.0, 8.0), size);
        painter.rect_filled(badge, dtool_design::size("radius.button"), ui.visuals().window_fill);
        painter.galley(badge.min + padding, galley, ink);
    }

    pub(super) fn command(self, mut shape: Command) -> Option<Command> {
        if self.counts == [1, 1] {
            return Some(shape);
        }
        let pos = serde_json::from_value(shape.params["pos"].clone()).ok()?;
        let size = serde_json::from_value(shape.params["size"].clone()).ok()?;
        let layout = self.layout(pos, size);
        shape.params["size"] = json!(layout.size);
        let commands: Vec<_> = layout.positions().map(|pos| {
            let mut cell = shape.clone();
            cell.params["pos"] = json!(pos);
            cell
        }).collect();
        Some(Command::with_params("edit.batch", json!({ "commands": commands })))
    }
}

