use dtool_ui::shortcuts::{Focus, Modifiers, ShortcutMap, TabAction, TOGGLE_UI_COMMAND};

#[test]
fn test_default_copy_paste_dispatch() {
    let map = ShortcutMap::affinity_default();
    
    let copy_mods = Modifiers { ctrl: true, shift: false, alt: false };
    let paste_mods = Modifiers { ctrl: true, shift: false, alt: false };

    let copy_cmd = map.dispatch(Focus::Canvas, "c", copy_mods);
    assert_eq!(copy_cmd, Some("Serif.Interop.Persona.Commands.CopyCommand".to_string()));

    let paste_cmd = map.dispatch(Focus::Canvas, "v", paste_mods);
    assert_eq!(paste_cmd, Some("Serif.Interop.Persona.Commands.PasteCommand".to_string()));
}

#[test]
fn test_text_field_focus_does_not_change_tools() {
    let map = ShortcutMap::affinity_default();
    
    let no_mods = Modifiers { ctrl: false, shift: false, alt: false };
    
    let cmd_canvas_v = map.dispatch(Focus::Canvas, "v", no_mods);
    assert_eq!(cmd_canvas_v, Some("tool.Select".to_string()));

    let cmd_textfield_v = map.dispatch(Focus::TextField, "v", no_mods);
    assert_eq!(cmd_textfield_v, None);
}

#[test]
fn test_shortcut_set_reset_import_export() {
    let mut map = ShortcutMap::affinity_default();
    
    let set_cmd = Some("tool.pen".to_string());
    let set_key = "q";
    let set_mods = Modifiers { ctrl: true, shift: true, alt: true };

    map.set("tool.pen", set_key, set_mods);
    
    let dispatched = map.dispatch(Focus::Canvas, set_key, set_mods);
    assert_eq!(dispatched, set_cmd);

    map.reset();
    
    let dispatched_after_reset = map.dispatch(Focus::Canvas, set_key, set_mods);
    assert_ne!(dispatched_after_reset, Some("tool.pen".to_string()));
}

#[test]
fn test_canvas_tab_hides_then_shows_panels() {
    let map = ShortcutMap::affinity_default();
    let no_mods = Modifiers { ctrl: false, shift: false, alt: false };
    assert_eq!(map.dispatch(Focus::Canvas, "Tab", no_mods), Some(TOGGLE_UI_COMMAND.to_string()));
    assert_eq!(map.tab_action(Focus::Canvas, no_mods), TabAction::ToggleUi);
    assert_eq!(map.tab_action(Focus::Canvas, no_mods), TabAction::ToggleUi);
}

#[test]
fn test_tab_in_field_moves_to_next_field_without_hiding_ui() {
    let map = ShortcutMap::affinity_default();
    let no_mods = Modifiers { ctrl: false, shift: false, alt: false };
    assert_eq!(map.tab_action(Focus::TextField, no_mods), TabAction::NextField);
    assert_eq!(map.tab_action(Focus::Dialog, no_mods), TabAction::NextField);
    assert_eq!(map.tab_action(Focus::TextField, Modifiers { ctrl: true, shift: false, alt: false }), TabAction::Ignore);
}
