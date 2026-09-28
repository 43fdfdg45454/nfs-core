//! The lease: every call renews it; when idle for a third of it, a SEQUENCE alone does.

use super::Session;
use crate::compound::Ops;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

impl Session {
    pub fn keep_lease(self: &Arc<Self>, lease: Duration) {
        let session = Arc::downgrade(self);
        let period = lease / 3;
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(period).await;
                let Some(session) = session.upgrade() else {
                    return;
                };
                let last = Duration::from_millis(session.last_call.load(Ordering::Relaxed));
                if session.origin.elapsed().saturating_sub(last) >= period {
                    _ = session.call(&Ops::default()).await;
                }
            }
        });
    }
}
