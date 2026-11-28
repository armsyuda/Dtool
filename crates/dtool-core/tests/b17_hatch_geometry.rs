use dtool_core::paint::{BitmapPaint, Colour, DiffusionCurveCreationMode, DiffusionPaint, GradientGeom, GradientKind, GradientStop, Hatch, HatchLine, HatchUnits, MeshPaint, Paint, PatternPaint, transform_paint_matrix};

const IDENTITY: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

fn hatch() -> Hatch {
    Hatch { lines: vec![HatchLine { angle_rad: 45.0_f64.to_radians(), origin: [0.0, 0.0], shift: 0.0, spacing: 10.0, dash: vec![] }], colour: Colour::new(20, 40, 60, 255), background: None, weight: 1.0, units: HatchUnits::Document, matrix: IDENTITY }
}

fn near(actual: f64, expected: f64) { assert!((actual - expected).abs() <= 1e-9, "{actual} != {expected}"); }

#[test]
fn gradient_stops_stay_normalized_and_ordered_after_matrix_transform() {
    let stops = vec![GradientStop { offset: 0.0, colour: Colour::new(0, 0, 0, 255) }, GradientStop { offset: 0.5, colour: Colour::new(127, 127, 127, 128) }, GradientStop { offset: 1.0, colour: Colour::new(255, 255, 255, 255) }];
    for kind in [GradientKind::Linear, GradientKind::Radial, GradientKind::Elliptical, GradientKind::Conical] {
        let mut paint = Paint::Gradient { kind, stops: stops.clone(), geom: GradientGeom::from_legacy_matrix(IDENTITY) };
        transform_paint_matrix(&mut paint, [2.0, 0.0, 0.0, 3.0, 5.0, 7.0]);
        let Paint::Gradient { stops: actual, geom, .. } = &paint else { panic!("gradient variant changed"); };
        assert_eq!(actual, &stops);
        assert!(actual.iter().all(|stop| (0.0..=1.0).contains(&stop.offset)));
        assert!(actual.windows(2).all(|pair| pair[0].offset <= pair[1].offset));
        let expected_sweep = (kind == GradientKind::Conical).then_some([0.0, 1.5]);
        let expected = GradientGeom::from_legacy_matrix(IDENTITY).transform([2.0, 0.0, 0.0, 3.0, 5.0, 7.0]);
        assert_eq!(*geom, GradientGeom { sweep_basis: expected_sweep, ..expected });
        assert_eq!(geom.start, [5.0, 7.0]);
        assert_eq!(geom.end, [7.0, 7.0]);
        let decoded: Paint = serde_json::from_value(serde_json::to_value(&paint).unwrap()).unwrap();
        assert_eq!(decoded, paint);
    }
}

#[test]
fn hatch_brief_45_degrees_spacing_10_is_stored_as_radians_and_clipped() {
    let hatch = hatch();
    near(hatch.lines[0].angle_rad, std::f64::consts::FRAC_PI_4);
    let stored = serde_json::to_value(&hatch).unwrap();
    near(stored["lines"][0]["angle_rad"].as_f64().unwrap(), std::f64::consts::FRAC_PI_4);
    let outline = [[0.0, 0.0], [40.0, 0.0], [40.0, 40.0], [0.0, 40.0]];
    let segments = hatch.clipped_segments(&outline).unwrap();
    assert!(!segments.is_empty());
    for segment in &segments {
        for point in [segment.start, segment.end] {
            assert!(point.iter().all(|value| *value >= -1e-9 && *value <= 40.0 + 1e-9));
            near((point[1] - point[0]) / 2.0_f64.sqrt(), segment.replica_index as f64 * 10.0);
        }
        near(segment.end[0] - segment.start[0], segment.end[1] - segment.start[1]);
    }
}

#[test]
fn hatch_affine_matrix_changes_emitted_geometry_without_mutating_line_parameters() {
    let mut hatch = hatch();
    hatch.lines[0].angle_rad = 0.0;
    hatch.matrix = [2.0, 0.0, 0.0, 3.0, 5.0, 7.0];
    let outline = [[5.0, 7.0], [45.0, 7.0], [45.0, 67.0], [5.0, 67.0]];
    let segments = hatch.clipped_segments(&outline).unwrap();
    let middle = segments.iter().find(|segment| segment.replica_index == 1).unwrap();
    near(middle.start[1], 37.0);
    near(middle.end[1], 37.0);
    near(middle.start[0].min(middle.end[0]), 5.0);
    near(middle.start[0].max(middle.end[0]), 45.0);
    near(hatch.lines[0].spacing, 10.0);
}

#[test]
fn all_ten_hatch_units_round_trip_with_matrix_and_order_intact() {
    let units = [HatchUnits::Document, HatchUnits::Px, HatchUnits::Pt, HatchUnits::Pc, HatchUnits::In, HatchUnits::Ft, HatchUnits::Yd, HatchUnits::Mm, HatchUnits::Cm, HatchUnits::M];
    let mut identities = std::collections::BTreeSet::new();
    for units in units {
        let mut original = hatch();
        original.units = units;
        original.matrix = [1.5, 0.25, -0.5, 2.0, 3.0, 4.0];
        let value = serde_json::to_value(&original).unwrap();
        identities.insert(value["units"].as_str().unwrap().to_string());
        let restored: Hatch = serde_json::from_value(value).unwrap();
        assert_eq!(restored, original);
    }
    assert_eq!(identities.len(), 10);
}

#[test]
fn hatch_line_order_and_actual_triangle_clip_are_preserved() {
    let mut hatch = hatch();
    hatch.lines.push(HatchLine { angle_rad: 0.0, origin: [0.0, 0.0], shift: 0.0, spacing: 5.0, dash: vec![] });
    let segments = hatch.clipped_segments(&[[0.0, 0.0], [20.0, 0.0], [0.0, 20.0]]).unwrap();
    assert!(segments.iter().any(|segment| segment.line_index == 0));
    assert!(segments.iter().any(|segment| segment.line_index == 1));
    assert!(segments.windows(2).all(|pair| pair[0].line_index <= pair[1].line_index));
    for segment in segments {
        for point in [segment.start, segment.end] {
            assert!(point[0] >= -1e-9 && point[1] >= -1e-9 && point[0] + point[1] <= 20.0 + 1e-9);
        }
    }
}

#[test]
fn eight_canonical_paint_variants_round_trip_without_solid_flattening() {
    let paints = [Paint::None, Paint::Solid { colour: Colour::new(1, 2, 3, 255) }, Paint::Gradient { kind: GradientKind::Linear, stops: vec![GradientStop { offset: 0.0, colour: Colour::new(0, 0, 0, 255) }, GradientStop { offset: 1.0, colour: Colour::new(255, 255, 255, 255) }], geom: GradientGeom::from_legacy_matrix(IDENTITY) }, Paint::Hatch(hatch()), Paint::Bitmap(BitmapPaint { resource_id: "b17.bitmap".into(), matrix: IDENTITY, seed: Some(3), single_image: false }), Paint::Pattern(PatternPaint { resource_id: "b17.pattern".into(), matrix: IDENTITY }), Paint::Mesh(MeshPaint::new()), Paint::Diffusion(DiffusionPaint::new(DiffusionCurveCreationMode::Manual))];
    for original in paints {
        let restored: Paint = serde_json::from_value(serde_json::to_value(&original).unwrap()).unwrap();
        assert_eq!(restored, original);
    }
}
