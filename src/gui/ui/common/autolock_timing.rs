//! Auto-lock timing while an assistive technology is attached.
//!
//! A screen reader never produces pointer or key events, so a user reading a page produces no
//! activity at all, and reading without moving focus produces nothing the process can see either.
//! The idle timer therefore runs longer while a reader is attached, and its last minutes are
//! announced so the user can come back before the wallet locks itself. Both halves are pure: the
//! watcher asks here and does the locking.

use crate::core::types::AutoLock;

/// What a reader hears the first time the longer idle timeout applies.
pub const GRACE_NOTICE: &str = "Auto-lock extended while a screen reader is active.";

/// The idle timeout to enforce, in seconds, once `at_active` is known.
///
/// With an assistive technology attached the configured timeout is tripled, but never past the
/// next tier the settings offer: ten minutes become thirty, an hour becomes three hours, and the
/// longest tier keeps its configured value because there is no next tier to stop at. `None`
/// (`AutoLock::Never`) stays `None`, and a session without a reader keeps the configured
/// timeout untouched.
pub fn effective_idle_secs(idle_secs: Option<u64>, at_active: bool) -> Option<u64> {
   if !at_active {
      return idle_secs;
   }

   let idle_secs = idle_secs?;

   match next_tier_secs(idle_secs) {
      Some(next_secs) => Some(idle_secs.saturating_mul(3).min(next_secs)),
      None => Some(idle_secs),
   }
}

/// The shortest configured idle timeout above `idle_secs`, if the settings offer one.
///
/// Scanning the tiers rather than looking up the stored variant also covers a dev-build
/// `AutoLock::OneMinute` that a release build still loads from `security.data`.
fn next_tier_secs(idle_secs: u64) -> Option<u64> {
   tiers()
      .iter()
      .filter_map(|tier| tier.idle_secs())
      .filter(|secs| *secs > idle_secs)
      .min()
}

/// The tier list the settings offer in this build.
fn tiers() -> &'static [AutoLock] {
   #[cfg(feature = "dev")]
   return &AutoLock::ALL_DEV;

   #[cfg(not(feature = "dev"))]
   return &AutoLock::ALL;
}

/// The lock warnings a reader hears on the way to an idle lock, longest remaining time first.
const WARNINGS: [(u64, &str); 3] = [
   (300, "Zeus locks in 5 minutes."),
   (60, "Zeus locks in 1 minute."),
   (30, "Zeus locks in 30 seconds."),
];

/// Announces that a lock is approaching, at most once per tick.
///
/// A warning whose threshold is not shorter than the timeout in force never fires, so a
/// one-minute timeout is not prefaced with "5 minutes" on its first tick. Each fires once per
/// idle period, longest first; a tick that finds the remaining time back above a threshold
/// re-arms it, so a user who returns hears the warning again on the next run down.
#[derive(Debug, Default)]
pub struct LockNotices {
   fired: [bool; WARNINGS.len()],
}

impl LockNotices {
   /// The warning due this tick, if any.
   pub fn next_notice(&mut self, remaining_secs: u64, effective_secs: u64) -> Option<&'static str> {
      for (index, (threshold, warning)) in WARNINGS.into_iter().enumerate() {
         let due = threshold < effective_secs && remaining_secs <= threshold;

         if !due {
            self.fired[index] = false;
            continue;
         }

         if !self.fired[index] {
            self.fired[index] = true;
            return Some(warning);
         }
      }

      None
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   /// Ten minutes become thirty and an hour becomes three hours, the longest tier has no next
   /// tier to stop at and keeps its value, `Never` stays never, and without a reader the
   /// configured timeout is untouched. The dev 1-minute tier triples like any other.
   #[test]
   fn a_reader_triples_the_timeout_up_to_the_next_tier() {
      assert_eq!(effective_idle_secs(Some(600), true), Some(1_800));
      assert_eq!(
         effective_idle_secs(Some(3_600), true),
         Some(10_800)
      );
      assert_eq!(
         effective_idle_secs(Some(14_400), true),
         Some(14_400)
      );
      assert_eq!(effective_idle_secs(Some(60), true), Some(180));
      assert_eq!(effective_idle_secs(None, true), None);

      assert_eq!(effective_idle_secs(Some(600), false), Some(600));
      assert_eq!(effective_idle_secs(None, false), None);
   }

   /// Every tier the settings offer is left alone without a reader, and with one it only ever
   /// grows, never past three times the configured value.
   #[test]
   fn a_reader_only_extends_a_configured_timeout() {
      for autolock in AutoLock::ALL {
         let idle = autolock.idle_secs();
         assert_eq!(
            effective_idle_secs(idle, false),
            idle,
            "{}",
            autolock.label()
         );

         let Some(idle) = idle else {
            continue;
         };
         let effective = effective_idle_secs(Some(idle), true).unwrap();
         assert!(
            (idle..=idle * 3).contains(&effective),
            "{}",
            autolock.label()
         );
      }
   }

   /// The warnings arrive once each, longest remaining time first.
   #[test]
   fn warnings_fire_once_longest_first() {
      let mut notices = LockNotices::default();
      let effective = 1_800;

      assert_eq!(notices.next_notice(1_799, effective), None);
      assert_eq!(
         notices.next_notice(300, effective),
         Some("Zeus locks in 5 minutes.")
      );
      assert_eq!(notices.next_notice(299, effective), None);
      assert_eq!(
         notices.next_notice(60, effective),
         Some("Zeus locks in 1 minute.")
      );
      assert_eq!(notices.next_notice(59, effective), None);
      assert_eq!(
         notices.next_notice(30, effective),
         Some("Zeus locks in 30 seconds.")
      );
      assert_eq!(notices.next_notice(29, effective), None);
      assert_eq!(notices.next_notice(1, effective), None);
   }

   /// A threshold at or beyond the timeout in force is never announced: a one-minute timeout is
   /// not prefaced with "5 minutes" on its first tick, and neither is the warning that equals it.
   #[test]
   fn warnings_at_or_beyond_the_timeout_never_fire() {
      let mut notices = LockNotices::default();
      assert_eq!(notices.next_notice(60, 60), None);
      assert_eq!(
         notices.next_notice(30, 60),
         Some("Zeus locks in 30 seconds.")
      );

      let mut notices = LockNotices::default();
      assert_eq!(notices.next_notice(30, 30), None);
      assert_eq!(notices.next_notice(1, 30), None);
   }

   /// Activity that takes the remaining time back above a threshold re-arms its warning.
   #[test]
   fn returning_activity_rearms_the_warning() {
      let mut notices = LockNotices::default();
      assert_eq!(
         notices.next_notice(300, 1_800),
         Some("Zeus locks in 5 minutes.")
      );
      assert_eq!(notices.next_notice(299, 1_800), None);

      assert_eq!(notices.next_notice(1_200, 1_800), None);
      assert_eq!(
         notices.next_notice(300, 1_800),
         Some("Zeus locks in 5 minutes.")
      );
   }
}
