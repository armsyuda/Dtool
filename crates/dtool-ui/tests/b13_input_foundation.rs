use dtool_ui::input::{ImeEvent, InputEvent, InputHandler, ShortcutDispatcher};
use dtool_core::shortcuts::{Focus, Modifiers};

#[test]
fn test_ime_update_does_not_change_objects() {
    let mut handler = InputHandler::new();
    
    handler.handle_ime_event(ImeEvent::CompositionStart { text: String::from("initial") });
    assert!(handler.is_ime_composing());
    assert_eq!(handler.state().ime_text, "initial");
    
    handler.handle_ime_event(ImeEvent::CompositionUpdate { text: String::from("updated") });
    assert!(handler.is_ime_composing());
    assert_eq!(handler.state().ime_text, "updated");
    
    assert!(!handler.document_mutated_on_commit());
}

#[test]
fn test_ime_cancel_leaves_text_unchanged() {
    let mut handler = InputHandler::new();
    
    handler.handle_ime_event(ImeEvent::CompositionStart { text: String::from("start") });
    assert!(handler.is_ime_composing());
    assert_eq!(handler.state().ime_text, "start");
    
    handler.handle_ime_event(ImeEvent::CompositionUpdate { text: String::from("modified") });
    assert_eq!(handler.state().ime_text, "modified");
    
    handler.handle_ime_event(ImeEvent::CompositionCancel);
    assert!(!handler.is_ime_composing());
    assert_eq!(handler.state().ime_text, "");
    assert!(!handler.document_mutated_on_commit());
}

#[test]
fn test_tool_shortcut_not_dispatched_mid_composition() {
    let mut dispatcher = ShortcutDispatcher::new(InputHandler::new());
    let plain = Modifiers { ctrl: false, shift: false, alt: false };
    let before = dispatcher.dispatch_shortcut(Focus::Canvas, String::from("v"), plain);
    assert_eq!(before, Some(String::from("tool.Select")));

    dispatcher.handler_mut().handle_ime_event(ImeEvent::CompositionStart { text: String::from("composing") });

    let result = dispatcher.dispatch_shortcut(Focus::Canvas, String::from("v"), plain);

    assert!(result.is_none());
}

#[test]
fn test_gesture_does_not_start_during_ime() {
    let mut handler = InputHandler::new();
    
    handler.handle_ime_event(ImeEvent::CompositionStart { text: String::from("text") });
    
    handler.handle_input_event(InputEvent::PointerDown { x: 100.0, y: 200.0 });
    assert!(!handler.state().pointer_down);
    
    handler.handle_input_event(InputEvent::PointerMove { x: 150.0, y: 250.0 });
    assert_eq!(handler.state().last_pointer_pos, Some((150.0, 250.0)));
    
    handler.handle_ime_event(ImeEvent::CompositionCommit { text: String::from("committed") });
    assert!(!handler.is_ime_composing());
    assert!(handler.document_mutated_on_commit());
}

#[test]
fn test_key_events_blocked_during_composition() {
    let mut handler = InputHandler::new();
    
    handler.handle_ime_event(ImeEvent::CompositionStart { text: String::from("composing") });
    
    handler.handle_input_event(InputEvent::KeyDown {
        key: String::from("A"),
        modifiers: Modifiers { ctrl: false, shift: false, alt: false },
    });
    
    assert!(handler.is_ime_composing());
}

#[test]
fn test_normal_input_after_composition_commit() {
    let mut handler = InputHandler::new();
    
    handler.handle_ime_event(ImeEvent::CompositionStart { text: String::from("start") });
    handler.handle_ime_event(ImeEvent::CompositionCommit { text: String::from("done") });
    
    assert!(!handler.is_ime_composing());
    
    handler.handle_input_event(InputEvent::PointerDown { x: 50.0, y: 60.0 });
    assert!(handler.state().pointer_down);
}
