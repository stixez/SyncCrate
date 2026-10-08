//! One upload budget for the host, shared by every friend syncing at once.
//!
//! The old throttle paced each connection on its own, so "10 MB/s" with five
//! friends at a LAN party meant 50 MB/s out of the host, and it read the
//! setting once per connection, so changing it did nothing until friends
//! reconnected. Here every chunk reserves the next free slot on one shared
//! clock: total throughput stays at the limit however many peers pull, and
//! a new limit applies to the very next chunk.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::Instant;

/// Bytes per second; 0 = unlimited. `UNSET` until first read from the config.
static RATE: AtomicU64 = AtomicU64::new(UNSET);
const UNSET: u64 = u64::MAX;
/// When the shared upload clock is free again.
static NEXT_FREE: Mutex<Option<Instant>> = Mutex::new(None);

pub fn set_rate(bytes_per_sec: u64) {
    RATE.store(bytes_per_sec, Ordering::Relaxed);
}

fn rate() -> u64 {
    match RATE.load(Ordering::Relaxed) {
        UNSET => {
            let r = crate::commands::sync::get_speed_limit();
            RATE.store(r, Ordering::Relaxed);
            r
        }
        r => r,
    }
}

/// Reserve `n` bytes on a shared clock that's free from `next_free`: returns
/// when this send may start and when the clock is free afterwards. Idle time
/// isn't saved up, so a long pause never allows a burst over the limit.
pub fn schedule(next_free: Option<Instant>, now: Instant, n: u64, rate: u64) -> (Instant, Instant) {
    let start = next_free.map_or(now, |t| t.max(now));
    (start, start + Duration::from_secs_f64(n as f64 / rate as f64))
}

/// Wait for this chunk's turn before sending it (no wait when unlimited).
pub async fn before_send(n: u64) {
    let r = rate();
    if r == 0 {
        return;
    }
    let start = {
        let mut next = NEXT_FREE.lock().unwrap_or_else(|e| e.into_inner());
        let (start, free) = schedule(*next, Instant::now(), n, r);
        *next = Some(free);
        start
    };
    tokio::time::sleep_until(start).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peers_share_one_budget() {
        let t0 = Instant::now();
        let mb = 1_000_000;
        // Two friends asking at the same moment take turns on one clock:
        // 4 MB at 1 MB/s takes 4 s in total, not 2.
        let mut next = None;
        let mut starts = Vec::new();
        for _ in 0..4 {
            let (start, free) = schedule(next, t0, mb, mb);
            starts.push(start - t0);
            next = Some(free);
        }
        assert_eq!(starts, vec![Duration::ZERO, Duration::from_secs(1), Duration::from_secs(2), Duration::from_secs(3)]);
    }

    #[test]
    fn idle_time_is_not_saved_up_for_a_burst() {
        let t0 = Instant::now();
        let (_, free) = schedule(None, t0, 1_000, 1_000);
        // Ten seconds later the clock starts from now, not from its old slot.
        let later = t0 + Duration::from_secs(10);
        let (start, free2) = schedule(Some(free), later, 1_000, 1_000);
        assert_eq!((start, free2), (later, later + Duration::from_secs(1)));
    }
}
