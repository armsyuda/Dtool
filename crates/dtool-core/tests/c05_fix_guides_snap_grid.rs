use dtool_core::document::{Document, Units};
use dtool_core::grid::{Grid, GridKind, GridSetArgs};
use dtool_core::guides::{GuideId, GuideKind, Guides};
use dtool_core::object::{Object, ObjectId};
use dtool_core::snap_candidates::{snap_candidates, CandidateKind, SnapDocument};
use dtool_core::snap_prefs::SnapPrefs;
use dtool_core::ViewState;
use serde_json::json;

fn prefs() -> SnapPrefs {
    let mut value = serde_json::to_value(SnapPrefs::default()).unwrap();
    for field in value.as_object_mut().unwrap().values_mut() {
        if field.is_boolean() {
            *field = json!(false);
        }
    }
    value["enabled"] = json!(true);
    value["screen_tolerance"] = json!(2);
    value["max_candidates"] = json!(32);
    serde_json::from_value(value).unwrap()
}

fn grid(kind: GridKind, origin: [f32; 2], angles: [f32; 2]) -> Grid {
    Grid { kind, show: true, spacing: [10.0, 10.0], subdivisions: 1, origin, angles, ..Grid::default() }
}

fn grid_points(doc: &Document, grid: &Grid, point: [f32; 2]) -> Vec<[f32; 2]> {
    let guides = Guides::default();
    let source = SnapDocument { document: doc, guides: &guides, grid };
    let mut options = prefs();
    options.snap_to_grid = true;
    snap_candidates(&source, &ViewState::default(), &options, point).into_iter().filter(|c| c.kind == CandidateKind::Grid).map(|c| c.point).collect()
}

fn near(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
}

#[test]
fn deleting_the_highest_guide_does_not_reuse_its_id() {
    let mut guides = Guides::default();
    for position in [1.0, 2.0, 3.0] {
        guides.add(GuideKind::Vertical, position, None).unwrap();
    }
    guides.delete(GuideId(3)).unwrap();
    assert_eq!(guides.add(GuideKind::Horizontal, 4.0, None).unwrap(), GuideId(4));
    guides.delete(GuideId(4)).unwrap();
    let mut reloaded: Guides = serde_json::from_value(serde_json::to_value(&guides).unwrap()).unwrap();
    assert_eq!(reloaded.add(GuideKind::Vertical, 5.0, None).unwrap(), GuideId(5));
}

#[test]
fn old_guides_without_high_water_load_and_continue_after_the_largest_id() {
    let mut guides: Guides = serde_json::from_value(json!({
        "items": [{ "id": 7, "kind": "Vertical", "position": 1.0 }],
        "show": true, "show_columns": true, "locked": false, "colour": null
    })).unwrap();
    assert_eq!(guides.next_guide_id, 0);
    assert_eq!(guides.add(GuideKind::Vertical, 2.0, None).unwrap(), GuideId(8));
}

#[test]
fn equal_distance_candidates_follow_kind_priority() {
    let mut doc = Document::new([400.0, 400.0], Units::Px, 96.0, 1);
    doc.objects.push(Object { id: ObjectId(1), pos: [10.0, 100.0], size: [20.0, 20.0], ..Object::default() });
    let mut guides = Guides::default();
    guides.add(GuideKind::Vertical, 10.0, None).unwrap();
    let grid = grid(GridKind::Standard, [0.0, 0.0], [30.0, 30.0]);
    let source = SnapDocument { document: &doc, guides: &guides, grid: &grid };
    let mut options = prefs();
    options.snap_to_guides = true;
    options.snap_to_grid = true;
    options.object_snapping = true;
    options.snap_to_object_bounds = true;
    options.force_pixel_alignment = true;
    let found = snap_candidates(&source, &ViewState::default(), &options, [10.5, 3.0]);
    let kinds: Vec<CandidateKind> = found.iter().take(4).map(|c| c.kind).collect();
    assert_eq!(kinds, vec![CandidateKind::Guide, CandidateKind::ObjectBounds, CandidateKind::Grid, CandidateKind::Pixel]);
    assert!(found.iter().take(4).all(|c| (c.distance - 0.5).abs() < 1e-4));
    assert!(CandidateKind::ObjectGeometry.priority() < CandidateKind::ObjectBounds.priority());
}

#[test]
fn grid_origin_defaults_to_zero_for_old_files_and_moves_candidates() {
    let old: Grid = serde_json::from_value(json!({ "kind": "Standard", "show": true, "spacing": [8.0, 8.0], "subdivisions": 2 })).unwrap();
    assert_eq!(old.origin, [0.0, 0.0]);
    assert_eq!(old.angles, [30.0, 30.0]);
    let mut grid = grid(GridKind::Standard, [0.0, 0.0], [30.0, 30.0]);
    grid.set(&GridSetArgs::parse(&json!({ "origin": [3.0, 0.0] })).unwrap()).unwrap();
    assert_eq!(grid.origin, [3.0, 0.0]);
    assert!(GridSetArgs::parse(&json!({ "origin": [1.0] })).is_err());
    let doc = Document::new([400.0, 400.0], Units::Px, 96.0, 1);
    assert!(grid_points(&doc, &grid, [12.6, 50.0]).iter().any(|p| near(*p, [13.0, 50.0])));
    let round: Grid = serde_json::from_value(serde_json::to_value(&grid).unwrap()).unwrap();
    assert_eq!(round, grid);
}

#[test]
fn axonometric_and_custom_grids_offer_lattice_candidates() {
    let doc = Document::new([400.0, 400.0], Units::Px, 96.0, 1);
    let axo = grid(GridKind::Axonometric, [5.0, 5.0], [30.0, 30.0]);
    assert!(grid_points(&doc, &axo, [5.3, -5.2]).iter().any(|p| near(*p, [5.0, -5.0])));
    let custom = grid(GridKind::Custom, [2.0, 0.0], [0.0, 0.0]);
    assert!(grid_points(&doc, &custom, [12.2, 9.6]).iter().any(|p| near(*p, [12.0, 10.0])));
    let degenerate = grid(GridKind::Axonometric, [0.0, 0.0], [0.0, 180.0]);
    assert!(grid_points(&doc, &degenerate, [1.0, 1.0]).is_empty());
}
