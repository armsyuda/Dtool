use dtool_core::paint::{CharacterPaint, Colour, Paint, ParagraphPaint, RunPaint, TextPaint, resolve_run_paint};

fn make_colour(r: u8, g: u8, b: u8) -> Colour {
    Colour::new(r, g, b, 255)
}

fn solid(r: u8, g: u8, b: u8) -> Paint {
    Paint::Solid { colour: make_colour(r, g, b) }
}

fn query(run: &RunPaint, character: &CharacterPaint, paragraph: &ParagraphPaint) -> TextPaint {
    resolve_run_paint(run, character, paragraph)
}

fn expect_solid(paint: &Paint, r: u8, g: u8, b: u8) {
    match paint {
        Paint::Solid { colour } => {
            assert_eq!(colour.r, r);
            assert_eq!(colour.g, g);
            assert_eq!(colour.b, b);
        }
        _ => panic!("Expected solid paint"),
    }
}

#[test]
fn test_paragraph_paint_inherited_by_run() {
    let mut paragraph = ParagraphPaint::new();
    let character = CharacterPaint::new();
    let run = RunPaint::new();
    paragraph.set_fill(solid(255, 0, 0));
    let resolved = query(&run, &character, &paragraph);
    expect_solid(&resolved.fill, 255, 0, 0);
}

#[test]
fn test_local_character_paint_wins() {
    let mut paragraph = ParagraphPaint::new();
    let mut character = CharacterPaint::new();
    let mut run = RunPaint::new();
    paragraph.set_fill(solid(255, 0, 0));
    character.set_fill(solid(0, 0, 255));
    expect_solid(&query(&run, &character, &paragraph).fill, 0, 0, 255);
    run.set_fill(solid(0, 255, 0));
    let resolved = query(&run, &character, &paragraph);
    expect_solid(&resolved.fill, 0, 255, 0);
}

#[test]
fn test_reset_returns_to_inherited() {
    let mut paragraph = ParagraphPaint::new();
    let character = CharacterPaint::new();
    let mut run = RunPaint::new();
    paragraph.set_fill(solid(255, 0, 0));
    run.set_fill(solid(0, 255, 0));
    expect_solid(&query(&run, &character, &paragraph).fill, 0, 255, 0);
    run.reset_fill();
    let resolved = query(&run, &character, &paragraph);
    expect_solid(&resolved.fill, 255, 0, 0);
}

#[test]
fn test_no_parallel_string_colour_left_behind() {
    let paragraph = ParagraphPaint::new();
    let character = CharacterPaint::new();
    let mut run = RunPaint::new();
    run.set_fill(solid(100, 150, 200));
    let resolved = query(&run, &character, &paragraph);
    let payload = serde_json::to_value(&resolved).unwrap();
    let object = payload.as_object().unwrap();
    assert!(!object.contains_key("colour_string"));
    assert!(!object.contains_key("hex_colour"));
    assert!(!object.contains_key("css_colour"));
    expect_solid(&resolved.fill, 100, 150, 200);
    let round_trip: TextPaint = serde_json::from_value(payload).unwrap();
    assert_eq!(round_trip, resolved);
}
