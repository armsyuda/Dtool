use dtool_tools::art_text::ArtTextTool;

#[test]
fn c10_art_text_select_word_and_paragraph_send_real_ranges() {
    let tool = ArtTextTool::new();
    let word = tool.select_word(3, "one two", 5);
    assert_eq!(word.name, "text.select");
    assert_eq!(word.params["story"], 3);
    assert_eq!(word.params["range"]["start"], 4);
    assert_eq!(word.params["range"]["end"], 7);
    let paragraph = tool.select_paragraph(3, "ab\ncd\nef", 4);
    assert_eq!(paragraph.params["range"]["start"], 3);
    assert_eq!(paragraph.params["range"]["end"], 6);
}
