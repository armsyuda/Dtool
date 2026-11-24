use dtool_core::colour::model::ObjectIcc;
use dtool_core::colour::{BitDepth, Colour, ColourFormat, ColourModel, ColourPrefs, RenderingIntent};

fn cmyk() -> ColourFormat {
    ColourFormat { model: ColourModel::Cmyk, depth: BitDepth::U8 }
}

fn missing(intent: RenderingIntent) -> ObjectIcc {
    ObjectIcc { profile: Some("c07 no such profile".into()), intent: Some(intent) }
}

#[test]
fn c07_missing_profile_never_returns_naive_cmyk() {
    let prefs = ColourPrefs::default();
    let source = Colour::rgb(0.2, 0.4, 0.6, 0.5);
    let kept = source.clone().write_for_object(cmyk(), &prefs, Some(&missing(RenderingIntent::Perceptual)));
    assert_eq!(kept.model, ColourModel::Rgb);
    assert_eq!(kept.values, source.values);
    assert_eq!(kept.alpha, source.alpha);
}

#[test]
fn c07_missing_profile_intents_are_not_faked() {
    let prefs = ColourPrefs::default();
    let source = Colour::rgb(0.9, 0.3, 0.1, 1.0);
    let results: Vec<Colour> = [RenderingIntent::Perceptual, RenderingIntent::Saturation, RenderingIntent::AbsoluteColorimetric, RenderingIntent::RelativeColorimetric]
        .into_iter()
        .map(|intent| source.clone().write_for_object(cmyk(), &prefs, Some(&missing(intent))))
        .collect();
    assert!(results.iter().all(|colour| colour.values == source.values && colour.model == ColourModel::Rgb));
}

#[test]
fn c07_try_write_reports_missing_profile() {
    let prefs = ColourPrefs::default();
    let source = Colour::rgb(0.2, 0.4, 0.6, 1.0);
    let error = source.clone().try_write_for_object(cmyk(), &prefs, Some(&missing(RenderingIntent::Perceptual))).unwrap_err();
    assert_eq!(error.code, "COLOUR_PROFILE_MISSING");
    let converted = source.try_write_for_document(cmyk(), &prefs).expect("builtin FOGRA39L converts");
    assert_eq!(converted.model, ColourModel::Cmyk);
}