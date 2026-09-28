//! Good-experience limits: what every performance test enforces (see CLAUDE.md). They are the
//! bar for a good experience over the VPN reference link, not for "it works".

mod check;

pub use check::{Summary, check};
use std::time::Duration;

/// Opening a file, until its first byte arrives.
pub const OPEN: Duration = Duration::from_millis(1500);
/// A seek inside a video, on average.
pub const SEEK_MEAN: Duration = Duration::from_secs(1);
/// A seek inside a video, every single one.
pub const SEEK_MAX: Duration = Duration::from_secs(3);
/// Closing a file.
pub const CLOSE: Duration = Duration::from_millis(300);
/// Going back to something already read: it comes from the cache.
pub const SEEN: Duration = Duration::from_millis(100);
/// Playback without stalls: a reader consuming this many bytes per second (1080p)...
pub const PLAYBACK_RATE: u64 = 1 << 20;
/// ...that starts with, and never runs out of, this much buffered.
pub const PLAYBACK_BUFFER: Duration = Duration::from_secs(2);
