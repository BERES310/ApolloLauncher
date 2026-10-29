use crate::model::Source;
use eframe::egui::Context;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::Duration;

/// How long the file system must stay quiet before a rescan is requested.
const QUIET_PERIOD: Duration = Duration::from_millis(400);

/// Watches the source folders and raises a flag when something changed.
pub struct SourceWatcher {
    watcher: Option<RecommendedWatcher>,
    changed: Arc<AtomicBool>,
    events: std::sync::mpsc::Sender<()>,
    watched: Vec<PathBuf>,
}

impl SourceWatcher {
    pub fn new(ctx: &Context) -> Self {
        let changed = Arc::new(AtomicBool::new(false));
        let (events, raw) = channel::<()>();
        let flag = Arc::clone(&changed);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            loop {
                if raw.recv().is_err() {
                    return;
                }
                loop {
                    match raw.recv_timeout(QUIET_PERIOD) {
                        Ok(()) => {}
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
                flag.store(true, Ordering::SeqCst);
                ctx.request_repaint();
            }
        });
        Self {
            watcher: None,
            changed,
            events,
            watched: Vec::new(),
        }
    }

    /// Re-targets the watcher when the set of source folders changes. Returns folders that could not be watched.
    pub fn set_sources(&mut self, sources: &[Source], recursive: bool) -> Vec<PathBuf> {
        let wanted: Vec<PathBuf> = sources.iter().map(|s| s.path.clone()).collect();
        let key: Vec<PathBuf> = wanted
            .iter()
            .cloned()
            .chain(std::iter::once(PathBuf::from(if recursive {
                "r"
            } else {
                "f"
            })))
            .collect();
        if key == self.watched {
            return Vec::new();
        }
        self.watched = key;
        let sender = self.events.clone();
        let created = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
            if result.is_ok() {
                let _ = sender.send(());
            }
        });
        let mut failed = Vec::new();
        match created {
            Ok(mut watcher) => {
                let mode = if recursive {
                    RecursiveMode::Recursive
                } else {
                    RecursiveMode::NonRecursive
                };
                for path in &wanted {
                    if watcher.watch(path, mode).is_err() {
                        failed.push(path.clone());
                    }
                }
                self.watcher = Some(watcher);
            }
            Err(_) => {
                self.watcher = None;
                failed = wanted;
            }
        }
        failed
    }

    /// True once per burst of file system changes.
    pub fn take_changed(&self) -> bool {
        self.changed.swap(false, Ordering::SeqCst)
    }
}
