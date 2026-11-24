use dtool_io::af::{inspect_af, AfInspectReport};
use std::fs;
use std::path::PathBuf;

fn get_samples() -> Vec<PathBuf> {
    let samples_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("samples");
    if !samples_dir.exists() {
        return vec![];
    }
    let mut files: Vec<PathBuf> = fs::read_dir(&samples_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("af"))
        .collect();
    files.sort();
    files
}

#[test]
fn b61_af_import_only_feasibility_matrix() {
    let samples = get_samples();
    for path in &samples {
        let original = fs::read(path).unwrap();
        let report: AfInspectReport = inspect_af(path).expect("inspect_af failed");
        let current = fs::read(path).unwrap();
        assert!(original == current, "File {} was modified by inspection", path.display());
        println!("--- Sample: {} ---", path.display());
        println!("Bytes: {}", original.len());
        println!("Version: {}", report.version);
        println!("Verdict go: {}", report.verdict_go);
        println!("Unresolved count: {}", report.unresolved_items.len());
        println!("Millis: {}", report.millis);
        println!("Peak bytes: {}", report.peak_bytes);
        if !report.verdict_go {
            for unresolved in &report.unresolved_items {
                println!("  Unresolved: {}", unresolved);
            }
        }
    }
}