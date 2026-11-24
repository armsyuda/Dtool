//! Test home for AppShell tests: points APPDATA / LOCALAPPDATA / USERPROFILE / HOME at a clean
//! per-test dir so no test reads or writes the owner's real DTool settings (layout.json,
//! personas, presets, prefs, ui_settings, shortcuts, snap prefs, defaults, recovery, pie menus).
//! Root: $DTOOL_TEST_HOME, else <CARGO_TARGET_DIR>/test_home (never the system temp dir).
//! The guard holds a per-binary lock (env vars are process-wide), so keep it alive for the whole test.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

static LOCK: Mutex<()> = Mutex::new(());

pub struct TestHome {
    pub root: PathBuf,
    _lock: MutexGuard<'static, ()>,
}

pub fn base() -> PathBuf {
    match std::env::var_os("DTOOL_TEST_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => Path::new(env!("CARGO_TARGET_TMPDIR")).parent().unwrap_or(Path::new(env!("CARGO_TARGET_TMPDIR"))).join("test_home"),
    }
}

pub fn enter(name: &str) -> TestHome {
    let lock = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let root = base().join(format!("{}_{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let roaming = root.join("AppData").join("Roaming");
    let local = root.join("AppData").join("Local");
    std::fs::create_dir_all(&roaming).expect("create test APPDATA");
    std::fs::create_dir_all(&local).expect("create test LOCALAPPDATA");
    unsafe {
        std::env::set_var("APPDATA", &roaming);
        std::env::set_var("LOCALAPPDATA", &local);
        std::env::set_var("USERPROFILE", &root);
        std::env::set_var("HOME", &root);
    }
    assert!(dtool_ui::settings::UiSettings::default_path().starts_with(&root), "ui settings must resolve under the test home");
    assert!(dtool_core::prefs::Prefs::default_path().starts_with(&root), "prefs must resolve under the test home");
    TestHome { root, _lock: lock }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
