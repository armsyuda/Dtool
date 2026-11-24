use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub struct EditorLock {
    locked: AtomicBool,
}

impl EditorLock {
    pub fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
        }
    }

    pub fn try_acquire(&self) -> bool {
        self.locked.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok()
    }

    pub fn acquire_with_timeout(&self, timeout: Duration) -> bool {
        let start = std::time::Instant::now();
        while !self.try_acquire() {
            if start.elapsed() >= timeout {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        true
    }

    pub fn release(&self) {
        self.locked.store(false, Ordering::Release);
    }

    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::Acquire)
    }
}

impl Default for EditorLock {
    fn default() -> Self {
        Self::new()
    }
}
