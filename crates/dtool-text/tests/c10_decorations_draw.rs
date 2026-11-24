use dtool_core::paint::{Colour, Paint};
use dtool_text::char_style::Decoration;
use dtool_text::decorations::{line_metrics, story_decorations, DecorationKind, DecorationRect};
use dtool_text::{CharStyle, FontLibrary, Run, Story, TextRange};

fn solid(r: u8, g: u8, b: u8) -> Paint {
    Paint::Solid { colour: Colour::new(r, g, b, 255) }
}

fn arial() -> CharStyle {
    CharStyle { family: "Arial".into(), style: "Regular".into(), size_pt: 20.0, ..CharStyle::default() }
}

fn two_runs(first: CharStyle, second: CharStyle) -> Story {
    Story { text: "abcd".into(), chars: vec![Run { range: TextRange::new(0, 2), style: first }, Run { range: TextRange::new(2, 4), style: second }], ..Story::default() }
}

fn kind(rects: &[DecorationRect], kind: DecorationKind) -> Vec<DecorationRect> {
    rects.iter().filter(|r| r.kind == kind).cloned().collect()
}

fn on() -> Decoration {
    Decoration { on: true, ..Decoration::default() }
}

#[test]
fn c10_two_runs_give_two_background_boxes() {
    let lib = FontLibrary::system();
    let story = two_runs(CharStyle { background: Some(solid(255, 255, 0)), ..arial() }, CharStyle { background: Some(solid(0, 255, 255)), ..arial() });
    let rects = story_decorations(&lib, &story, None, &solid(0, 0, 0));
    let boxes = kind(&rects, DecorationKind::Background);
    assert_eq!(boxes.len(), 2, "{rects:?}");
    assert_eq!(boxes[0].range, TextRange::new(0, 2));
    assert_eq!(boxes[1].range, TextRange::new(2, 4));
    assert!((boxes[0].rect[0] + boxes[0].rect[2] - boxes[1].rect[0]).abs() < 0.01);
    assert_eq!(boxes[0].paint, solid(255, 255, 0));
    assert_eq!(boxes[1].paint, solid(0, 255, 255));
    let metrics = line_metrics(&lib, &arial());
    assert!((boxes[0].rect[3] - (metrics.ascent + metrics.descent)).abs() < 0.01);
    assert!(rects.iter().position(|r| r.kind == DecorationKind::Background).unwrap() == 0);
}

#[test]
fn c10_underline_covers_only_its_run() {
    let lib = FontLibrary::system();
    let story = two_runs(arial(), CharStyle { underline: on(), ..arial() });
    let rects = story_decorations(&lib, &story, None, &solid(10, 20, 30));
    let lines = kind(&rects, DecorationKind::Underline);
    assert_eq!(lines.len(), 1, "{rects:?}");
    assert_eq!(lines[0].range, TextRange::new(2, 4));
    let full = story_decorations(&lib, &two_runs(CharStyle { underline: on(), ..arial() }, CharStyle { underline: on(), ..arial() }), None, &solid(10, 20, 30));
    let all = kind(&full, DecorationKind::Underline);
    assert_eq!(all.len(), 2);
    assert!((lines[0].rect[0] - all[1].rect[0]).abs() < 0.01);
    assert!(lines[0].rect[0] > all[0].rect[0] + 1.0);
    assert_eq!(lines[0].paint, solid(10, 20, 30));
    let metrics = line_metrics(&lib, &arial());
    assert!(lines[0].rect[1] > 0.0 && metrics.underline_offset < 0.0);
}

#[test]
fn c10_strike_uses_the_font_value() {
    let lib = FontLibrary::system();
    let metrics = line_metrics(&lib, &arial());
    assert!(metrics.actual, "{metrics:?}");
    let story = two_runs(CharStyle { strike: on(), ..arial() }, arial());
    let rects = story_decorations(&lib, &story, None, &solid(0, 0, 0));
    let strikes = kind(&rects, DecorationKind::Strike);
    assert_eq!(strikes.len(), 1);
    let lines = dtool_text::layout_story(&lib, &story, None);
    let baseline = lines[0].baseline;
    assert!((strikes[0].rect[3] - metrics.strike_thickness).abs() < 0.001);
    assert!((strikes[0].rect[1] - (baseline - metrics.strike_offset - metrics.strike_thickness * 0.5)).abs() < 0.001);
    let custom = two_runs(CharStyle { strike: Decoration { on: true, weight: Some(3.0), offset: Some(5.0), colour: Some(solid(200, 0, 0)) }, ..arial() }, arial());
    let custom_rect = kind(&story_decorations(&lib, &custom, None, &solid(0, 0, 0)), DecorationKind::Strike)[0].clone();
    assert!((custom_rect.rect[3] - 3.0).abs() < 0.001);
    assert!((custom_rect.rect[1] - (baseline - 5.0 - 1.5)).abs() < 0.001);
    assert_eq!(custom_rect.paint, solid(200, 0, 0));
}

#[test]
fn c10_toggling_off_removes_the_paint() {
    let lib = FontLibrary::system();
    let styled = CharStyle { underline: on(), strike: on(), background: Some(solid(1, 2, 3)), ..arial() };
    assert_eq!(story_decorations(&lib, &two_runs(styled.clone(), styled.clone()), None, &solid(0, 0, 0)).len(), 6);
    let mut off = styled.clone();
    dtool_text::char_style::toggle_underline(&mut off.underline);
    off.strike.on = false;
    off.background = None;
    assert!(story_decorations(&lib, &two_runs(off.clone(), off), None, &solid(0, 0, 0)).is_empty());
}