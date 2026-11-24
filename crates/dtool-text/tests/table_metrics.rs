use dtool_text::metrics::{font_metrics, FontMetrics, FontMetricsRequest, MetricsSource};

fn request(font: &str, size_pt: f32) -> FontMetricsRequest {
    FontMetricsRequest {
        font: font.into(),
        size_pt,
    }
}

#[test]
fn test_font_metrics_empty_font_matches_approximate() {
    let metrics = font_metrics(&request("", 12.0));

    assert_eq!(metrics, FontMetrics::approximate(12.0));
    assert!(metrics.cap_height_pt > 0.0);
    assert!(metrics.x_height_pt > 0.0);
}

#[test]
fn test_font_metrics_missing_font_approximate() {
    let metrics = font_metrics(&request("nonexistent_font_xyz", 12.0));

    assert_eq!(metrics.source, MetricsSource::Approximate);
    assert!(metrics.ascent_pt > 0.0);
}

#[test]
fn test_font_metrics_no_cap_height_marks_approximate() {
    let metrics = font_metrics(&request("font_without_cap_height", 12.0));

    assert_eq!(metrics.source, MetricsSource::Approximate);
    assert!(metrics.cap_height_pt > metrics.x_height_pt);
}

#[test]
fn test_font_metrics_approximate_constructor() {
    let metrics = FontMetrics::approximate(12.0);

    assert_eq!(metrics.source, MetricsSource::Approximate);
    assert!(metrics.ascent_pt > 0.0);
    assert!(metrics.descent_pt > 0.0);
    assert_eq!(metrics.ascent_pt + metrics.descent_pt, 12.0);
    assert_eq!(metrics.line_gap_pt, 0.0);
}

#[test]
fn test_font_metrics_scale_with_size() {
    let small = font_metrics(&request("", 10.0));
    let large = font_metrics(&request("", 20.0));

    assert_eq!(large.ascent_pt, small.ascent_pt * 2.0);
    assert_eq!(large.x_height_pt, small.x_height_pt * 2.0);
}