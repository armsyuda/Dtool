use dtool_ui::context_bar::choice_label;

#[test]
fn shape_builder_choice_values_use_ko_labels() {
    assert_eq!(choice_label("context.shape_builder.drag_method", "marquee"), dtool_strings::text("context.shape_builder.drag_method.marquee"));
    assert_eq!(choice_label("context.unknown_option", "raw"), "raw");
}
