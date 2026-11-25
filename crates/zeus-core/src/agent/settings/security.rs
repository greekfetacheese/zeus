//! The `security` screen: the auto-lock timeout, and nothing that gates the vault.
//!
//! The write is a **whitelist of one field**, not a grant on the Security page. It may make the idle
//! window shorter, never longer: a longer window widens every other grant a profile holds, so the check
//! lives in the writer rather than in a note telling callers to be careful. Credentials, the KDF
//! parameters, privacy mode and the per-endpoint MEV preference are out of reach, each for a reason the
//! refusal states.

use super::{Applied, Closed, SettingsError, SettingsScreen, deserialize, refuse_closed};
use crate::ZeusCtx;
use crate::types::AutoLock;
use serde::Deserialize;
use serde_json::{Value, json};

/// One patch of the security screen.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Patch {
   autolock: Option<Choice>,
}

/// The idle timeouts, by the names the patch spells them.
///
/// `one_minute` is the dev build's option. It stays accepted because the variant is compiled into a
/// release build too (a `security.data` written by a dev build has to load), and a *shorter* window is
/// exactly the direction an agent is allowed to move.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Choice {
   OneMinute,
   TenMinutes,
   OneHour,
   FourHours,
   Never,
}

impl Choice {
   fn to_auto_lock(self) -> AutoLock {
      match self {
         Self::OneMinute => AutoLock::OneMinute,
         Self::TenMinutes => AutoLock::TenMinutes,
         Self::OneHour => AutoLock::OneHour,
         Self::FourHours => AutoLock::FourHours,
         Self::Never => AutoLock::Never,
      }
   }
}

/// What this screen never lets an agent write.
const CLOSED: &[Closed] = &[
   Closed {
      field: "credentials",
      runtime: false,
      why: "the vault's password is never a grant, because an agent that can set it owns the wallet",
   },
   Closed {
      field: "kdf",
      runtime: false,
      why: "the KDF parameters are part of the vault's header and are never writable",
   },
   Closed {
      field: "privacy_mode",
      runtime: true,
      why: "privacy mode is per-session state, changed in the main window, not a persisted setting",
   },
   Closed {
      field: "mev_protect",
      runtime: false,
      why: "MEV protection is a per-endpoint choice, set with the endpoint it belongs to rather than on this screen",
   },
];

pub const SCREEN: SettingsScreen = SettingsScreen {
   name: "security",
   capability: "settings.security.write",
   schema,
   closed: CLOSED,
   write,
};

fn schema() -> Value {
   json!({
       "type": "object",
       "additionalProperties": false,
       "properties": {
           "autolock": {
               "type": "string",
               "enum": ["one_minute", "ten_minutes", "one_hour", "four_hours", "never"],
               "persisted": true,
               "description": "The idle timeout before Zeus locks. It may be shortened, never lengthened.",
           },
       },
   })
}

fn write(ctx: &ZeusCtx, value: &Value) -> Result<Applied, SettingsError> {
   let patch = check(value)?;
   let applied = apply(ctx, patch)?;

   if applied.changed.is_empty() {
      return Ok(applied);
   }

   ctx.save_security_settings()
      .map_err(|error| SettingsError::refused(error.to_string()))?;

   Ok(applied)
}

pub(super) fn check(value: &Value) -> Result<Patch, SettingsError> {
   refuse_closed(value, CLOSED)?;
   deserialize(value, "security")
}

pub(super) fn apply(ctx: &ZeusCtx, patch: Patch) -> Result<Applied, SettingsError> {
   let Some(choice) = patch.autolock else {
      return Ok(Applied::none());
   };

   let next = choice.to_auto_lock();
   let current = ctx.read(|ctx| ctx.security.autolock);

   if rank(next) > rank(current) {
      return Err(SettingsError::invalid(format!(
         "the auto-lock is {} now; an agent may shorten it, never lengthen it",
         current.label()
      )));
   }

   ctx.write(|ctx| {
      ctx.security.autolock = next;
      // An explicit choice: stop the page's "auto-lock is not configured" nudge.
      ctx.security.autolock_changed = true;
      // And it supersedes a file that did not read, the same way a choice on the page does — clearing
      // this is what lets the save touch that file instead of refusing over it forever.
      ctx.security.load_failed = false;
   });

   Ok(Applied {
      changed: vec!["autolock"],
   })
}

/// How long a window is, with "never" ranked as the longest of all.
fn rank(lock: AutoLock) -> u64 {
   lock.idle_secs().unwrap_or(u64::MAX)
}
