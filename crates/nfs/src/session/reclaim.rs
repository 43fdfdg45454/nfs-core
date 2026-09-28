//! A new client id (the server restarted, or forgot this client): in the server's grace period,
//! every open file asks for its opens and locks back before RECLAIM_COMPLETE. A server that is not
//! in grace refuses (NFS4ERR_NO_GRACE): those files reopen on their next call and their locks are
//! lost.

use super::Session;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Weak};

pub trait Reclaim: Send + Sync {
    fn reclaim(self: Arc<Self>) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}

impl Session {
    pub fn opened<T: Reclaim + 'static>(&self, file: &Arc<T>) {
        let mut opens = self.opens.lock().expect("not poisoned");
        opens.retain(|f| f.strong_count() > 0);
        opens.push(Arc::downgrade(file) as Weak<dyn Reclaim>);
    }

    pub(super) async fn reclaim(&self) {
        let files: Vec<_> =
            self.opens.lock().expect("not poisoned").iter().filter_map(Weak::upgrade).collect();
        let mut tasks = tokio::task::JoinSet::new();
        files.into_iter().for_each(|file| _ = tasks.spawn(file.reclaim()));
        while tasks.join_next().await.is_some() {}
    }
}
