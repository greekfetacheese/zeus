use super::super::{ZeusContext, ZeusCtx};
use crate::core::types::*;
use crate::core::ui;

impl ZeusCtx {
   pub fn vault_exists(&self) -> bool {
      self.read(|ctx| ctx.vault_exists)
   }

   pub fn vault_unlocked(&self) -> bool {
      self.read(|ctx| ctx.vault_unlocked)
   }

   /// True while the UI is locked by auto-lock.
   pub fn is_locked(&self) -> bool {
      self.read(|ctx| ctx.locked)
   }

   /// Guard for anything that signs, sends, or authorizes on the user's behalf.
   ///
   /// Auto-lock gates the UI, but headless callers — the dapp server, a built-in
   /// flow still mid-simulation — have no window to be hidden by, so every
   /// signing / broadcasting chokepoint re-checks the lock here.
   pub fn ensure_unlocked(&self) -> Result<(), anyhow::Error> {
      if self.is_locked() {
         return Err(anyhow::anyhow!("Zeus is locked"));
      }
      Ok(())
   }

   /// Verify `other` against the in-memory vault, counting the attempt.
   ///
   /// This is the Argon2-free re-login / confirm check (the lock screen and the
   /// confirm-password dialogs). Because it is pure equality — no KDF to slow a
   /// guess — it is capped: after `MAX_CREDENTIAL_ATTEMPTS` failures Zeus
   /// refuses further tries and shuts down.
   ///
   /// `login_only` picks [`Vault::credentials_match_login`] (username +
   /// password) over [`Vault::credentials_match`], which also compares the
   /// confirm field the login form does not have.
   pub fn check_credentials(
      &self,
      other: &ncrypt_me::Credentials,
      login_only: bool,
   ) -> CredentialCheck {
      // Refuse before comparing once the cap is reached.
      if self.read(|ctx| ctx.security.credential_attempts) >= MAX_CREDENTIAL_ATTEMPTS {
         Self::credential_lockout_shutdown();
         return CredentialCheck::LockedOut;
      }

      let matched = self.read_vault(|vault| {
         if login_only {
            vault.credentials_match_login(other)
         } else {
            vault.credentials_match(other)
         }
      });

      let result = self.write(|ctx| ctx.security.note_credential_result(matched));

      if result == CredentialCheck::LockedOut {
         Self::credential_lockout_shutdown();
      }

      result
   }

   /// Tell the user and shut Zeus down (see [`Self::check_credentials`]).
   ///
   /// The notice window stays up while `ZeusApp::on_shutdown` saves the vault,
   /// so it is readable before the window closes.
   fn credential_lockout_shutdown() {
      tracing::warn!("Too many failed credential attempts, shutting Zeus down");
      ui::host().lockout_and_shutdown("Too many failed attempts. Zeus will close.");
   }
}

impl ZeusContext {
   /// Whether an "auto-lock is not configured" toast is due.
   ///
   /// Fires at most once per `repeat_ms`, and the first one only after
   /// `first_delay_ms` from the seeded timestamp (both are set at unlock).
   /// Returns false once the user has made an explicit choice.
   pub fn should_warn_autolock(
      &mut self,
      now_millis: u64,
      first_delay_ms: u64,
      repeat_ms: u64,
   ) -> bool {
      if self.security.autolock_changed || self.locked {
         return false;
      }

      let delay = if self.autolock_notice_shown {
         repeat_ms
      } else {
         first_delay_ms
      };

      if now_millis.saturating_sub(self.last_autolock_notice_ms) < delay {
         return false;
      }

      self.last_autolock_notice_ms = now_millis;
      self.autolock_notice_shown = true;
      true
   }
}
