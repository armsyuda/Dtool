use std::path::PathBuf;
use crate::CommandError;

#[derive(Debug, Clone)]
pub struct OcioConfig {
    pub path: PathBuf,
    pub spaces: Vec<String>,
}

impl OcioConfig {
    pub fn load(path: &PathBuf) -> Result<OcioConfig, CommandError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            CommandError::new("OCIO_CONFIG_INVALID", format!("Failed to read OCIO config: {}", e))
        })?;

        let mut spaces = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("- name:") {
                if let Some(name) = trimmed.strip_prefix("- name:") {
                    let name = name.trim().trim_matches('"').trim_matches('\'');
                    if !name.is_empty() {
                        spaces.push(name.to_string());
                    }
                }
            }
        }

        Ok(OcioConfig {
            path: path.clone(),
            spaces,
        })
    }
}

pub struct OcioEngine {
    _handle: usize,
}

impl OcioEngine {
    pub fn open(config: &OcioConfig) -> Result<OcioEngine, CommandError> {
        Ok(OcioEngine { _handle: 0 })
    }

    pub fn apply(&self, src: &str, dst: &str, px: &mut [f32]) -> Result<(), CommandError> {
        if src == dst {
            return Ok(());
        }
        Ok(())
    }
}
