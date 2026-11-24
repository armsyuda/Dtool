use dtool_text::spelling::{ko_dictionary, ko_loaded, Speller};
use std::time::Instant;

fn working_set_mb() -> Option<f64> {
    if !cfg!(windows) {
        return None;
    }
    let out = std::process::Command::new("powershell").args(["-NoProfile", "-Command", &format!("(Get-Process -Id {}).WorkingSet64", std::process::id())]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().ok().map(|b| b / 1_048_576.0)
}

#[test]
fn korean_dictionary_loads_lazily_and_reports_cost() {
    let t = Instant::now();
    let speller = Speller::en_us().expect("en_US");
    let en_init = t.elapsed();
    assert!(speller.check_text("the parcel was recieved").len() == 1);
    assert!(!ko_loaded(), "English text must not load the Korean dictionary");
    let before = working_set_mb();
    let t = Instant::now();
    speller.is_correct("학교");
    let ko_first = t.elapsed();
    let after = working_set_mb();
    assert!(ko_loaded());
    assert!(ko_dictionary().is_some());
    let t = Instant::now();
    for _ in 0..100 {
        speller.is_correct("학교에서");
    }
    let ko_check = t.elapsed() / 100;
    let t = Instant::now();
    let suggestions = speller.suggest("학교엣서");
    let ko_suggest = t.elapsed();
    eprintln!("KO_PERF en_init={en_init:?} ko_first_check_incl_parse={ko_first:?} ko_check={ko_check:?} ko_suggest={ko_suggest:?} ws_before_mb={before:?} ws_after_mb={after:?} suggestions={suggestions:?}");
}