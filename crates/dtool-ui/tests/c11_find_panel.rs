use dtool_ui::find_panel::{FindPanel, FindPanelScope, FindRequest, FIND_SCOPES};

#[test]
fn c11_find_panel_requests_need_a_pattern() {
    let mut panel = FindPanel::default();
    assert_eq!(panel.scope, FindPanelScope::Document);
    assert!(panel.request(FindRequest::ReplaceAll).is_none());
    panel.pattern = "cat".into();
    assert_eq!(panel.request(FindRequest::Next { backwards: true }), Some(FindRequest::Next { backwards: true }));
    assert_eq!(panel.request(FindRequest::Replace), Some(FindRequest::Replace));
    assert_eq!(FIND_SCOPES.len(), 3);
    panel.report_found(Some(2));
    assert!(panel.status.as_deref().is_some_and(|s| s.ends_with(" 2")));
    panel.report_error();
    assert!(panel.status.is_some());
}

#[test]
fn c11_find_panel_draws_without_panicking() {
    let mut panel = FindPanel { pattern: "a".into(), ..FindPanel::default() };
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        assert!(panel.ui(ui).is_none());
    });
    output.textures_delta.clear();
}