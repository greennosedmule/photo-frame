use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use photoframe_store::{Layout, Store};

use crate::config::Config;

/// Everything a scan cycle needs. Cheap to clone.
#[derive(Clone)]
pub struct Ctx {
    pub store: Arc<dyn Store>,
    pub config: Arc<Config>,
    pub layout: Layout,
    shutdown: Arc<AtomicBool>,
}

impl Ctx {
    pub fn new(store: Arc<dyn Store>, config: Config) -> Self {
        let layout = Layout::new(config.library_root.clone());
        Self {
            store,
            config: Arc::new(config),
            layout,
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Set on SIGTERM. Long loops check it between items so shutdown is prompt
    /// and every interruption point is one the next scan can reconcile.
    pub fn stop(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    pub fn stopping(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }
}
