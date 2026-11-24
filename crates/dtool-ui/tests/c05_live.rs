use dtool_core::grid::Grid;
use dtool_core::workspace::Workspace;
use dtool_core::ViewState;
use dtool_ui::guides_dialog::{guide_number, guide_number_header, guide_rows, GUIDE_NUMBER_KEY};
use dtool_core::guides::GuideKind;
use dtool_ui::rulers::{grid_lines, guide_kind_for, paint_grid, GridLine, RulerDropResult, RulerSide, Rulers, PIXEL_GRID_MIN_ZOOM};
use egui::{Context, Pos2, RawInput, Rect};
use serde_json::json;

const AREA: [[f32; 2]; 4] = [[0.0, 0.0], [200.0, 0.0], [200.0, 200.0], [0.0, 200.0]];

fn workspace() -> Workspace {
    let mut ws = Workspace::default();
    ws.new_document("c05");
    ws
}

fn run(ws: &mut Workspace, name: &str, params: serde_json::Value) -> serde_json::Value {
    ws.session_mut().unwrap().run_named(name, params).unwrap()
}

fn grid(ws: &Workspace) -> Grid {
    ws.session().unwrap().document.grid.clone()
}

fn vertical_xs(lines: &[GridLine], major: bool) -> Vec<i32> {
    let mut xs: Vec<i32> = lines.iter().filter(|l| l.major == major && (l.from[0] - l.to[0]).abs() < 1e-3).map(|l| l.from[0].round() as i32).collect();
    xs.sort();
    xs
}

fn horizontal_ys(lines: &[GridLine], major: bool) -> Vec<i32> {
    let mut ys: Vec<i32> = lines.iter().filter(|l| l.major == major && (l.from[1] - l.to[1]).abs() < 1e-3).map(|l| l.from[1].round() as i32).collect();
    ys.sort();
    ys
}

#[test]
fn grid_set_on_live_document_drives_lines_and_undo_hides_them() {
    let mut ws = workspace();
    assert!(grid_lines(&grid(&ws), 1.0, AREA).is_empty());
    run(&mut ws, "grid.set", json!({"show": true, "kind": "Standard", "spacing": [50.0, 40.0], "subdivisions": 1, "origin": [10.0, 5.0]}));
    let lines = grid_lines(&grid(&ws), 1.0, AREA);
    assert_eq!(vertical_xs(&lines, true), vec![-40, 10, 60, 110, 160, 210]);
    assert_eq!(horizontal_ys(&lines, true), vec![-35, 5, 45, 85, 125, 165, 205]);
    assert!(lines.iter().all(|l| l.major));
    run(&mut ws, "edit.undo", json!({}));
    assert!(!grid(&ws).show);
    assert!(grid_lines(&grid(&ws), 1.0, AREA).is_empty());
}

#[test]
fn subdivisions_add_minor_lines_between_majors_only() {
    let mut ws = workspace();
    run(&mut ws, "grid.set", json!({"show": true, "spacing": [100.0, 100.0], "subdivisions": 4}));
    let lines = grid_lines(&grid(&ws), 1.0, AREA);
    assert_eq!(vertical_xs(&lines, true), vec![0, 100, 200]);
    assert_eq!(vertical_xs(&lines, false), vec![25, 50, 75, 125, 150, 175]);
}

#[test]
fn dense_grid_is_skipped_when_zoomed_out() {
    let mut ws = workspace();
    run(&mut ws, "grid.set", json!({"show": true, "spacing": [10.0, 10.0], "subdivisions": 1}));
    assert!(!grid_lines(&grid(&ws), 1.0, AREA).is_empty());
    assert!(grid_lines(&grid(&ws), 0.1, AREA).is_empty());
}

#[test]
fn isometric_and_axonometric_line_directions_follow_kind_and_angles() {
    let mut ws = workspace();
    run(&mut ws, "grid.set", json!({"show": true, "kind": "Isometric", "spacing": [40.0, 40.0], "subdivisions": 1}));
    let iso = grid_lines(&grid(&ws), 1.0, AREA);
    let slopes = |lines: &[GridLine]| {
        let mut s: Vec<i32> = lines.iter().map(|l| {
            let d = [l.to[0] - l.from[0], l.to[1] - l.from[1]];
            (d[1].atan2(d[0]).to_degrees().rem_euclid(180.0)).round() as i32
        }).collect();
        s.sort();
        s.dedup();
        s
    };
    assert_eq!(slopes(&iso).len(), 3, "{:?}", slopes(&iso));
    assert!(slopes(&iso).contains(&0));
    run(&mut ws, "grid.set", json!({"kind": "Axonometric", "angles": [20.0, 40.0]}));
    let axo = grid_lines(&grid(&ws), 1.0, AREA);
    assert_eq!(slopes(&axo), vec![40, 160]);
    run(&mut ws, "grid.set", json!({"kind": "Custom", "angles": [15.0, 0.0]}));
    assert_eq!(slopes(&grid_lines(&grid(&ws), 1.0, AREA)), vec![15, 105]);
}

#[test]
fn pixel_grid_only_when_zoomed_in() {
    let mut ws = workspace();
    run(&mut ws, "grid.set", json!({"show_pixel": true}));
    let small = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
    assert!(grid_lines(&grid(&ws), 1.0, small).is_empty());
    let lines = grid_lines(&grid(&ws), PIXEL_GRID_MIN_ZOOM, small);
    assert_eq!(vertical_xs(&lines, false), vec![0, 1, 2, 3, 4]);
}

#[test]
fn canvas_paints_grid_lines_from_live_document() {
    let mut ws = workspace();
    let ctx = Context::default();
    let canvas = Rect::from_min_size(Pos2::new(0.0, 0.0), egui::vec2(300.0, 200.0));
    let painted = |ws: &Workspace| {
        let mut n = 0;
        let mut out = ctx.run_ui(RawInput { screen_rect: Some(canvas), ..Default::default() }, |ui| {
            n = paint_grid(ui, &ws.session().unwrap().document.grid, &ViewState::default(), canvas);
        });
        out.textures_delta.clear();
        (n, out.shapes.len())
    };
    let (none, base_shapes) = painted(&ws);
    assert_eq!(none, 0);
    run(&mut ws, "grid.set", json!({"show": true, "spacing": [50.0, 50.0], "subdivisions": 1}));
    let (n, shapes) = painted(&ws);
    assert!(n > 4, "{n}");
    assert!(shapes >= base_shapes + n, "{shapes} {base_shapes} {n}");
}

#[test]
fn guides_manager_shows_stable_guide_numbers() {
    let mut ws = workspace();
    for p in [10.0, 20.0, 30.0] {
        run(&mut ws, "guides.add", json!({"kind": "Horizontal", "position": p}));
    }
    let id3 = guide_rows(ws.session().unwrap())[2].id;
    run(&mut ws, "guides.delete", json!({"id": id3}));
    let added = run(&mut ws, "guides.add", json!({"kind": "Vertical", "position": 5.0}))["id"].as_u64().unwrap();
    assert!(added > id3);
    let numbers: Vec<String> = guide_rows(ws.session().unwrap()).iter().map(guide_number).collect();
    assert_eq!(numbers.last().unwrap(), &added.to_string());
    let header = guide_number_header();
    assert_ne!(header, GUIDE_NUMBER_KEY);
}

#[test]
fn snap_prefs_global_but_guides_and_grid_per_document() {
    let mut ws = Workspace::default();
    let a = ws.new_document("a");
    let b = ws.new_document("b");
    ws.activate(a).unwrap();
    run(&mut ws, "snap.prefs.set", json!({"screen_tolerance": 23}));
    run(&mut ws, "guides.add", json!({"kind": "Horizontal", "position": 12.0}));
    run(&mut ws, "grid.set", json!({"show": true}));
    ws.activate(b).unwrap();
    assert_eq!(run(&mut ws, "snap.prefs.query", json!({}))["screen_tolerance"], json!(23));
    assert!(ws.session().unwrap().guides.list().is_empty());
    assert!(!grid(&ws).show);
}
#[test]
fn alt_does_not_flip_ruler_guide_orientation() {
    for alt in [false, true] {
        assert_eq!(guide_kind_for(RulerSide::Top, alt), GuideKind::Horizontal);
        assert_eq!(guide_kind_for(RulerSide::Left, alt), GuideKind::Vertical);
    }
    let mut ws = workspace();
    let mut rulers = Rulers::default();
    rulers.begin_new_guide(RulerSide::Top, [30.0, 0.0], true);
    rulers.update_drag([30.0, 40.0], true, false);
    let result = rulers.finish_drag(ws.session_mut().unwrap());
    assert!(matches!(result, RulerDropResult::Added(_)), "{result:?}");
    let guides = ws.session().unwrap().guides.list().to_vec();
    assert_eq!(guides.len(), 1);
    assert_eq!(guides[0].kind, GuideKind::Horizontal);
    assert_eq!(guides[0].position, 40.0);
    run(&mut ws, "edit.undo", json!({}));
    assert!(ws.session().unwrap().guides.list().is_empty());
}

#[test]
fn escape_and_drop_on_ruler_add_nothing_and_existing_guide_dropped_on_ruler_is_deleted() {
    let mut ws = workspace();
    let mut rulers = Rulers::default();
    rulers.begin_new_guide(RulerSide::Left, [0.0, 0.0], false);
    rulers.update_drag([25.0, 10.0], false, false);
    rulers.cancel_drag();
    assert_eq!(rulers.finish_drag(ws.session_mut().unwrap()), RulerDropResult::None);
    rulers.begin_new_guide(RulerSide::Left, [0.0, 0.0], false);
    rulers.update_drag([2.0, 10.0], false, true);
    assert_eq!(rulers.finish_drag(ws.session_mut().unwrap()), RulerDropResult::None);
    assert!(ws.session().unwrap().guides.list().is_empty());
    let id = run(&mut ws, "guides.add", json!({"kind": "Vertical", "position": 50.0}))["id"].as_u64().unwrap();
    let gid = ws.session().unwrap().guides.list()[0].id;
    assert!(rulers.begin_move_guide(ws.session().unwrap(), gid, [50.0, 10.0]));
    rulers.update_drag([1.0, 10.0], false, true);
    assert!(matches!(rulers.finish_drag(ws.session_mut().unwrap()), RulerDropResult::Deleted(_)));
    assert!(ws.session().unwrap().guides.list().iter().all(|g| g.id.0 != id));
    run(&mut ws, "edit.undo", json!({}));
    assert_eq!(ws.session().unwrap().guides.list()[0].position, 50.0);
}