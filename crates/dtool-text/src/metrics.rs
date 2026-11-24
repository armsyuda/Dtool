use serde::{Deserialize, Serialize};

use crate::fonts::{FontEngine, FontFace, FontId, FontLibrary};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MetricsSource {
    Actual,
    Approximate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontMetricsRequest {
    pub font: String,
    pub size_pt: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FontMetrics {
    pub ascent_pt: f32,
    pub descent_pt: f32,
    pub line_gap_pt: f32,
    pub cap_height_pt: f32,
    pub x_height_pt: f32,
    pub source: MetricsSource,
}

impl FontMetrics {
    pub fn approximate(size_pt: f32) -> Self {
        Self {
            ascent_pt: size_pt * 0.8,
            descent_pt: size_pt * 0.2,
            line_gap_pt: 0.0,
            cap_height_pt: size_pt * 0.7,
            x_height_pt: size_pt * 0.5,
            source: MetricsSource::Approximate,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MetricsReason {
    EmptyFamily,
    InvalidSize,
    FontNotFound,
    FaceLoadFailed,
    InvalidFont,
    ZeroUnitsPerEm,
    NoCapHeight,
    NoXHeight,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceMetricsRequest {
    pub face: FontId,
    pub size_pt: f32,
    #[serde(default)]
    pub variations: Vec<(String, f32)>,
    #[serde(default)]
    pub language: Option<String>,
}

impl FaceMetricsRequest {
    pub fn new(face: FontId, size_pt: f32) -> Self {
        Self {
            face,
            size_pt,
            variations: Vec::new(),
            language: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontMetricsReport {
    pub metrics: FontMetrics,
    pub face: Option<FontId>,
    pub language: Option<String>,
    pub reasons: Vec<MetricsReason>,
}

impl FontMetricsReport {
    fn fallback(size_pt: f32, face: Option<FontId>, language: Option<String>, reason: MetricsReason) -> Self {
        Self {
            metrics: FontMetrics::approximate(size_pt),
            face,
            language,
            reasons: vec![reason],
        }
    }

    pub fn is_actual(&self) -> bool {
        self.metrics.source == MetricsSource::Actual
    }
}

fn valid_size(size_pt: f32) -> bool {
    size_pt.is_finite() && size_pt > 0.0
}

fn face_blob(lib: &FontLibrary, face: &FontFace) -> Option<parley::fontique::Blob<u8>> {
    let mut engine = lib.engine.lock().unwrap_or_else(|e| e.into_inner());
    let FontEngine { fonts, .. } = &mut *engine;
    let family = fonts.collection.family_by_name(&face.family)?;
    for info in family.fonts() {
        if info.index() != face.index {
            continue;
        }
        let same_path = match info.source().kind() {
            parley::fontique::SourceKind::Path(path) => path.as_ref() == face.path.as_path(),
            _ => face.path.as_os_str().is_empty(),
        };
        if !same_path {
            continue;
        }
        return info.load(Some(&mut fonts.source_cache));
    }
    None
}

pub fn face_metrics(lib: &FontLibrary, request: &FaceMetricsRequest) -> FontMetricsReport {
    use skrifa::instance::Size;
    use skrifa::MetadataProvider;

    let language = request.language.clone();
    let size_pt = request.size_pt;
    if !valid_size(size_pt) {
        return FontMetricsReport::fallback(0.0, None, language, MetricsReason::InvalidSize);
    }
    let Some(face) = lib.face(request.face) else {
        return FontMetricsReport::fallback(size_pt, None, language, MetricsReason::FontNotFound);
    };
    let Some(blob) = face_blob(lib, face) else {
        return FontMetricsReport::fallback(size_pt, Some(face.id), language, MetricsReason::FaceLoadFailed);
    };
    let Ok(font) = skrifa::FontRef::from_index(blob.data(), face.index) else {
        return FontMetricsReport::fallback(size_pt, Some(face.id), language, MetricsReason::InvalidFont);
    };

    let location = font
        .axes()
        .location(request.variations.iter().map(|(tag, value)| (tag.as_str(), *value)));
    let metrics = font.metrics(Size::unscaled(), &location);
    let units_per_em = metrics.units_per_em as f32;
    if units_per_em <= 0.0 {
        return FontMetricsReport::fallback(size_pt, Some(face.id), language, MetricsReason::ZeroUnitsPerEm);
    }

    let scale = size_pt / units_per_em;
    let ascent_pt = metrics.ascent * scale;
    let descent_pt = metrics.descent.abs() * scale;
    let line_gap_pt = metrics.leading * scale;
    if !(ascent_pt.is_finite() && descent_pt.is_finite() && line_gap_pt.is_finite() && ascent_pt + descent_pt > 0.0) {
        return FontMetricsReport::fallback(size_pt, Some(face.id), language, MetricsReason::InvalidFont);
    }

    let approx = FontMetrics::approximate(size_pt);
    let mut reasons = Vec::new();
    let cap_height_pt = match metrics.cap_height {
        Some(v) if v.is_finite() && v > 0.0 => v * scale,
        _ => {
            reasons.push(MetricsReason::NoCapHeight);
            approx.cap_height_pt
        }
    };
    let x_height_pt = match metrics.x_height {
        Some(v) if v.is_finite() && v > 0.0 => v * scale,
        _ => {
            reasons.push(MetricsReason::NoXHeight);
            approx.x_height_pt
        }
    };
    let source = if reasons.is_empty() { MetricsSource::Actual } else { MetricsSource::Approximate };

    FontMetricsReport {
        metrics: FontMetrics {
            ascent_pt,
            descent_pt,
            line_gap_pt,
            cap_height_pt,
            x_height_pt,
            source,
        },
        face: Some(face.id),
        language,
        reasons,
    }
}

pub fn family_metrics(lib: &FontLibrary, family: &str, style: &str, size_pt: f32) -> FontMetricsReport {
    let family = family.trim();
    if family.is_empty() {
        return FontMetricsReport::fallback(size_pt, None, None, MetricsReason::EmptyFamily);
    }
    if !valid_size(size_pt) {
        return FontMetricsReport::fallback(0.0, None, None, MetricsReason::InvalidSize);
    }
    let id = lib
        .resolve(family, style)
        .or_else(|| lib.faces(family).first().map(|f| f.id));
    let Some(id) = id else {
        return FontMetricsReport::fallback(size_pt, None, None, MetricsReason::FontNotFound);
    };
    face_metrics(lib, &FaceMetricsRequest::new(id, size_pt))
}

pub fn font_metrics(request: &FontMetricsRequest) -> FontMetrics {
    if request.font.trim().is_empty() {
        return FontMetrics::approximate(request.size_pt);
    }
    family_metrics(&FontLibrary::shared(), &request.font, "Regular", request.size_pt).metrics
}

#[cfg(test)]
mod tests {
    use super::*;

    const CANDIDATES: [&str; 6] = ["Malgun Gothic", "Segoe UI", "Arial", "Calibri", "Times New Roman", "DejaVu Sans"];

    fn installed(lib: &FontLibrary) -> Option<(FontId, &'static str)> {
        CANDIDATES.iter().find_map(|f| lib.resolve(f, "Regular").map(|id| (id, *f)))
    }

    #[test]
    fn installed_font_reports_actual_from_os2() {
        use skrifa::raw::TableProvider;
        let lib = FontLibrary::system();
        let Some((id, _)) = installed(&lib) else {
            eprintln!("skip: no candidate system font");
            return;
        };
        let report = face_metrics(&lib, &FaceMetricsRequest::new(id, 12.0));
        assert_eq!(report.metrics.source, MetricsSource::Actual, "{:?}", report.reasons);
        assert!(report.reasons.is_empty());
        assert_eq!(report.face, Some(id));
        assert!(report.metrics.ascent_pt > 0.0);
        assert!(report.metrics.descent_pt > 0.0);

        let face = lib.face(id).unwrap();
        let blob = face_blob(&lib, face).unwrap();
        let font = skrifa::FontRef::from_index(blob.data(), face.index).unwrap();
        let upem = font.head().unwrap().units_per_em() as f32;
        let os2 = font.os2().unwrap();
        let cap = os2.s_cap_height().unwrap() as f32 * 12.0 / upem;
        let x = os2.sx_height().unwrap() as f32 * 12.0 / upem;
        assert!((report.metrics.cap_height_pt - cap).abs() < 1e-3);
        assert!((report.metrics.x_height_pt - x).abs() < 1e-3);
        assert_ne!(report.metrics, FontMetrics::approximate(12.0));
    }

    #[test]
    fn installed_family_via_compat_api_is_actual() {
        let lib = FontLibrary::shared();
        let Some((_, family)) = installed(&lib) else {
            eprintln!("skip: no candidate system font");
            return;
        };
        let metrics = font_metrics(&FontMetricsRequest { font: family.into(), size_pt: 12.0 });
        assert_eq!(metrics.source, MetricsSource::Actual);
    }

    #[test]
    fn unknown_axis_is_ignored() {
        let lib = FontLibrary::system();
        let Some((id, _)) = installed(&lib) else {
            eprintln!("skip: no candidate system font");
            return;
        };
        let plain = face_metrics(&lib, &FaceMetricsRequest::new(id, 12.0));
        let mut request = FaceMetricsRequest::new(id, 12.0);
        request.variations.push(("zzzz".into(), 1.0));
        request.variations.push(("bad".into(), 1.0));
        let varied = face_metrics(&lib, &request);
        assert_eq!(plain.metrics, varied.metrics);
    }

    #[test]
    fn missing_family_is_approximate_with_reason() {
        let lib = FontLibrary::system();
        let report = family_metrics(&lib, "nonexistent_font_xyz", "Regular", 12.0);
        assert_eq!(report.metrics, FontMetrics::approximate(12.0));
        assert_eq!(report.reasons, vec![MetricsReason::FontNotFound]);
        assert_eq!(report.face, None);
        assert!(!report.is_actual());
    }

    #[test]
    fn missing_face_id_is_approximate_with_reason() {
        let lib = FontLibrary::system();
        let report = face_metrics(&lib, &FaceMetricsRequest::new(FontId(u32::MAX), 12.0));
        assert_eq!(report.metrics, FontMetrics::approximate(12.0));
        assert_eq!(report.reasons, vec![MetricsReason::FontNotFound]);
    }

    #[test]
    fn empty_family_and_invalid_size_have_reasons() {
        let lib = FontLibrary::system();
        let empty = family_metrics(&lib, "  ", "Regular", 12.0);
        assert_eq!(empty.reasons, vec![MetricsReason::EmptyFamily]);
        assert_eq!(empty.metrics, FontMetrics::approximate(12.0));
        for size in [0.0, -3.0, f32::NAN, f32::INFINITY] {
            let report = family_metrics(&lib, "Arial", "Regular", size);
            assert_eq!(report.reasons, vec![MetricsReason::InvalidSize]);
            assert!(report.metrics.ascent_pt.is_finite());
            assert!(report.metrics.cap_height_pt.is_finite());
        }
    }

    #[test]
    fn compat_empty_font_matches_approximate() {
        assert_eq!(font_metrics(&FontMetricsRequest { font: String::new(), size_pt: 12.0 }), FontMetrics::approximate(12.0));
    }

    #[test]
    fn actual_metrics_scale_linearly() {
        let lib = FontLibrary::system();
        let Some((id, _)) = installed(&lib) else {
            eprintln!("skip: no candidate system font");
            return;
        };
        let small = face_metrics(&lib, &FaceMetricsRequest::new(id, 10.0)).metrics;
        let large = face_metrics(&lib, &FaceMetricsRequest::new(id, 20.0)).metrics;
        assert!((large.ascent_pt - small.ascent_pt * 2.0).abs() < 1e-3);
        assert!((large.cap_height_pt - small.cap_height_pt * 2.0).abs() < 1e-3);
    }
}