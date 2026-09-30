//! The NFSv4.1+ session every call goes through: slots, replays after a lost connection, and
//! recovery when the server forgets the session or the client.

mod attempt;
mod bind;
mod call;
mod channels;
mod info;
mod reclaim;
mod renew;
mod returns;
mod setup;
mod slots;

use crate::callback::Callbacks;
use crate::config::Config;
use crate::error::Result;
use crate::ops::session::{Channel, SessionId};
use channels::Channels;
pub use reclaim::Reclaim;
use slots::Slots;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::{Instant, SystemTime};
use tokio::sync::RwLock;

pub struct Session {
    config: Arc<Config>,
    /// The server's callbacks, and what they leave for the client (delegations, lock waits).
    pub callbacks: Arc<Callbacks>,
    channels: Arc<Channels>,
    state: RwLock<Arc<State>>,
    /// Changes with each run of the client (EXCHANGE_ID's verifier).
    verifier: [u8; 8],
    origin: Instant,
    /// Milliseconds since `origin` of the last call: the lease is renewed only when idle.
    last_call: AtomicU64,
    /// Left (Client::leave) or closed: the lease is no longer renewed.
    closed: std::sync::atomic::AtomicBool,
    /// Open files, reclaimed when the server restarts.
    opens: std::sync::Mutex<Vec<std::sync::Weak<dyn reclaim::Reclaim>>>,
}

struct State {
    clientid: u64,
    id: SessionId,
    slots: Arc<Slots>,
    fore: Channel,
    /// Counts recoveries, so that concurrent callers recover once.
    generation: u64,
}

impl Session {
    pub async fn new(config: Config) -> Result<Arc<Self>> {
        let config = Arc::new(config);
        let (callbacks, returns) = Callbacks::new();
        let channels = Channels::new(config.clone(), callbacks.clone());
        channels.warm().await?;
        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
        let verifier = (now.as_nanos() as u64).to_be_bytes();
        let state = Self::establish(&config, &channels, &callbacks, verifier, 0).await?;
        let session = Arc::new(Self {
            config,
            callbacks,
            channels,
            state: RwLock::new(state),
            verifier,
            origin: Instant::now(),
            last_call: 0.into(),
            closed: false.into(),
            opens: Default::default(),
        });
        session.reclaim_complete().await?;
        session.return_delegations(returns);
        Ok(session)
    }

    async fn establish(
        config: &Config,
        channels: &Arc<Channels>,
        callbacks: &Callbacks,
        verifier: [u8; 8],
        generation: u64,
    ) -> Result<Arc<State>> {
        let e = setup::establish(&channels.get(false).await?, config, verifier).await?;
        callbacks.set_session(e.id);
        channels.bind_all(e.id);
        let slots = Slots::new(e.fore.slots.clamp(1, 64));
        Ok(Arc::new(State { clientid: e.clientid, id: e.id, slots, fore: e.fore, generation }))
    }

    /// The server forgot the session (or the client): a new one, once for all callers.
    async fn recover(&self, generation: u64) -> Result<()> {
        let mut state = self.state.write().await;
        if state.generation == generation {
            let (callbacks, next) = (&self.callbacks, generation + 1);
            let new = Self::establish(&self.config, &self.channels, callbacks, self.verifier, next)
                .await?;
            let restarted = new.clientid != state.clientid;
            if restarted {
                callbacks.delegations.forget();
            }
            *state = new;
            drop(state);
            if restarted {
                self.reclaim().await;
            }
            self.reclaim_complete().await?;
        }
        Ok(())
    }
}
