//! How often one credential may call.
//!
//! A token bucket per profile, refilled continuously: an agent that reads a portfolio ten times in a
//! minute is doing its job, and one that calls a thousand times is looping, so the budget is generous
//! and the answer says how long to wait rather than closing the door. It is deliberately per profile
//! rather than global — one harness misbehaving must not take the others' budget — and it is not an
//! authentication throttle, because a per-address limit on *failed* attempts would let any local process
//! lock the real agent out by guessing wrong.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use zeus_core::agent::AgentProfileId;

/// How many calls one profile may make per window.
pub const CALLS_PER_WINDOW: u32 = 120;

/// The window a full budget refills over.
pub const WINDOW: Duration = Duration::from_secs(60);

/// What one profile has left.
struct Bucket {
   tokens: f64,
   refilled_at: Instant,
}

/// A budget per credential.
pub struct RateLimiter {
   capacity: f64,
   per_second: f64,
   buckets: Mutex<HashMap<AgentProfileId, Bucket>>,
}

impl RateLimiter {
   pub fn new(calls: u32, window: Duration) -> Self {
      let capacity = f64::from(calls.max(1));
      let seconds = window.as_secs_f64().max(0.001);

      Self {
         capacity,
         per_second: capacity / seconds,
         buckets: Mutex::new(HashMap::new()),
      }
   }

   /// The budget a normal session gets.
   pub fn standard() -> Self {
      Self::new(CALLS_PER_WINDOW, WINDOW)
   }

   /// Take one call from `profile`'s budget, or say how long until there is one.
   pub fn check(&self, profile: &AgentProfileId, now: Instant) -> Result<(), Duration> {
      let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());

      let bucket = buckets.entry(profile.clone()).or_insert_with(|| Bucket {
         tokens: self.capacity,
         refilled_at: now,
      });

      let elapsed = now.saturating_duration_since(bucket.refilled_at);
      bucket.tokens = (bucket.tokens + elapsed.as_secs_f64() * self.per_second).min(self.capacity);
      bucket.refilled_at = now;

      if bucket.tokens >= 1.0 {
         bucket.tokens -= 1.0;
         return Ok(());
      }

      let missing = 1.0 - bucket.tokens;
      Err(Duration::from_secs_f64(missing / self.per_second))
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   fn id() -> AgentProfileId {
      AgentProfileId::generate()
   }

   #[test]
   fn a_burst_is_allowed_then_refused_with_the_wait() {
      let limiter = RateLimiter::new(3, Duration::from_secs(3));
      let profile = id();
      let start = Instant::now();

      for _ in 0..3 {
         assert!(limiter.check(&profile, start).is_ok());
      }

      let wait = limiter.check(&profile, start).expect_err("the budget is spent");
      assert!(
         wait > Duration::ZERO && wait <= Duration::from_secs(1),
         "{wait:?}"
      );
   }

   #[test]
   fn the_budget_refills_over_the_window() {
      let limiter = RateLimiter::new(2, Duration::from_secs(2));
      let profile = id();
      let start = Instant::now();

      assert!(limiter.check(&profile, start).is_ok());
      assert!(limiter.check(&profile, start).is_ok());
      assert!(limiter.check(&profile, start).is_err());
      assert!(limiter.check(&profile, start + Duration::from_secs(1)).is_ok());
   }

   #[test]
   fn profiles_do_not_share_a_budget() {
      let limiter = RateLimiter::new(1, Duration::from_secs(60));
      let (one, other) = (id(), id());
      let start = Instant::now();

      assert!(limiter.check(&one, start).is_ok());
      assert!(limiter.check(&one, start).is_err());
      assert!(limiter.check(&other, start).is_ok());
   }
}
