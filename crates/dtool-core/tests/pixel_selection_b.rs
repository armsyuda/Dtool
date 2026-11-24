use dtool_core::raster_view::RasterView;
use dtool_core::selection::{brush_mask, colour_mask, ellipse_mask, parse_length, rect_mask};
use serde_json::json;

#[test]
fn ellipse_twelve_by_six_has_area_eighteen_pi_within_two_percent_and_zero_corner() {
    let mask = ellipse_mask(16, 12, [2.0, 3.0, 12.0, 6.0], true);
    let expected = 18.0 * std::f64::consts::PI;
    assert!((mask.area() - expected).abs() <= expected * 0.02, "area {} expected {expected}", mask.area());
    assert_eq!(mask.get(2, 3), 0);
    assert_eq!(mask.get(0, 0), 0);
    assert_eq!(mask.bounds(), Some([2, 3, 12, 6]));
}

#[test]
fn one_pixel_selection_brush_adds_only_four_four_and_preserves_rgba() {
    let rgba = vec![37u8; 8 * 8 * 4];
    let before = rgba.clone();
    let mask = brush_mask(8, 8, &[[4.5, 4.5, 1.0]], 1.0, 1.0);
    assert_eq!(mask.area(), 1.0);
    assert_eq!(mask.get(4, 4), 255);
    for y in 0..8 { for x in 0..8 {
        assert_eq!(mask.get(x, y), if (x, y) == (4, 4) {255} else {0});
    }}
    assert_eq!(rgba, before);
}

#[test]
fn colour_range_selects_disconnected_red_islands_and_excludes_zero_alpha() {
    let mut rgba = vec![0u8; 8 * 4 * 4];
    for pixel in rgba.chunks_exact_mut(4) { pixel.copy_from_slice(&[0, 0, 255, 255]); }
    for (x, y) in [(1, 1), (1, 2), (6, 1), (6, 2)] {
        let i = (y * 8 + x) * 4;
        rgba[i..i + 4].copy_from_slice(&[255, 0, 0, 255]);
    }
    rgba[0..4].copy_from_slice(&[255, 0, 0, 0]);
    let before = rgba.clone();
    let mask = colour_mask(&RasterView::new(8, 4, &rgba), [255, 0, 0, 255], 0.0);
    assert_eq!(mask.area(), 4.0);
    for (x, y) in [(1, 1), (1, 2), (6, 1), (6, 2)] { assert_eq!(mask.get(x, y), 255); }
    assert_eq!(mask.get(0, 0), 0);
    assert_eq!(mask.get(4, 1), 0);
    assert_eq!(rgba, before);
}

#[test]
fn shrink_three_by_three_keeps_only_centre_and_one_pixel_line_becomes_empty() {
    let mut square = rect_mask(7, 7, [2.0, 2.0, 3.0, 3.0], false);
    square.shrink(1.0, false);
    assert_eq!(square.area(), 1.0);
    assert_eq!(square.bounds(), Some([3, 3, 1, 1]));
    assert_eq!(square.get(3, 3), 255);
    let mut line = rect_mask(7, 7, [3.0, 1.0, 1.0, 5.0], false);
    line.shrink(1.0, false);
    assert!(line.is_empty());
}

#[test]
fn row_three_point_seventy_five_points_height_zero_point_seventy_five_selects_row_five() {
    let y = parse_length(&json!("3.75pt"), 96.0).unwrap();
    let height = parse_length(&json!("0.75pt"), 96.0).unwrap();
    assert_eq!((y, height), (5.0, 1.0));
    let mask = rect_mask(8, 8, [0.0, y, 8.0, height], false);
    assert_eq!(mask.area(), 8.0);
    for row in 0..8 { for x in 0..8 { assert_eq!(mask.get(x, row), if row == 5 {255} else {0}); }}
}

#[test]
fn feather_produces_soft_edge_values_between_one_and_two_hundred_fifty_four() {
    let mut mask = rect_mask(16, 16, [4.0, 4.0, 8.0, 8.0], false);
    let before = mask.clone();
    mask.feather(2.0);
    assert_ne!(mask, before);
    assert!((1..=254).contains(&mask.get(4, 8)));
    assert!((1..=254).contains(&mask.get(3, 8)));
    assert!(mask.get(8, 8) > mask.get(4, 8));
}
