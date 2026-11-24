use std::fs;
use std::path::{Path, PathBuf};

const RECENT_FILE_NAME: &str = "recent.json";
const MAX_RECENT: usize = 10;

#[derive(Debug, Clone)]
pub struct RecentList {
    paths: Vec<PathBuf>,
    cap: usize,
}

impl RecentList {
    pub fn new() -> Self {
        Self::with_cap(MAX_RECENT)
    }

    pub fn with_cap(cap: usize) -> Self {
        Self { paths: Vec::new(), cap: cap.max(1) }
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    pub fn set_cap(&mut self, cap: usize) {
        self.cap = cap.max(1);
        self.paths.truncate(self.cap);
    }

    pub fn load() -> Self {
        let cap = dtool_core::prefs::Prefs::load(&dtool_core::prefs::Prefs::default_path()).recent_cap();
        let mut list = Self::load_uncapped();
        list.set_cap(cap);
        list
    }

    fn load_uncapped() -> Self {
        let path = recent_file_path();
        match fs::read_to_string(&path) {
            Ok(content) => {
                match serde_json::from_str::<Vec<String>>(&content) {
                    Ok(list) => {
                        let paths: Vec<PathBuf> = list.into_iter().map(PathBuf::from).collect();
                        Self { paths, cap: MAX_RECENT }
                    }
                    Err(_) => Self::new(),
                }
            }
            Err(_) => Self::new(),
        }
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        let path = recent_file_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json: Vec<String> = self.paths.iter().map(|p| p.to_string_lossy().to_string()).collect();
        fs::write(&path, serde_json::to_string_pretty(&json)?)?;
        Ok(())
    }

    pub fn push(&mut self, path: &Path) {
        let canonical = path.to_path_buf();
        self.paths.retain(|p| p != &canonical);
        self.paths.insert(0, canonical);
        self.paths.truncate(self.cap);
    }

    pub fn remove(&mut self, path: &Path) {
        let canonical = path.to_path_buf();
        self.paths.retain(|p| p != &canonical);
    }

    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

fn recent_file_path() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let mut path = PathBuf::from(appdata);
        path.push("DTool");
        path.push(RECENT_FILE_NAME);
        return path;
    }
    PathBuf::from(RECENT_FILE_NAME)
}
