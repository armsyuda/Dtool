use dtool_export::{atomic_temp_path, write_atomic};

#[test]
fn write_atomic_replaces_target_and_leaves_no_temp() {
    let dir = std::env::temp_dir().join(format!("c13_atomic_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("out.png");
    std::fs::write(&target, b"old").unwrap();
    std::fs::write(dir.join("out.tmp"), b"user").unwrap();
    write_atomic(&target, b"new bytes").unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"new bytes");
    assert_eq!(std::fs::read(dir.join("out.tmp")).unwrap(), b"user");
    assert!(!atomic_temp_path(&target).exists());
    let name = atomic_temp_path(&target).file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with(".out.png.") && name.ends_with(".dtool-tmp"));
}

#[test]
fn write_atomic_failure_keeps_target_untouched() {
    let dir = std::env::temp_dir().join(format!("c13_atomic_fail_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let missing = dir.join("no_such_dir").join("out.png");
    assert!(write_atomic(&missing, b"x").is_err());
    assert!(!missing.exists());
    assert!(!atomic_temp_path(&missing).exists());
}