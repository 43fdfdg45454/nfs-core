//! How far ahead a reader is read: the more it reads without jumping, the further. A player that
//! just jumped gets nothing until it reads on, then a little; one that plays steadily gets the
//! whole read-ahead.

use crate::shared::Shared;
use crate::{BLOCK, Config, PIECE};
use std::sync::Arc;

const MIB: u64 = 1 << 20;

/// Bytes ahead of a reader that read `run` bytes since its last jump.
pub fn window(run: u64, config: &Config) -> u64 {
    match run {
        // One read since a jump may be a frame shown while scrubbing: nothing ahead until the
        // reader goes on (stage 3, TCP: 30 jumps left ~120 MiB of read-ahead nobody wanted).
        r if r <= PIECE => 0,
        r if r < MIB => 4 * MIB,
        r if r < 8 * MIB => 16 * MIB,
        // Without a disk cache, only what fits in memory.
        _ if config.cache.is_none() => config.memory_ahead,
        _ => config.read_ahead.max(config.memory_ahead),
    }
}

/// Queues the blocks of the window after `position` not yet here nor queued, starting after
/// `scheduled` (the end of what this reader queued before), which it moves forward.
pub fn schedule(shared: &Arc<Shared>, position: u64, run: u64, scheduled: &mut u64) {
    let block_bytes = BLOCK * PIECE;
    let window = window(run, &shared.config);
    // A steady player with the whole read-ahead: refilled in bursts, not a block per block read,
    // so that the phone's radio rests in between (a minute at 1 MB/s). What is ahead never drops
    // below three quarters of the window, far more than a player needs.
    if window > 16 * MIB && *scheduled > position + window - window / 4 {
        return;
    }
    let end = (position + window).min(shared.end());
    let first = position.max(*scheduled) / block_bytes;
    let last = end.div_ceil(block_bytes);
    for block in first..last {
        let pieces = block * BLOCK..(block + 1) * BLOCK;
        let missing = pieces.clone().any(|i| i * PIECE < shared.size && !shared.present(i));
        if missing && shared.queued.lock().expect("not poisoned").insert(block) {
            shared.queue.push(shared, block, (block * block_bytes).saturating_sub(position));
        }
    }
    *scheduled = (*scheduled).max(last * block_bytes);
}
