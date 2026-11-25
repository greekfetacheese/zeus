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

#[test]
fn a_railgun_patch_sets_the_knobs_and_the_chain_it_names() {
   let ctx = context();

   let patch = railgun::check(&json!({
      "syncer_concurrency": 4,
      "state_update_interval_minutes": 15,
      "allow_circuit_download": true,
      "chains": [{"chain": 1, "enabled": true, "block_range": 500}],
   }))
   .expect("a patch");

   assert_eq!(
      railgun::apply(&ctx, patch).expect("the patch applies").changed,
      [
         "syncer_concurrency",
         "state_update_interval_minutes",
         "allow_circuit_download",
         "chains"
      ]
   );

   let config = ctx.read(|ctx| ctx.railgun_config.clone());

   assert_eq!(config.rpc_syncer_concurrency(), 4);
   assert_eq!(config.state_update_interval_secs(), 15 * 60);
   assert!(config.allow_circuit_download());
   assert!(config.is_enabled(1));
   assert_eq!(config.rpc_syncer_block_range(1), 500);
}

#[test]
fn a_railgun_number_outside_the_range_the_slider_allows_is_refused() {
   for (patch, expected) in [
      (
         json!({"syncer_concurrency": 11}),
         "between 1 and 10",
      ),
      (
         json!({"state_update_interval_minutes": 0}),
         "between 1 and 60",
      ),
      (
         json!({"chains": [{"chain": 1, "block_range": 50}]}),
         "between 100 and 30000",
      ),
   ] {
      let error = railgun::check(&patch).expect_err("out of range");

      assert!(error.to_string().contains(expected), "{error}");
   }
}

#[test]
fn a_railgun_chain_it_cannot_run_on_is_refused_before_anything_lands() {
   let ctx = context();
   let before = ctx.read(|ctx| ctx.railgun_config.rpc_syncer_concurrency());

   let patch = railgun::check(&json!({
      "syncer_concurrency": 4,
      "chains": [{"chain": 8453, "enabled": true}],
   }))
   .expect("a patch");

   let error = railgun::apply(&ctx, patch).expect_err("Railgun does not run on Base");

   assert!(
      error.to_string().contains("Ethereum and Ethereum Sepolia"),
      "{error}"
   );
   assert_eq!(
      ctx.read(|ctx| ctx.railgun_config.rpc_syncer_concurrency()),
      before,
      "a refused patch changes nothing at all"
   );
   assert!(!ctx.read(|ctx| ctx.railgun_config.is_enabled(8453)));
}

#[test]
fn a_railgun_chain_entry_has_to_say_something_and_say_it_once() {
   let empty = railgun::check(&json!({"chains": [{"chain": 1}]})).expect_err("no field is set");
   assert!(empty.to_string().contains("neither"), "{empty}");

   let twice = railgun::check(&json!({
      "chains": [{"chain": 1, "enabled": true}, {"chain": 1, "enabled": false}],
   }))
   .expect_err("two entries for one chain");
   assert!(twice.to_string().contains("twice"), "{twice}");

   let unknown =
      railgun::check(&json!({"chains": [{"chain": 999, "enabled": true}]})).expect("a patch");

   assert_eq!(
      railgun::apply(&context(), unknown),
      Err(SettingsError::Unsupported { chain: 999 })
   );
}

#[test]
fn the_bundler_url_belongs_to_the_unshield_screen() {
   let error = railgun::check(&json!({"bundler_url": "https://bundler.example"}))
      .expect_err("the syncer screen does not own it");

   assert!(
      matches!(
         error,
         SettingsError::Closed {
            field: "bundler_url",
            ..
         }
      ),
      "{error}"
   );
   assert!(error.to_string().contains("unshield"), "{error}");
}

#[test]
fn an_unshield_patch_writes_the_bundler_url_and_nothing_else() {
   let ctx = context();
   let patch = unshield::check(&json!({
      "bundler_url": "https://bundler.example/v1/1/rpc?apikey=k"
   }))
   .expect("a patch");

   assert_eq!(
      unshield::apply(&ctx, patch).expect("the patch applies").changed,
      ["bundler_url"]
   );

   assert_eq!(
      unshield::apply(
         &ctx,
         unshield::check(&json!({})).expect("an empty patch")
      )
      .expect("the patch applies"),
      Applied::none()
   );
}

#[test]
fn a_bundler_url_that_is_not_one_is_refused() {
   for (value, expected) in [
      (json!({"bundler_url": ""}), "cannot be empty"),
      (
         json!({"bundler_url": "bundler.example"}),
         "http:// or https://",
      ),
      (
         json!({"bundler_url": "https://a b"}),
         "whitespace",
      ),
      (
         json!({"bundler_url": "https://"}),
         "names no host",
      ),
   ] {
      let error = unshield::check(&value).expect_err("not a URL");

      assert!(error.to_string().contains(expected), "{error}");
   }
}

#[test]
fn the_unshield_broadcast_choices_are_per_call_and_say_so() {
   for field in ["self_broadcast", "memo", "unwrap_to_eth"] {
      let error = unshield::check(&json!({field: true})).expect_err("runtime state");

      assert_eq!(
         error,
         SettingsError::RuntimeOnly {
            field: field.to_owned(),
         }
      );
      assert!(error.to_string().contains("per call"));
   }
}

#[test]
fn the_two_new_schemas_name_what_they_take_and_nothing_more() {
   let railgun = railgun::schema();

   for field in [
      "syncer_concurrency",
      "state_update_interval_minutes",
      "allow_circuit_download",
      "chains",
   ] {
      assert!(
         !railgun["properties"][field].is_null(),
         "railgun names {field}"
      );
   }
   assert!(railgun["properties"]["bundler_url"].is_null());

   let unshield = unshield::schema();

   assert_eq!(
      unshield["properties"]
         .as_object()
         .expect("properties")
         .keys()
         .collect::<Vec<_>>(),
      ["bundler_url"]
   );
}

#[test]
fn a_general_patch_records_the_application_menu_choice() {
   let ctx = context();

   let patch =
      general::check(&json!({"desktop_integration": true})).expect("the flag is a field again");
   let applied = general::apply(&ctx, patch).expect("the choice");

   assert_eq!(applied.changed, ["desktop_integration"]);
   assert!(
      ctx.read(|ctx| ctx.misc_config.desktop_integration()),
      "the flag the port's installer runs before is the flag this patch writes"
   );

   // The one switch that stays out of reach is the endpoint's own.
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
}
