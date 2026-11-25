//! What each screen accepts, what it refuses, and the difference between the two.
//!
//! Every test here stops at `apply`: the mutation and its checks, with no file written. The save is the
//! screen's own helper — the one the GUI already calls — and is exercised from the edge's integration
//! suite, where a write has a temporary working directory to land in.

use super::*;
use crate::ZeusCtx;
use crate::agent::profile::{CapabilityScope, CapabilityVerb, capability};
use crate::agent::read;
use crate::types::AutoLock;
use serde_json::json;

fn context() -> ZeusCtx {
   let ctx = ZeusCtx::new(crate::platform::runtime::test_handle());

   ctx.write(|ctx| {
      ctx.vault_unlocked = true;
      ctx.locked = false;
   });

   ctx
}

#[test]
fn a_patch_cannot_name_a_field_its_screen_does_not_have() {
   for screen in SCREENS {
      let error = (screen.write)(&context(), &json!({"nonsense": true}))
         .expect_err("an unknown field is refused, not ignored");

      assert!(
         matches!(error, SettingsError::Invalid { .. }),
         "{}: {error}",
         screen.name
      );
      assert!(
         error.to_string().contains("nonsense"),
         "{}: {error}",
         screen.name
      );
   }
}

#[test]
fn a_runtime_field_is_refused_with_the_per_call_sentence() {
   let error = (security::SCREEN.write)(&context(), &json!({"privacy_mode": true}))
      .expect_err("privacy mode is not a setting");

   assert_eq!(
      error,
      SettingsError::RuntimeOnly {
         field: "privacy_mode".to_owned(),
      }
   );
   assert!(error.to_string().contains("per call"));
}

#[test]
fn a_closed_field_is_refused_with_its_own_reason() {
   let error =
      general::check(&json!({"agent_server": true})).expect_err("the switch is the user's");

   assert!(
      matches!(
         error,
         SettingsError::Closed {
            field: "agent_server",
            ..
         }
      ),
      "{error}"
   );
   assert!(error.to_string().contains("both directions"));
}

#[test]
fn a_general_patch_sets_only_what_it_names() {
   let ctx = context();
   let patch = general::check(&json!({"check_for_updates": true})).expect("a patch");

   let applied = general::apply(&ctx, patch).expect("the patch applies");

   assert_eq!(applied.changed, ["check_for_updates"]);
   assert!(ctx.read(|ctx| ctx.misc_config.check_for_updates()));
   assert!(!ctx.read(|ctx| ctx.misc_config.fetch_asset_images()));
}

#[test]
fn a_patch_that_names_nothing_changes_nothing() {
   let ctx = context();
   let patch = general::check(&json!({})).expect("an empty patch");

   assert_eq!(
      general::apply(&ctx, patch).expect("the patch applies"),
      Applied::none()
   );
}

#[test]
fn the_general_schema_has_no_agent_server_switch() {
   let described = describe("general");

   assert_eq!(described["writable"], true);
   assert!(described["schema"]["properties"]["agent_server"].is_null());
   assert!(
      described["closed"]
         .as_array()
         .expect("a closed list")
         .iter()
         .any(|entry| entry["field"] == "agent_server")
   );
}

#[test]
fn an_agent_may_shorten_the_auto_lock_but_never_lengthen_it() {
   let ctx = context();
   ctx.write(|ctx| ctx.security.autolock = AutoLock::TenMinutes);

   let longer = security::check(&json!({"autolock": "four_hours"})).expect("a patch");
   let error = security::apply(&ctx, longer).expect_err("a longer window widens every grant");

   assert!(
      error.to_string().contains("never lengthen"),
      "{error}"
   );
   assert_eq!(
      ctx.read(|ctx| ctx.security.autolock),
      AutoLock::TenMinutes
   );

   let shorter = security::check(&json!({"autolock": "one_minute"})).expect("a patch");
   assert_eq!(
      security::apply(&ctx, shorter).expect("a shorter window").changed,
      ["autolock"]
   );
   assert_eq!(
      ctx.read(|ctx| ctx.security.autolock),
      AutoLock::OneMinute
   );
}

#[test]
fn choosing_an_auto_lock_clears_a_read_failure_the_way_the_page_does() {
   let ctx = context();
   ctx.write(|ctx| ctx.security.load_failed = true);

   let patch = security::check(&json!({"autolock": "one_hour"})).expect("a patch");
   security::apply(&ctx, patch).expect("the patch applies");

   assert!(!ctx.read(|ctx| ctx.security.load_failed));
   assert!(ctx.read(|ctx| ctx.security.autolock_changed));
}

#[test]
fn concurrency_is_refused_outside_the_range_the_app_allows() {
   let ctx = context();

   let too_high = networks::check(&json!({"concurrency": 99})).expect("a patch");
   let error = networks::apply(&ctx, too_high).expect_err("99 is past the ceiling");

   assert!(
      error.to_string().contains("between 1 and 16"),
      "{error}"
   );

   let fine = networks::check(&json!({"concurrency": 4})).expect("a patch");
   assert_eq!(
      networks::apply(&ctx, fine).expect("the patch applies").changed,
      ["concurrency"]
   );
   assert_eq!(ctx.get_client_manager().concurrency(), 4);
}

#[test]
fn a_chain_this_build_does_not_know_is_refused_before_anything_lands() {
   let ctx = context();
   let patch =
      networks::check(&json!({"concurrency": 4, "disable_chains": [999]})).expect("a patch");

   let error = networks::apply(&ctx, patch).expect_err("999 is not a chain");

   assert_eq!(error, SettingsError::Unsupported { chain: 999 });
   assert_eq!(
      ctx.get_client_manager().concurrency(),
      2,
      "a refused patch changes nothing at all"
   );
}

#[test]
fn a_chain_is_switched_off_and_back_on() {
   let ctx = context();

   let off = networks::check(&json!({"disable_chains": [1]})).expect("a patch");
   networks::apply(&ctx, off).expect("the patch applies");
   assert!(ctx.read(|ctx| ctx.disabled_chains.is_disabled(1)));

   let on = networks::check(&json!({"enable_chains": [1]})).expect("a patch");
   networks::apply(&ctx, on).expect("the patch applies");
   assert!(!ctx.read(|ctx| ctx.disabled_chains.is_disabled(1)));
}

#[test]
fn a_contact_is_added_renamed_and_removed() {
   let ctx = context();
   let stored = "0xabcdef0123456789abcdef0123456789abcdef01";

   let add =
      contacts::check(&json!({"add": [{"name": "Alice", "address": stored}]})).expect("a patch");
   assert_eq!(
      contacts::apply(&ctx, add).expect("the patch applies").changed,
      ["add"]
   );
   assert_eq!(ctx.contacts().len(), 1);

   // Renamed by the same address in a different case: the patch matches what an address parses to,
   // because the app has always stored contacts in whatever spelling they arrived with.
   let rename = contacts::check(&json!({
      "rename": [{"address": stored.to_uppercase().replace("0X", "0x"), "name": "Alice M."}]
   }))
   .expect("a patch");
   assert_eq!(
      contacts::apply(&ctx, rename).expect("the patch applies").changed,
      ["rename"]
   );
   assert_eq!(ctx.contacts()[0].name, "Alice M.");

   let remove = contacts::check(&json!({"remove": [stored]})).expect("a patch");
   assert_eq!(
      contacts::apply(&ctx, remove).expect("the patch applies").changed,
      ["remove"]
   );
   assert!(ctx.contacts().is_empty());
}

#[test]
fn a_second_contact_at_the_same_address_is_refused() {
   let ctx = context();
   let lower = "0xabcdef0123456789abcdef0123456789abcdef01";
   let upper = "0xABCDEF0123456789ABCDEF0123456789ABCDEF01";

   let first =
      contacts::check(&json!({"add": [{"name": "Alice", "address": lower}]})).expect("a patch");
   contacts::apply(&ctx, first).expect("the patch applies");

   let second = contacts::check(&json!({"add": [{"name": "Alice again", "address": upper}]}))
      .expect("a patch");
   let error = contacts::apply(&ctx, second).expect_err("the address is already filed");

   assert!(
      error.to_string().contains("already exists"),
      "{error}"
   );
   assert_eq!(ctx.contacts().len(), 1);
}

#[test]
fn removing_a_contact_that_is_not_there_is_refused() {
   let ctx = context();
   let patch = contacts::check(&json!({"remove": ["0x2222222222222222222222222222222222222222"]}))
      .expect("a patch");

   let error = contacts::apply(&ctx, patch).expect_err("nothing was removed");

   assert!(
      error.to_string().contains("no contact"),
      "{error}"
   );
}

#[test]
fn every_writable_screen_is_one_the_read_side_knows() {
   for screen in SCREENS {
      let readable = read::screen_entry(screen.name)
         .unwrap_or_else(|| panic!("{} has no read projection", screen.name));

      assert!(
         readable.read.starts_with("settings."),
         "{}",
         screen.name
      );

      let entry = capability(screen.capability)
         .unwrap_or_else(|| panic!("{} is not in the catalog", screen.capability));

      assert_eq!(entry.verb(), CapabilityVerb::Write);
      assert_eq!(entry.scope, CapabilityScope::Global);
   }
}

#[test]
fn no_screen_schema_names_a_secret() {
   for screen in SCREENS {
      let text = (screen.schema)().to_string().to_lowercase();

      for word in ["password", "mnemonic", "seed", "private_key", "secret"] {
         assert!(
            !text.contains(word),
            "{} names {word} in its schema",
            screen.name
         );
      }
   }
}

#[test]
fn a_screen_with_no_writer_says_so_instead_of_offering_a_schema() {
   let described = describe("pools");

   assert_eq!(described["writable"], false);
   assert!(described["schema"].is_null());
   assert!(screen("pools").is_none());
}
