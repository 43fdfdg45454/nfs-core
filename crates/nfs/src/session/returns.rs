//! Delegations go back to the server (DELEGRETURN) as they are recalled, in the background: the
//! callback that recalls one is answered first.

use super::Session;
use crate::compound::Ops;
use crate::types::{Fh, Stateid};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedReceiver;

impl Session {
    pub(super) fn return_delegations(
        self: &Arc<Self>,
        mut returns: UnboundedReceiver<(Fh, Stateid)>,
    ) {
        let session = Arc::downgrade(self);
        tokio::spawn(async move {
            while let Some((fh, stateid)) = returns.recv().await {
                let Some(session) = session.upgrade() else { return };
                let mut ops = Ops::default();
                ops.putfh(&fh).delegreturn(&stateid);
                tokio::spawn(async move { _ = session.call(&ops).await });
            }
        });
    }
}
