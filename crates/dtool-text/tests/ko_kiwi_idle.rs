use dtool_text::spelling::ko_grammar::{self, IssueKind};
use std::time::{Duration, Instant};

fn memory_mb() -> Option<(u64, u64)> {
    if !cfg!(windows) {
        return None;
    }
    let script = format!("$p = Get-Process -Id {}; \"$($p.WorkingSet64) $($p.PrivateMemorySize64)\"", std::process::id());
    let out = std::process::Command::new("powershell").args(["-NoProfile", "-Command", &script]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let mut parts = text.split_whitespace().map(|v| v.parse::<u64>().ok().map(|b| b / 1_048_576));
    Some((parts.next()??, parts.next()??))
}

fn wait_until(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    done()
}

#[test]
fn kiwi_unloads_after_idle_and_reloads_lazily_on_the_next_korean_check() {
    if ko_grammar::kiwi_dir().is_none() {
        assert!(std::env::var_os("DTOOL_KIWI_REQUIRED").is_none(), "Kiwi bundle missing");
        eprintln!("KIWI_SKIP");
        return;
    }
    assert_eq!(ko_grammar::idle_unload(), Some(ko_grammar::DEFAULT_IDLE_UNLOAD));
    assert_eq!(ko_grammar::DEFAULT_IDLE_UNLOAD, Duration::from_secs(300));
    ko_grammar::set_idle_unload(Some(Duration::from_secs(60)));
    assert!(dtool_text::spelling::ko_dictionary().is_some());
    let before = memory_mb();
    let first = ko_grammar::check("아버지가방에들어가신다");
    assert_eq!(first.first().map(|i| i.kind), Some(IssueKind::Spacing), "{first:?}");
    let loaded = memory_mb();
    assert!(ko_grammar::kiwi_ready());
    assert_eq!(ko_grammar::load_count(), 1);
    ko_grammar::set_idle_unload(Some(Duration::from_millis(300)));
    assert!(wait_until(Duration::from_secs(20), || !ko_grammar::kiwi_ready()), "Kiwi stayed loaded past the idle timeout");
    assert_eq!(ko_grammar::unload_count(), 1);
    assert!(!ko_grammar::kiwi_loaded());
    assert!(wait_until(Duration::from_secs(10), || !ko_grammar::kiwi_resident()), "Kiwi worker still resident");
    std::thread::sleep(Duration::from_millis(500));
    let unloaded = memory_mb();
    std::thread::sleep(Duration::from_secs(2));
    let settled = memory_mb();
    if let (Some((_, loaded_private)), Some((_, unloaded_private))) = (loaded, unloaded) {
        assert!(unloaded_private + 200 < loaded_private, "private {loaded_private} MB -> {unloaded_private} MB");
    }
    ko_grammar::set_idle_unload(Some(Duration::from_secs(60)));
    assert!(ko_grammar::check_if_ready("우리는내일만나기로했다").is_none());
    assert!(wait_until(Duration::from_secs(60), ko_grammar::kiwi_ready), "{:?}", ko_grammar::load_error());
    let again = ko_grammar::check_if_ready("우리는내일만나기로했다").expect("ready after reload");
    assert_eq!(again.len(), 1, "{again:?}");
    assert_eq!(again[0].suggestion, "우리는 내일 만나기로 했다");
    assert_eq!(ko_grammar::load_count(), 2);
    let reloaded = memory_mb();
    ko_grammar::set_idle_unload_secs(0);
    assert_eq!(ko_grammar::idle_unload(), None);
    assert!(!ko_grammar::unload_if_idle(Instant::now() + Duration::from_secs(3600)));
    assert!(ko_grammar::kiwi_ready());
    ko_grammar::set_idle_unload_secs(300);
    assert_eq!(ko_grammar::idle_unload(), Some(Duration::from_secs(300)));
    assert!(ko_grammar::unload_if_idle(Instant::now() + Duration::from_secs(301)));
    assert!(!ko_grammar::kiwi_ready());
    eprintln!("KIWI_IDLE ws_private_mb before={before:?} loaded={loaded:?} after_unload={unloaded:?} settled={settled:?} reloaded={reloaded:?} load_time={:?} loads={} unloads={}", ko_grammar::load_time(), ko_grammar::load_count(), ko_grammar::unload_count());
}
