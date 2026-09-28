//! The congestion controller of every QUIC connection. Stage 1 measured that the throughput over
//! a lossy link comes from BBR (QUIC with CUBIC: a tenth of it), so BBR is not optional.

mod capped;
mod estimate;

use capped::CappedBbrConfig;
use quinn::congestion::{BbrConfig, ControllerFactory};
use std::str::FromStr;
use std::sync::Arc;

/// BBR, with the bytes in flight capped at `cap` bandwidth-delay products (see capped.rs),
/// or not capped. Written `bbr` or `bbr/<cap>`.
#[derive(Clone, Copy, Debug)]
pub struct Congestion {
    pub cap: Option<f64>,
}

/// The cap stage 1 settled on.
impl Default for Congestion {
    fn default() -> Self {
        Self { cap: Some(1.5) }
    }
}

impl FromStr for Congestion {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.split_once('/') {
            None if s == "bbr" => Ok(Self { cap: None }),
            Some(("bbr", cap)) => {
                cap.parse().map(|cap| Self { cap: Some(cap) }).map_err(|e| format!("{s}: {e}"))
            }
            _ => Err(format!("unknown congestion controller {s}: bbr or bbr/<cap>")),
        }
    }
}

impl Congestion {
    pub fn factory(self) -> Arc<dyn ControllerFactory + Send + Sync> {
        match self.cap {
            None => Arc::new(BbrConfig::default()),
            Some(gain) => Arc::new(CappedBbrConfig { gain }),
        }
    }
}
