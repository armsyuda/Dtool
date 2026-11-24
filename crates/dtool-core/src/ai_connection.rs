use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use crate::editor_lock::EditorLock;

pub struct AiConnection {
    enabled: AtomicBool,
    editor: Arc<EditorLock>,
}

impl Default for AiConnection {
    fn default() -> Self { Self { enabled: AtomicBool::new(true), editor: Arc::new(EditorLock::new()) } }
}

impl std::fmt::Debug for AiConnection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("AiConnection").field("enabled", &self.enabled()).finish()
    }
}

impl AiConnection {
    pub fn enabled(&self) -> bool { self.enabled.load(Ordering::Acquire) }
    pub fn set_from_gui(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
        if !enabled { self.editor.release(); }
    }
    pub fn editor_lock(&self) -> Arc<EditorLock> { Arc::clone(&self.editor) }
}
