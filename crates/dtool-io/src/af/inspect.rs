use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use crate::af::{AfNode, read_container, read_entry, read_tree};
use crate::{IoError, IoErrorKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureStatus {
    Supported,
    Unsupported,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct AfInspectReport {
    pub version: String,
    pub entries: Vec<String>,
    pub node_counts: HashMap<String, u32>,
    pub feature_status: HashMap<String, FeatureStatus>,
    pub unresolved_items: Vec<String>,
    pub millis: u128,
    pub peak_bytes: usize,
    pub verdict_go: bool,
}

fn classify_tag(tag: &str) -> FeatureStatus {
    match tag {
        "Document" | "Page" | "Artboard" | "Layer" | "Group" | "Shape" | "Text" | "Image" => {
            FeatureStatus::Supported
        }
        "Filter" | "BlendMode" | "Gradient" | "Pattern" | "Mask" | "ICCProfile" => {
            FeatureStatus::Unsupported
        }
        _ => FeatureStatus::Unknown,
    }
}

fn collect_nodes(node: &AfNode, counts: &mut HashMap<String, u32>, unresolved: &mut Vec<String>) {
    let tag = node.tag.clone();
    *counts.entry(tag.clone()).or_insert(0) += 1;

    if classify_tag(&tag) == FeatureStatus::Unknown {
        unresolved.push(format!("unknown_node:{}", tag));
    }

    for child in &node.children {
        collect_nodes(child, counts, unresolved);
    }
}

fn is_tree_entry(name: &str) -> bool {
    name.ends_with(".afdoc")
        || name == "document.af"
        || name == "doc.dat"
        || name.ends_with(".dat")
}

pub fn inspect_af(path: &Path) -> Result<AfInspectReport, IoError> {
    let start = Instant::now();

    let container = match read_container(path) {
        Ok(container) => container,
        Err(err) => return Err(err),
    };

    let mut peak_bytes = 0usize;
    let mut node_counts = HashMap::new();
    let mut unresolved_items = Vec::new();
    let mut feature_status = HashMap::new();

    for entry in &container.entries {
        peak_bytes = peak_bytes.saturating_add(entry.stored_size as usize);
        if !is_tree_entry(&entry.name) {
            continue;
        }
        let data = match read_entry(path, entry) {
            Ok(data) => data,
            Err(_) => {
                unresolved_items.push(format!("unreadable_entry:{}", entry.name));
                continue;
            }
        };
        peak_bytes = peak_bytes.max(data.len());
        match read_tree(&data) {
            Ok(tree) => collect_nodes(&tree, &mut node_counts, &mut unresolved_items),
            Err(_) => unresolved_items.push(format!("unparsed_tree:{}", entry.name)),
        }
    }

    for (tag, _) in &node_counts {
        feature_status.insert(tag.clone(), classify_tag(tag));
    }

    let version = container.version.to_string();
    let entries: Vec<String> = container.entries.iter().map(|e| e.name.clone()).collect();

    let millis = start.elapsed().as_millis();

    let all_supported = feature_status.values().all(|s| *s == FeatureStatus::Supported);
    let has_unsupported = feature_status.values().any(|s| *s == FeatureStatus::Unsupported);
    let verdict_go = all_supported && !has_unsupported && feature_status.is_empty() == false;

    let _kind_hint = IoErrorKind::Other;
    let _ = _kind_hint;

    Ok(AfInspectReport {
        version,
        entries,
        node_counts,
        feature_status,
        unresolved_items,
        millis,
        peak_bytes,
        verdict_go,
    })
}
