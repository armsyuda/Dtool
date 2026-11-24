use dtool_text::spelling::ko_grammar;
use std::time::{Duration, Instant};

fn working_set_mb() -> Option<f64> {
    if !cfg!(windows) {
        return None;
    }
    let out = std::process::Command::new("powershell").args(["-NoProfile", "-Command", &format!("(Get-Process -Id {}).WorkingSet64", std::process::id())]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().ok().map(|b| b / 1_048_576.0)
}

#[test]
fn kiwi_loads_lazily_and_reports_cost() {
    if ko_grammar::kiwi_dir().is_none() {
        assert!(std::env::var_os("DTOOL_KIWI_REQUIRED").is_none(), "Kiwi bundle missing");
        eprintln!("KIWI_SKIP");
        return;
    }
    assert!(!ko_grammar::kiwi_loaded());
    assert!(ko_grammar::check("plain English text").is_empty());
    assert!(!ko_grammar::kiwi_loaded());
    let before = working_set_mb();
    let started = Instant::now();
    assert!(ko_grammar::engine().is_some(), "{:?}", ko_grammar::load_error());
    let load = started.elapsed();
    let after = working_set_mb();
    let sentences = [
        "아버지가방에들어가신다",
        "오늘 날씨가 정말 좋네요.",
        "디자인 툴에서 텍스트를 편집합니다.",
        "나 죰 도와죠.",
        "우리는내일만나기로했다",
        "이 문서는 세 개의 단락으로 구성되어 있습니다.",
        "사용자가 글꼴을 바꾸면 미리 보기가 갱신된다.",
        "정말 잘했따",
    ];
    let first = Instant::now();
    ko_grammar::check(sentences[0]);
    let first = first.elapsed();
    let rounds = 20;
    let started = Instant::now();
    for _ in 0..rounds {
        for s in sentences {
            ko_grammar::check(s);
        }
    }
    let per_sentence = started.elapsed() / (rounds * sentences.len() as u32);
    let paragraph = sentences.join(" ").repeat(6);
    let started = Instant::now();
    let found = ko_grammar::check(&paragraph);
    let per_paragraph = started.elapsed();
    let peak = working_set_mb();
    eprintln!(
        "KIWI_PERF version={:?} load={load:?} engine_load={:?} first_check={first:?} per_sentence={per_sentence:?} paragraph_chars={} paragraph={per_paragraph:?} paragraph_issues={} ws_before_mb={before:?} ws_after_load_mb={after:?} ws_after_checks_mb={peak:?}",
        ko_grammar::version(),
        ko_grammar::load_time(),
        paragraph.chars().count(),
        found.len()
    );
    assert!(per_sentence < Duration::from_millis(250));
}
