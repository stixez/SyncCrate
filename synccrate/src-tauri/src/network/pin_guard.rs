//! Slows down PIN guessing. A session PIN is short (it has to fit a join
//! code), so without this anyone on the LAN, or anyone who knows the host's
//! internet id, could try every value in seconds.
//!
//! Per source (IP for LAN, proven node id for iroh): 3 free tries, then a
//! lockout that doubles from 30 s up to 15 min. Globally: more than 20 wrong
//! PINs within a minute locks PIN attempts for everyone for that minute,
//! since iroh ids are free to generate and a per-source limit alone doesn't
//! stop a distributed guesser. A correct PIN clears its source's count.
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

const FREE_TRIES: u32 = 3;
const FIRST_LOCKOUT: Duration = Duration::from_secs(30);
const MAX_LOCKOUT: Duration = Duration::from_secs(15 * 60);
const GLOBAL_WINDOW: Duration = Duration::from_secs(60);
const GLOBAL_MAX_FAILURES: usize = 20;
const MAX_SOURCES: usize = 1024;

#[derive(Debug, Default)]
pub struct PinGuard {
    per_source: HashMap<String, (u32, Option<Instant>)>,
    recent: VecDeque<Instant>,
}

impl PinGuard {
    /// `Err(wait)` while `source` (or everyone) is locked out.
    ///
    /// `trusted`: a proven crew member (iroh id), exempt from the global
    /// limit, or anyone who knows the host's id could keep real friends
    /// locked out indefinitely by guessing 20 times a minute. Its own
    /// per-source lockout still applies.
    pub fn check(&mut self, source: &str, trusted: bool, now: Instant) -> Result<(), Duration> {
        while self.recent.front().is_some_and(|t| now.duration_since(*t) >= GLOBAL_WINDOW) {
            self.recent.pop_front();
        }
        if !trusted && self.recent.len() >= GLOBAL_MAX_FAILURES {
            let oldest = *self.recent.front().expect("non-empty");
            return Err(GLOBAL_WINDOW.saturating_sub(now.duration_since(oldest)));
        }
        if let Some((_, Some(until))) = self.per_source.get(source) {
            if *until > now {
                return Err(*until - now);
            }
        }
        Ok(())
    }

    pub fn record_failure(&mut self, source: &str, now: Instant) {
        self.recent.push_back(now);
        if self.per_source.len() >= MAX_SOURCES && !self.per_source.contains_key(source) {
            // Bounded memory under a flood of fresh sources. Keep active
            // lockouts (clearing everything freed a locked-out guesser too);
            // only if they alone fill the table, drop them (the global limit
            // still applies).
            self.per_source.retain(|_, (_, until)| until.is_some_and(|u| u > now));
            if self.per_source.len() >= MAX_SOURCES {
                self.per_source.clear();
            }
        }
        let entry = self.per_source.entry(source.to_string()).or_insert((0, None));
        entry.0 += 1;
        if entry.0 >= FREE_TRIES {
            let doublings = (entry.0 - FREE_TRIES).min(8);
            let lock = FIRST_LOCKOUT.saturating_mul(1 << doublings).min(MAX_LOCKOUT);
            entry.1 = Some(now + lock);
        }
    }

    pub fn record_success(&mut self, source: &str) {
        self.per_source.remove(source);
    }
}

/// Compare without an early exit on the first differing byte.
pub fn pin_matches(provided: &str, expected: &str) -> bool {
    let (a, b) = (provided.as_bytes(), expected.as_bytes());
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= (a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0)) as usize;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_source_lockout_doubles_and_success_clears_it() {
        let mut g = PinGuard::default();
        let t0 = Instant::now();
        for _ in 0..2 {
            assert!(g.check("1.2.3.4", false, t0).is_ok());
            g.record_failure("1.2.3.4", t0);
        }
        assert!(g.check("1.2.3.4", false, t0).is_ok(), "two misses are free");
        g.record_failure("1.2.3.4", t0);
        let wait = g.check("1.2.3.4", false, t0).unwrap_err();
        assert_eq!(wait, FIRST_LOCKOUT);
        assert!(g.check("5.6.7.8", false, t0).is_ok(), "other sources aren't affected");
        let t1 = t0 + FIRST_LOCKOUT;
        assert!(g.check("1.2.3.4", false, t1).is_ok(), "lockout expires");
        g.record_failure("1.2.3.4", t1);
        assert_eq!(g.check("1.2.3.4", false, t1).unwrap_err(), FIRST_LOCKOUT * 2, "then doubles");
        g.record_success("1.2.3.4");
        assert!(g.check("1.2.3.4", false, t1).is_ok());
    }

    #[test]
    fn lockout_is_capped_and_many_sources_trip_the_global_limit() {
        let mut g = PinGuard::default();
        let t0 = Instant::now();
        for _ in 0..40 {
            g.record_failure("a", t0);
        }
        assert!(g.check("a", false, t0).unwrap_err() <= MAX_LOCKOUT);

        let mut g = PinGuard::default();
        for i in 0..GLOBAL_MAX_FAILURES {
            assert!(g.check(&format!("id{i}"), false, t0).is_ok());
            g.record_failure(&format!("id{i}"), t0);
        }
        assert!(g.check("fresh-id", false, t0).is_err(), "a distributed guesser is stopped too");
        assert!(g.check("fresh-id", false, t0 + GLOBAL_WINDOW).is_ok(), "for one minute");
        assert!(g.check("crew-member", true, t0).is_ok(), "proven crew members aren't locked out by strangers");
    }

    #[test]
    fn a_full_table_keeps_active_lockouts() {
        let mut g = PinGuard::default();
        let t0 = Instant::now();
        for _ in 0..FREE_TRIES {
            g.record_failure("guesser", t0);
        }
        assert!(g.check("guesser", true, t0).is_err());
        for i in 0..MAX_SOURCES {
            g.record_failure(&format!("one-miss-{i}"), t0);
        }
        assert!(g.check("guesser", true, t0).is_err(), "flooding fresh sources must not clear a lockout");
        assert!(g.per_source.len() < MAX_SOURCES);
    }

    #[test]
    fn pin_compare() {
        assert!(pin_matches("12345", "12345"));
        assert!(!pin_matches("12346", "12345"));
        assert!(!pin_matches("1234", "12345"));
        assert!(!pin_matches("", "12345"));
    }
}
