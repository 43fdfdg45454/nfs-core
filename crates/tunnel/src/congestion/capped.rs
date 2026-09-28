//! BBR with a cap on the bytes in flight, a small multiple of the path's bandwidth-delay product.
//! quinn's BBR (v1) keeps up to 2 BDP in flight (2.89 in startup): the extra waits in the
//! bottleneck's queue, and so does every small request behind it (stage 1, VPN link: ~190 ms on
//! average and over 1 s at peaks). BBR still sets the pace; the cap only keeps the queue short.

use super::estimate::Bdp;
use quinn::congestion::{BbrConfig, Controller, ControllerFactory};
use quinn_proto::RttEstimator;
use std::any::Any;
use std::sync::Arc;
use std::time::Instant;

pub struct CappedBbrConfig {
    /// The cap, in bandwidth-delay products.
    pub gain: f64,
}

impl ControllerFactory for CappedBbrConfig {
    fn build(self: Arc<Self>, now: Instant, mtu: u16) -> Box<dyn Controller> {
        let inner = Arc::new(BbrConfig::default()).build(now, mtu);
        Box::new(CappedBbr {
            floor: inner.initial_window(),
            inner,
            gain: self.gain,
            bdp: Bdp::new(now),
        })
    }
}

struct CappedBbr {
    inner: Box<dyn Controller>,
    gain: f64,
    floor: u64,
    bdp: Bdp,
}

impl Controller for CappedBbr {
    fn on_sent(&mut self, now: Instant, bytes: u64, last_packet_number: u64) {
        self.inner.on_sent(now, bytes, last_packet_number);
    }

    fn on_ack(
        &mut self,
        now: Instant,
        sent: Instant,
        bytes: u64,
        app_limited: bool,
        rtt: &RttEstimator,
    ) {
        self.inner.on_ack(now, sent, bytes, app_limited, rtt);
        self.bdp.on_ack(now, bytes, app_limited, rtt.min());
    }

    fn on_end_acks(
        &mut self,
        now: Instant,
        in_flight: u64,
        app_limited: bool,
        largest: Option<u64>,
    ) {
        self.inner.on_end_acks(now, in_flight, app_limited, largest);
    }

    fn on_congestion_event(
        &mut self,
        now: Instant,
        sent: Instant,
        persistent: bool,
        lost_bytes: u64,
    ) {
        self.inner.on_congestion_event(now, sent, persistent, lost_bytes);
    }

    fn on_mtu_update(&mut self, new_mtu: u16) {
        self.inner.on_mtu_update(new_mtu);
    }

    fn window(&self) -> u64 {
        let cap = self.bdp.get().map_or(u64::MAX, |bdp| ((bdp * self.gain) as u64).max(self.floor));
        self.inner.window().min(cap)
    }

    fn clone_box(&self) -> Box<dyn Controller> {
        Box::new(Self { inner: self.inner.clone_box(), ..*self })
    }

    fn initial_window(&self) -> u64 {
        self.inner.initial_window()
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}
