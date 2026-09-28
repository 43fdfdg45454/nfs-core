//! A strict video player: 128 KiB reads, playback at nfs_limits::PLAYBACK_RATE
//! once nfs_limits::PLAYBACK_BUFFER is buffered, and never more than that buffer read ahead: any
//! longer wait for the network is a stall. Every byte is checked against the fixtures' pattern:
//! each 8-byte word holds its offset divided by 8, plus the file's label shifted by 48 bits.

#![allow(dead_code, unused_imports)]

mod other;
mod setup;

pub use other::*;
pub use setup::*;

use nfs_engine::Reader;
use nfs_limits::{PLAYBACK_BUFFER, PLAYBACK_RATE};
use std::time::{Duration, Instant};

pub const CHUNK: u64 = 128 << 10;

pub struct Played {
    /// From the first read to its data.
    pub first_byte: Duration,
    pub stalls: u32,
    pub stalled: Duration,
}

/// Plays `seconds` from `from`.
pub async fn play(reader: &Reader, label: u64, from: u64, seconds: u64) -> Played {
    let chunks = seconds * PLAYBACK_RATE / CHUNK;
    let buffered = PLAYBACK_BUFFER.as_secs() * PLAYBACK_RATE / CHUNK;
    let begin = Instant::now();
    let (mut arrivals, mut playing) = (Vec::new(), None::<Instant>);
    for k in 0..chunks {
        let offset = from + k * CHUNK;
        if offset >= reader.size() {
            break;
        }
        if let Some(start) = playing {
            let due = start + Duration::from_secs_f64((k * CHUNK) as f64 / PLAYBACK_RATE as f64);
            tokio::time::sleep_until((due.saturating_sub(PLAYBACK_BUFFER)).into()).await;
        }
        let data = reader.read_at(offset, CHUNK as usize).await.expect("read");
        check(label, offset, &data);
        arrivals.push(begin.elapsed());
        if k + 1 == buffered {
            playing = Some(Instant::now());
        }
    }
    stalls(&arrivals, buffered as usize)
}

trait SaturatingSub {
    fn saturating_sub(self, d: Duration) -> Instant;
}

impl SaturatingSub for Instant {
    fn saturating_sub(self, d: Duration) -> Instant {
        self.checked_sub(d).unwrap_or(self)
    }
}

/// Playback starts when `buffered` chunks arrived; chunk k is due k chunks of playback later,
/// plus the time already stalled.
fn stalls(arrivals: &[Duration], buffered: usize) -> Played {
    let first_byte = arrivals.first().copied().unwrap_or_default();
    let Some(&start) = arrivals.get(buffered.saturating_sub(1)) else {
        return Played { first_byte, stalls: 0, stalled: Duration::ZERO };
    };
    let (mut stalls, mut stalled) = (0, Duration::ZERO);
    for (k, &arrived) in arrivals.iter().enumerate().skip(buffered) {
        let due = start
            + stalled
            + Duration::from_secs_f64((k * CHUNK as usize) as f64 / PLAYBACK_RATE as f64);
        if arrived > due {
            stalls += 1;
            stalled += arrived - due;
        }
    }
    Played { first_byte, stalls, stalled }
}
