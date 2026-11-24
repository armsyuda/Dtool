#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::{doc_to_view, view_to_doc, ViewState, ZOOM_MAX, ZOOM_MIN};
use dtool_tools::{zoom_tool_click, zoom_tool_marquee};
use dtool_ui::shell::AppShell;

const SIZE: [f32; 2] = [800.0, 600.0];

fn shell() -> AppShell {
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("c02_zoom");
    shell.workspace.activate(id).unwrap();
    shell
}

fn near(a: [f32; 2], b: [f32; 2], tol: f32) -> bool {
    (a[0] - b[0]).abs() < tol && (a[1] - b[1]).abs() < tol
}

fn doc_state(shell: &mut AppShell) -> (u64, bool) {
    let s = shell.workspace.session_mut().unwrap();
    (s.document.version, s.undo.can_undo())
}

fn view(shell: &mut AppShell) -> &mut ViewState {
    &mut shell.workspace.session_mut().unwrap().view
}

#[test]
fn zoom_click_and_alt_click_keep_point_under_cursor_on_live_view() {
    let _home = test_home::enter("zoom_click_and_alt_click_keep_point_under_cursor_on_live_view");
    let mut s = shell();
    let before = doc_state(&mut s);
    for rotation in [0.0, 30.0] {
        let v = view(&mut s);
        v.zoom = 1.0;
        v.scroll = [12.0, -7.0];
        v.rotation_deg = rotation;
        let point = [321.0, 123.0];
        let anchor = view_to_doc(v, point);
        zoom_tool_click(v, point, false);
        assert!((v.zoom - 2.0).abs() < 1e-5);
        assert!(near(doc_to_view(v, anchor), point, 1e-2), "rot {rotation}");
        zoom_tool_click(v, point, true);
        zoom_tool_click(v, point, true);
        assert!((v.zoom - 0.5).abs() < 1e-5);
        assert!(near(doc_to_view(v, anchor), point, 1e-2), "rot {rotation}");
    }
    let v = view(&mut s);
    v.rotation_deg = 0.0;
    v.zoom = ZOOM_MAX;
    zoom_tool_click(v, [10.0, 10.0], false);
    assert_eq!(v.zoom, ZOOM_MAX);
    v.zoom = ZOOM_MIN;
    zoom_tool_click(v, [10.0, 10.0], true);
    assert_eq!(v.zoom, ZOOM_MIN);
    assert_eq!(doc_state(&mut s), before);
}

#[test]
fn zoom_marquee_fits_dragged_rect_and_ignores_degenerate_drags() {
    let _home = test_home::enter("zoom_marquee_fits_dragged_rect_and_ignores_degenerate_drags");
    let mut s = shell();
    let before = doc_state(&mut s);
    for (start, end) in [([100.0, 100.0], [300.0, 200.0]), ([300.0, 200.0], [100.0, 100.0])] {
        let v = view(&mut s);
        v.zoom = 1.0;
        v.scroll = [0.0, 0.0];
        v.rotation_deg = 0.0;
        let a = view_to_doc(v, start);
        let b = view_to_doc(v, end);
        let mid = view_to_doc(v, [(start[0] + end[0]) * 0.5, (start[1] + end[1]) * 0.5]);
        zoom_tool_marquee(v, start, end, SIZE);
        assert!((v.zoom - 4.0).abs() < 1e-4, "{}", v.zoom);
        assert!(near(doc_to_view(v, mid), [SIZE[0] * 0.5, SIZE[1] * 0.5], 1e-2));
        for corner in [doc_to_view(v, a), doc_to_view(v, b)] {
            assert!(corner[0] >= -1e-2 && corner[0] <= SIZE[0] + 1e-2 && corner[1] >= -1e-2 && corner[1] <= SIZE[1] + 1e-2, "{corner:?}");
        }
    }
    let v = view(&mut s);
    let snapshot = v.clone();
    zoom_tool_marquee(v, [50.0, 50.0], [50.0, 90.0], SIZE);
    zoom_tool_marquee(v, [50.0, 50.0], [90.0, 50.0], SIZE);
    zoom_tool_marquee(v, [50.0, 50.0], [90.0, 90.0], [0.0, 600.0]);
    zoom_tool_marquee(v, [50.0, 50.0], [90.0, 90.0], [f32::NAN, 600.0]);
    assert_eq!(*v, snapshot);
    v.zoom = 1.0;
    zoom_tool_marquee(v, [0.0, 0.0], [0.001, 0.001], SIZE);
    assert_eq!(v.zoom, ZOOM_MAX);
    assert_eq!(doc_state(&mut s), before);
}