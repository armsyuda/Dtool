use dtool_core::document::{BreakKind as CoreBreak, Special as CoreSpecial, Story as CoreStory, StoryId as CoreStoryId};
use dtool_text::layout::{get_specials, insert_special, layout_story};
use dtool_text::special::{break_menu_items, is_break_special, special_from_name, special_json, BreakKind, Special};
use dtool_text::{story_from_core, FontLibrary, Story, StoryId, TextRange};
use dtool_text::frame::{layout_chain, FrameProps};

fn story(text: &str) -> Story {
    Story { id: StoryId(17), text: text.into(), ..Story::default() }
}

fn even() -> Special {
    Special::Break(BreakKind::EvenPage)
}

#[test]
fn even_page_break_storage_inside_sentence() {
    let mut value = story("Test content for even page break test.");
    let original = value.clone();
    insert_special(&mut value, 10, even());
    assert_eq!(value.id, original.id);
    assert_eq!(value.text, original.text);
    assert_eq!(value.chars, original.chars);
    assert_eq!(value.paras, original.paras);
    assert_eq!(get_specials(&value), &[(10, even())]);
    insert_special(&mut value, 10, even());
    assert_eq!(value.specials.len(), 1);
    value.insert(0, "XYZ");
    assert_eq!(get_specials(&value), &[(10 + 3, even())]);
    assert_eq!(value.text, format!("XYZ{}", original.text));
}
#[test]
fn even_page_break_storage_odd_page_text() {
    let mut value = story("Content starting on odd page 1.");
    let original = value.clone();
    insert_special(&mut value, 5, even());
    assert_eq!(value.id, original.id);
    assert_eq!(value.text, original.text);
    assert_eq!(value.chars, original.chars);
    assert_eq!(value.paras, original.paras);
    assert_eq!(get_specials(&value), &[(5, even())]);
    insert_special(&mut value, 5, even());
    assert_eq!(value.specials.len(), 1);
    value.insert(0, "XYZ");
    assert_eq!(get_specials(&value), &[(5 + 3, even())]);
    assert_eq!(value.text, format!("XYZ{}", original.text));
}
#[test]
fn even_page_break_storage_even_page_text() {
    let mut value = story("Content starting on even page 2.");
    let original = value.clone();
    insert_special(&mut value, 5, even());
    assert_eq!(value.id, original.id);
    assert_eq!(value.text, original.text);
    assert_eq!(value.chars, original.chars);
    assert_eq!(value.paras, original.paras);
    assert_eq!(get_specials(&value), &[(5, even())]);
    insert_special(&mut value, 5, even());
    assert_eq!(value.specials.len(), 1);
    value.insert(0, "XYZ");
    assert_eq!(get_specials(&value), &[(5 + 3, even())]);
    assert_eq!(value.text, format!("XYZ{}", original.text));
}
#[test]
fn even_page_break_storage_preserves_content() {
    let mut value = story("Hello World");
    let original = value.clone();
    insert_special(&mut value, 5, even());
    assert_eq!(value.id, original.id);
    assert_eq!(value.text, original.text);
    assert_eq!(value.chars, original.chars);
    assert_eq!(value.paras, original.paras);
    assert_eq!(get_specials(&value), &[(5, even())]);
    insert_special(&mut value, 5, even());
    assert_eq!(value.specials.len(), 1);
    value.insert(0, "XYZ");
    assert_eq!(get_specials(&value), &[(5 + 3, even())]);
    assert_eq!(value.text, format!("XYZ{}", original.text));
}
#[test]
fn even_page_break_storage_at_start() {
    let mut value = story("Start content");
    let original = value.clone();
    insert_special(&mut value, 0, even());
    assert_eq!(value.id, original.id);
    assert_eq!(value.text, original.text);
    assert_eq!(value.chars, original.chars);
    assert_eq!(value.paras, original.paras);
    assert_eq!(get_specials(&value), &[(0, even())]);
    insert_special(&mut value, 0, even());
    assert_eq!(value.specials.len(), 1);
    value.insert(0, "XYZ");
    assert_eq!(get_specials(&value), &[(0 + 3, even())]);
    assert_eq!(value.text, format!("XYZ{}", original.text));
}
#[test]
fn even_page_break_storage_at_end() {
    let mut value = story("End content");
    let original = value.clone();
    insert_special(&mut value, 11, even());
    assert_eq!(value.id, original.id);
    assert_eq!(value.text, original.text);
    assert_eq!(value.chars, original.chars);
    assert_eq!(value.paras, original.paras);
    assert_eq!(get_specials(&value), &[(11, even())]);
    insert_special(&mut value, 11, even());
    assert_eq!(value.specials.len(), 1);
    value.insert(0, "XYZ");
    assert_eq!(get_specials(&value), &[(11 + 3, even())]);
    assert_eq!(value.text, format!("XYZ{}", original.text));
}
#[test]
fn even_page_break_storage_empty_text_after() {
    let mut value = story("Before");
    let original = value.clone();
    insert_special(&mut value, 6, even());
    assert_eq!(value.id, original.id);
    assert_eq!(value.text, original.text);
    assert_eq!(value.chars, original.chars);
    assert_eq!(value.paras, original.paras);
    assert_eq!(get_specials(&value), &[(6, even())]);
    insert_special(&mut value, 6, even());
    assert_eq!(value.specials.len(), 1);
    value.insert(0, "XYZ");
    assert_eq!(get_specials(&value), &[(6 + 3, even())]);
    assert_eq!(value.text, format!("XYZ{}", original.text));
}
#[test]
fn even_page_break_storage_empty_text_before() {
    let mut value = story("After");
    let original = value.clone();
    insert_special(&mut value, 0, even());
    assert_eq!(value.id, original.id);
    assert_eq!(value.text, original.text);
    assert_eq!(value.chars, original.chars);
    assert_eq!(value.paras, original.paras);
    assert_eq!(get_specials(&value), &[(0, even())]);
    insert_special(&mut value, 0, even());
    assert_eq!(value.specials.len(), 1);
    value.insert(0, "XYZ");
    assert_eq!(get_specials(&value), &[(0 + 3, even())]);
    assert_eq!(value.text, format!("XYZ{}", original.text));
}
#[test]
fn even_page_break_storage_short_text() {
    let mut value = story("Hi");
    let original = value.clone();
    insert_special(&mut value, 1, even());
    assert_eq!(value.id, original.id);
    assert_eq!(value.text, original.text);
    assert_eq!(value.chars, original.chars);
    assert_eq!(value.paras, original.paras);
    assert_eq!(get_specials(&value), &[(1, even())]);
    insert_special(&mut value, 1, even());
    assert_eq!(value.specials.len(), 1);
    value.insert(0, "XYZ");
    assert_eq!(get_specials(&value), &[(1 + 3, even())]);
    assert_eq!(value.text, format!("XYZ{}", original.text));
}
#[test]
fn even_page_break_storage_stored_in_story() {
    let mut value = story("Test");
    let original = value.clone();
    insert_special(&mut value, 2, even());
    assert_eq!(value.id, original.id);
    assert_eq!(value.text, original.text);
    assert_eq!(value.chars, original.chars);
    assert_eq!(value.paras, original.paras);
    assert_eq!(get_specials(&value), &[(2, even())]);
    insert_special(&mut value, 2, even());
    assert_eq!(value.specials.len(), 1);
    value.insert(0, "XYZ");
    assert_eq!(get_specials(&value), &[(2 + 3, even())]);
    assert_eq!(value.text, format!("XYZ{}", original.text));
}
#[test]
fn even_page_break_storage_command_payload_text() {
    let mut value = story("Command test content.");
    let original = value.clone();
    insert_special(&mut value, 5, even());
    assert_eq!(value.id, original.id);
    assert_eq!(value.text, original.text);
    assert_eq!(value.chars, original.chars);
    assert_eq!(value.paras, original.paras);
    assert_eq!(get_specials(&value), &[(5, even())]);
    insert_special(&mut value, 5, even());
    assert_eq!(value.specials.len(), 1);
    value.insert(0, "XYZ");
    assert_eq!(get_specials(&value), &[(5 + 3, even())]);
    assert_eq!(value.text, format!("XYZ{}", original.text));
}
#[test]
fn even_page_break_storage_mcp_payload_text() {
    let mut value = story("MCP API test content.");
    let original = value.clone();
    insert_special(&mut value, 8, even());
    assert_eq!(value.id, original.id);
    assert_eq!(value.text, original.text);
    assert_eq!(value.chars, original.chars);
    assert_eq!(value.paras, original.paras);
    assert_eq!(get_specials(&value), &[(8, even())]);
    insert_special(&mut value, 8, even());
    assert_eq!(value.specials.len(), 1);
    value.insert(0, "XYZ");
    assert_eq!(get_specials(&value), &[(8 + 3, even())]);
    assert_eq!(value.text, format!("XYZ{}", original.text));
}
#[test]
fn even_page_break_storage_gui_payload_text() {
    let mut value = story("GUI test content.");
    let original = value.clone();
    insert_special(&mut value, 4, even());
    assert_eq!(value.id, original.id);
    assert_eq!(value.text, original.text);
    assert_eq!(value.chars, original.chars);
    assert_eq!(value.paras, original.paras);
    assert_eq!(get_specials(&value), &[(4, even())]);
    insert_special(&mut value, 4, even());
    assert_eq!(value.specials.len(), 1);
    value.insert(0, "XYZ");
    assert_eq!(get_specials(&value), &[(4 + 3, even())]);
    assert_eq!(value.text, format!("XYZ{}", original.text));
}
#[test]
fn even_page_break_storage_acceptance_payload_text() {
    let mut value = story("Full acceptance test content for even page break feature.");
    let original = value.clone();
    insert_special(&mut value, 12, even());
    assert_eq!(value.id, original.id);
    assert_eq!(value.text, original.text);
    assert_eq!(value.chars, original.chars);
    assert_eq!(value.paras, original.paras);
    assert_eq!(get_specials(&value), &[(12, even())]);
    insert_special(&mut value, 12, even());
    assert_eq!(value.specials.len(), 1);
    value.insert(0, "XYZ");
    assert_eq!(get_specials(&value), &[(12 + 3, even())]);
    assert_eq!(value.text, format!("XYZ{}", original.text));
}
#[test]
fn multiple_even_page_breaks_are_sorted_without_changing_text() {
    let mut value = story("A B C D E F G H I J K L M N O P Q R S T U V W X Y Z");
    let original = value.text.clone();
    insert_special(&mut value, 10, even());
    insert_special(&mut value, 5, even());
    assert_eq!(get_specials(&value), &[(5, even()), (10, even())]);
    assert_eq!(value.text, original);
    value.insert(7, "XX");
    assert_eq!(get_specials(&value), &[(5, even()), (12, even())]);
    assert_eq!(value.id, StoryId(17));
}

#[test]
fn even_page_break_interaction_with_odd_page_break_storage() {
    let mut value = story("Mixed breaks test content here.");
    let original = value.text.clone();
    let odd = Special::Break(BreakKind::OddPage);
    insert_special(&mut value, 15, even());
    insert_special(&mut value, 5, odd.clone());
    assert_eq!(get_specials(&value), &[(5, odd.clone()), (15, even())]);
    assert_eq!(value.text, original);
    value.insert(6, "X");
    assert_eq!(get_specials(&value), &[(5, odd), (16, even())]);
}

#[test]
fn even_page_break_interaction_with_column_break_storage() {
    let mut value = story("Column then even page break test.");
    let original = value.text.clone();
    let column = Special::Break(BreakKind::Column);
    insert_special(&mut value, 5, column.clone());
    insert_special(&mut value, 15, even());
    assert_eq!(get_specials(&value), &[(5, column.clone()), (15, even())]);
    assert_eq!(value.text, original);
    value.insert(0, "ABC");
    assert_eq!(get_specials(&value), &[(8, column), (18, even())]);
}

#[test]
fn even_page_break_interaction_with_frame_break_storage() {
    let mut value = story("Frame then even page break test.");
    let original = value.text.clone();
    let frame = Special::Break(BreakKind::Frame);
    insert_special(&mut value, 5, frame.clone());
    insert_special(&mut value, 15, even());
    assert_eq!(get_specials(&value), &[(5, frame.clone()), (15, even())]);
    assert_eq!(value.text, original);
    value.insert(0, "ABC");
    assert_eq!(get_specials(&value), &[(8, frame), (18, even())]);
}

#[test]
fn consecutive_even_page_breaks_keep_distinct_positions() {
    let mut value = story("Consecutive breaks test.");
    let original = value.text.clone();
    insert_special(&mut value, 5, even());
    insert_special(&mut value, 6, even());
    assert_eq!(get_specials(&value), &[(5, even()), (6, even())]);
    assert_eq!(value.text, original);
    value.insert(6, "XX");
    assert_eq!(get_specials(&value), &[(5, even()), (8, even())]);
    assert_eq!(value.text.chars().count(), original.chars().count() + 2);
}

#[test]
fn even_page_break_with_long_text_preserves_all_scalars() {
    let original = "A".repeat(1000);
    let mut value = story(&original);
    insert_special(&mut value, 500, even());
    assert_eq!(value.text, original);
    assert_eq!(get_specials(&value), &[(500, even())]);
    value.insert(250, "BC");
    assert_eq!(get_specials(&value), &[(502, even())]);
    assert_eq!(value.text.chars().count(), 1002);
    assert_eq!(&value.text[250..252], "BC");
    assert_eq!(value.id, StoryId(17));
}

#[test]
fn core_bridge_preserves_even_page_break_and_story_identity() {
    let core = CoreStory {
        id: CoreStoryId(19),
        text: "Bridge break payload".into(),
        specials: vec![(6, CoreSpecial::Break(CoreBreak::EvenPage))],
        ..CoreStory::default()
    };
    let before = format!("{core:?}");
    let value = story_from_core(&core);
    assert_eq!(value.id, StoryId(19));
    assert_eq!(value.text, core.text);
    assert_eq!(get_specials(&value), &[(6, even())]);
    assert_eq!(format!("{core:?}"), before);
}

#[test]
fn even_page_break_name_and_menu_resolve_to_the_same_special() {
    assert!(is_break_special("even_page_break"));
    assert_eq!(special_from_name("even_page_break"), Some(even()));
    assert!(!is_break_special("unknown_break"));
    assert_eq!(special_from_name("unknown_break"), None);
    let items = break_menu_items();
    assert_eq!(items.iter().filter(|(_, value)| *value == "even_page_break").count(), 1);
    assert_eq!(special_json("even_page_break"), None);
    assert_eq!(special_json("em_dash").as_deref(), Some("\u{2014}"));
}

#[test]
fn even_page_break_layout_emits_a_break_after_without_mutating_story() {
    let lib = FontLibrary::system();
    assert!(lib.resolve("Arial", "Regular").is_some());
    let mut value = story("Alpha Beta");
    insert_special(&mut value, 6, even());
    let original = value.clone();
    let lines = layout_story(&lib, &value, Some(500.0));
    assert!(lines.iter().any(|line| line.break_after == Some(BreakKind::EvenPage)));
    assert_eq!(value, original);
    assert!(lines.iter().all(|line| line.range.start <= line.range.end));
}

#[test]
fn even_page_break_no_frames_reports_full_overset_without_fabricating_pages() {
    let lib = FontLibrary::system();
    let mut value = story("No frames available");
    insert_special(&mut value, 5, even());
    let original = value.clone();
    let layout = layout_chain(&lib, &value, &[]);
    assert!(layout.per_frame.is_empty());
    assert_eq!(layout.overset, Some(TextRange::new(0, value.text.chars().count())));
    assert_eq!(value, original);
}

#[test]
fn even_page_break_layout_keeps_requested_frame_count() {
    let lib = FontLibrary::system();
    let mut value = story("Before after");
    insert_special(&mut value, 7, even());
    let original = value.clone();
    let frames = vec![FrameProps::from_rect([500.0, 500.0]); 3];
    let layout = layout_chain(&lib, &value, &frames);
    assert_eq!(layout.per_frame.len(), frames.len());
    assert_eq!(value, original);
    assert!(layout.per_frame.iter().flatten().all(|line| line.range.end <= value.text.chars().count()));
}