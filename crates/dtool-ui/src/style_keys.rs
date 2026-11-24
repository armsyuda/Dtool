use dtool_core::shortcuts::{Focus, Modifiers};
use dtool_core::{CommandError, Session};
use dtool_tools::style_picker::StylePickerTool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleKey {
    Copy,
    Paste,
}

pub fn style_key(focus: Focus, key: &str, modifiers: Modifiers) -> Option<StyleKey> {
    if focus != Focus::Canvas || !modifiers.ctrl || !modifiers.shift || modifiers.alt {
        return None;
    }
    match key.to_ascii_uppercase().as_str() {
        "C" => Some(StyleKey::Copy),
        "V" => Some(StyleKey::Paste),
        _ => None,
    }
}

pub fn run(tool: &mut StylePickerTool, session: &mut Session, key: StyleKey) -> Result<u64, CommandError> {
    match key {
        StyleKey::Copy => tool.pick(session),
        StyleKey::Paste => tool.apply(session),
    }
}

fn held(input: &egui::InputState) -> Modifiers {
    Modifiers { ctrl: input.modifiers.ctrl || input.modifiers.command, shift: input.modifiers.shift, alt: input.modifiers.alt }
}

pub fn event_key(event: &egui::Event, held: Modifiers) -> Option<StyleKey> {
    match event {
        egui::Event::Key { key: egui::Key::C, pressed: true, .. } | egui::Event::Copy => style_key(Focus::Canvas, "C", held),
        egui::Event::Key { key: egui::Key::V, pressed: true, .. } | egui::Event::Paste(_) => style_key(Focus::Canvas, "V", held),
        _ => None,
    }
}

fn pressed(input: &egui::InputState) -> Option<StyleKey> {
    let held = held(input);
    input.events.iter().find_map(|event| event_key(event, held))
}

pub fn consume(input: &mut egui::InputState) {
    let held = held(input);
    input.events.retain(|event| event_key(event, held).is_none());
}

pub fn is_plain_paste(event: &egui::Event, held: Modifiers) -> bool {
    matches!(event, egui::Event::Paste(_) | egui::Event::Key { key: egui::Key::V, pressed: true, .. }) && held.ctrl && !held.shift && !held.alt
}

pub fn handle(ctx: &egui::Context, session: &mut Session) -> Option<Result<u64, CommandError>> {
    if session.text_edit.is_some() || ctx.memory(|memory| memory.focused().is_some()) {
        return None;
    }
    let key = ctx.input(pressed)?;
    ctx.input_mut(consume);
    let id = egui::Id::unique("dtool.style_picker");
    let mut tool = ctx.data_mut(|data| data.get_temp::<StylePickerTool>(id)).unwrap_or_default();
    let result = run(&mut tool, session, key);
    ctx.data_mut(|data| data.insert_temp(id, tool));
    Some(result)
}

#[cfg(test)]
mod contracts {
    use super::*;

    #[test]
    fn ctrl_shift_c_v_map_to_style_copy_paste_on_canvas_only() {
        let both = Modifiers { ctrl: true, shift: true, alt: false };
        assert_eq!(style_key(Focus::Canvas, "c", both), Some(StyleKey::Copy));
        assert_eq!(style_key(Focus::Canvas, "V", both), Some(StyleKey::Paste));
        assert_eq!(style_key(Focus::TextField, "V", both), None);
        assert_eq!(style_key(Focus::Canvas, "V", Modifiers { ctrl: true, shift: false, alt: false }), None);
    }

    fn pass(modifiers: egui::Modifiers, events: Vec<egui::Event>) -> (egui::Context, egui::RawInput) {
        let ctx = egui::Context::default();
        let mut all = vec![egui::Event::ModifiersChanged(modifiers)];
        all.extend(events);
        let raw = egui::RawInput { events: all, ..Default::default() };
        (ctx, raw)
    }

    #[test]
    fn ctrl_shift_v_is_consumed_and_plain_paste_is_kept() {
        let mut session = Session::new(dtool_core::Document::new([100.0, 100.0], dtool_core::document::Units::Px, 72.0, 1));
        session.has_document = true;
        let both = egui::Modifiers { ctrl: true, shift: true, command: true, ..Default::default() };
        let (ctx, raw) = pass(both, vec![egui::Event::Paste("x".into()), egui::Event::Key { key: egui::Key::V, physical_key: None, pressed: true, repeat: false, modifiers: both }]);
        ctx.begin_pass(raw);
        assert!(handle(&ctx, &mut session).is_some());
        let left = ctx.input(|i| i.events.iter().filter(|e| matches!(e, egui::Event::Paste(_) | egui::Event::Key { .. })).count());
        let held = ctx.input(|i| Modifiers { ctrl: i.modifiers.ctrl, shift: i.modifiers.shift, alt: i.modifiers.alt });
        let plain = ctx.input(|i| i.events.iter().any(|e| is_plain_paste(e, held)));
        assert_eq!(left, 0);
        assert!(!plain);
        let ctrl = egui::Modifiers { ctrl: true, command: true, ..Default::default() };
        let (ctx, raw) = pass(ctrl, vec![egui::Event::Paste("x".into())]);
        ctx.begin_pass(raw);
        assert!(handle(&ctx, &mut session).is_none());
        let held = ctx.input(|i| Modifiers { ctrl: i.modifiers.ctrl, shift: i.modifiers.shift, alt: i.modifiers.alt });
        assert!(ctx.input(|i| i.events.iter().any(|e| is_plain_paste(e, held))));
    }
}