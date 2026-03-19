/// Hash-based polling fallback watcher.
///
/// Every `interval`, walks all source files and emits `WatchEvent::CheckAll`.
/// Used as a last resort when git and native watching are both unavailable.

use std::path::Path;
use std::sync::mpsc::Sender;
use std::time::Duration;

use super::WatchEvent;

/// Poll `root` every `interval`, emitting `CheckAll` on each tick.
/// This runs forever — spawn it in a background thread.
pub fn watch(root: &Path, interval: Duration, tx: Sender<WatchEvent>) {
    let _ = root; // Not needed — daemon does the actual file scanning
    loop {
        std::thread::sleep(interval);
        if tx.send(WatchEvent::CheckAll).is_err() {
            break; // Channel closed — daemon shut down
        }
    }
}
