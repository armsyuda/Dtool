use dtool_core::{Story, StoryId};
use dtool_text::{layout_story, story_from_core, FontLibrary, Line};

fn lines(text: &str, width: Option<f32>) -> Vec<Line> {
    let core = Story { id: StoryId(1), text: text.into(), ..Story::default() };
    layout_story(&FontLibrary::shared(), &story_from_core(&core), width)
}

fn steps(lines: &[Line]) -> Vec<f32> {
    lines.windows(2).map(|pair| pair[1].baseline - pair[0].baseline).collect()
}

#[test]
fn wrapped_and_unwrapped_paragraph_gap_is_line_height() {
    let text = "ab\ncd\n\nef";
    let unwrapped = lines(text, None);
    let wrapped = lines(text, Some(300.0));
    assert_eq!(unwrapped.len(), 4, "{unwrapped:?}");
    assert_eq!(wrapped.len(), 4, "{wrapped:?}");
    for (a, b) in unwrapped.iter().zip(&wrapped) {
        assert_eq!(a.range, b.range);
        assert!((a.baseline - b.baseline).abs() <= 0.01, "{} {}", a.baseline, b.baseline);
    }
    for step in steps(&unwrapped).into_iter().chain(steps(&wrapped)) {
        assert!((step - 12.0).abs() <= 0.01, "{:?} {:?}", steps(&unwrapped), steps(&wrapped));
    }
}

#[test]
fn trailing_empty_paragraph_steps_one_line_height_when_wrapped() {
    let wrapped = lines("ab\n", Some(300.0));
    let unwrapped = lines("ab\n", None);
    assert_eq!(wrapped.len(), 2, "{wrapped:?}");
    assert_eq!(unwrapped.len(), 2, "{unwrapped:?}");
    assert!((wrapped[1].baseline - wrapped[0].baseline - 12.0).abs() <= 0.01, "{wrapped:?}");
    assert!((wrapped[1].baseline - unwrapped[1].baseline).abs() <= 0.01, "{wrapped:?} {unwrapped:?}");
}
