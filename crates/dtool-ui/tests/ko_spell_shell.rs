#[path = "support/test_home.rs"]
mod test_home;
use dtool_tools::dtool_text::spelling;
use dtool_ui::about::LICENCES;
use dtool_ui::shell::AppShell;

fn frame(shell: &mut AppShell, ctx: &egui::Context) -> Vec<String> {
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| shell.show(ui));
    out.textures_delta.clear();
    out.shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn shell_warms_the_korean_dictionary_at_idle_and_about_lists_its_licence() {
    let _home = test_home::enter("ko_spell_shell");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    assert!(!shell.spell_warm_requested);
    frame(&mut shell, &ctx);
    frame(&mut shell, &ctx);
    assert!(shell.spell_warm_requested);
    let started = std::time::Instant::now();
    while !spelling::ko_loaded() && started.elapsed() < std::time::Duration::from_secs(60) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(spelling::ko_loaded());
    assert!(spelling::checkable("한국어"));

    assert!(LICENCES.iter().any(|item| item.contains("hunspell-dict-ko") && item.contains("GPL-3.0")));
    shell.open_about();
    frame(&mut shell, &ctx);
    let texts = frame(&mut shell, &ctx);
    assert!(shell.about.open);
    assert!(texts.iter().any(|text| text.contains("hunspell-dict-ko 0.7.94 (GPL-3.0)")), "{texts:?}");
}