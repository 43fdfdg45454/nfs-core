//! The path's bandwidth-delay product, estimated as BBR does: the highest delivery rate of the
//! last rounds (one minimum RTT each) times the minimum RTT.

use std::time::{Duration, Instant};

const ROUNDS: usize = 10;

#[derive(Clone, Copy)]
pub struct Bdp {
    min_rtt: Duration,
    /// Start of the current round and the bytes acknowledged in it.
    round: (Instant, u64),
    /// Delivery rate of the last rounds, bytes per second.
    rates: [f64; ROUNDS],
    next: usize,
}

impl Bdp {
    pub fn new(now: Instant) -> Self {
        Self { min_rtt: Duration::MAX, round: (now, 0), rates: [0.0; ROUNDS], next: 0 }
    }

    pub fn on_ack(&mut self, now: Instant, bytes: u64, app_limited: bool, min_rtt: Duration) {
        self.min_rtt = self.min_rtt.min(min_rtt);
        self.round.1 += bytes;
        let elapsed = now.duration_since(self.round.0);
        if elapsed >= self.min_rtt {
            // A round limited by the application says nothing about the path.
            if !app_limited {
                self.rates[self.next] = self.round.1 as f64 / elapsed.as_secs_f64();
                self.next = (self.next + 1) % ROUNDS;
            }
            self.round = (now, 0);
        }
    }

    /// In bytes, once there is a measurement.
    pub fn get(&self) -> Option<f64> {
        let rate = self.rates.iter().copied().fold(0.0, f64::max);
        (rate > 0.0 && self.min_rtt != Duration::MAX).then_some(rate * self.min_rtt.as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vpn_link_is_one_and_a_quarter_megabytes() {
        let (start, rtt) = (Instant::now(), Duration::from_millis(100));
        let mut bdp = Bdp::new(start);
        assert_eq!(bdp.get(), None);
        // 12.5 MB/s acknowledged in 10 ms steps, with one application-limited round ignored.
        for step in 1..=30 {
            let limited = (11..=20).contains(&step);
            bdp.on_ack(start + Duration::from_millis(10 * step), 125_000, limited, rtt);
        }
        let got = bdp.get().unwrap();
        assert!((1.24e6..1.26e6).contains(&got), "{got}");
    }
}
