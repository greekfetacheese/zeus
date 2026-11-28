//! The private side: its status, and the three calls that move value across it.
//!
//! Shield and unshield are the two directions of one asset, and merge is neither: it rewrites a wallet's own
//! notes where they are, so it is a wallet write and not an asset move. The broadcast an unshield takes is a
//! per-call choice, and the one part of it the app remembers — the bundler URL — is a stored setting, which
//! is why overriding it needs the grant that may write that setting.

use super::tx::{decimals_argument, wei_amount};
use super::{
   Call, ToolFuture, ToolOutput, ToolSpec, Warning, action, dry_run_argument, exec_error,
   optional_address, priority_argument, read_error, required_address, required_chain,
   settings_error, string_argument, u256_argument, wallet_argument,
};
use crate::error::ToolError;
use crate::protocol::types::Annotations;
use serde_json::{Value, json};
use zeus_core::agent::exec::{
   self, Broadcast, ExecCall, MergeNotesRequest, NftMovement, ShieldRequest, UnshieldRequest,
};
use zeus_core::agent::read;
use zeus_core::agent::settings;
use zeus_core::agent::{Intent, SELF_BROADCAST_NOTICE};
use zeus_eth::alloy_primitives::Address;
use zeus_eth::nft::NftStandard;

pub const STATUS: ToolSpec = ToolSpec {
   name: "railgun_status",
   title: "Private side",
   description: "Whether one wallet's private side is enabled, synced and idle on each Railgun chain, and the zk address it is registered with. A null zk address means the wallet cannot derive one.",
   schema: r#"{"type": "object", "properties": {"wallet": {"type": "string",
      "description": "The wallet's address, as wallet_list shows it."}}, "required": ["wallet"],
      "additionalProperties": false}"#,
   annotations: Annotations::READ,
   capability: Some("wallet.read.private"),
   call: status,
};

pub const SHIELD: ToolSpec = ToolSpec {
   name: "railgun_shield",
   title: "Shield into Railgun",
   description: "Move an asset from a public balance into the private pool, as a zk address of the same wallet or another. The token that moves has to be in that wallet's allowlist. There is no dry run: the shield's transaction is proved and simulated inside the flow.",
   schema: r#"{"type": "object", "properties": {
      "wallet": {"type": "string", "description": "The wallet whose public balance pays."},
      "chain": {"type": "integer", "description": "The chain to shield on."},
      "token": {"type": "string", "description": "The ERC-20 to shield. Omit it for the chain's own coin."},
      "collection": {"type": "string", "description": "Shield one NFT instead: the collection's contract."},
      "token_id": {"type": "string", "description": "The NFT's id, as a whole number written as a string."},
      "standard": {"type": "string", "enum": ["erc721", "erc1155"], "description": "The collection's standard. ERC-1155 shielding is refused by the flow."},
      "decimals": {"type": "integer", "description": "The token's decimals, as wallet_get reports them. Only for a fungible asset."},
      "amount": {"type": "string", "description": "How much to shield, in whole units, as a decimal string such as 1.25. A fungible asset only."},
      "recipient": {"type": "string", "description": "The zk address the private note goes to, as railgun_status shows it for this wallet."},
      "dry_run": {"type": "boolean", "description": "Refused for a shield: its transaction is proved inside the flow, so nothing earlier can be simulated."},
      "priority": {"type": "string", "enum": ["normal", "fast", "very_fast"], "description": "The fee tier; fast by default."},
      "action_id": {"type": "string", "description": "A client-chosen id. Repeating a call with the same id and arguments replays the first answer instead of shielding twice."}
   }, "required": ["wallet", "chain", "recipient"], "additionalProperties": false}"#,
   annotations: Annotations::DESTRUCTIVE,
   capability: Some("wallet.write.shield"),
   call: shield,
};

pub const UNSHIELD: ToolSpec = ToolSpec {
   name: "railgun_unshield",
   title: "Unshield from Railgun",
   description: "Move private notes out to a public address on the same chain. The recipient is screened first. broadcast is per unshield: bundler is the private path, self sends from the wallet itself and breaks anonymity, and the answer says so in the app's own words. Naming a bundler URL the app does not already hold is a change to a stored setting and needs settings.unshield.write; the change is announced and recorded.",
   schema: r#"{"type": "object", "properties": {
      "wallet": {"type": "string", "description": "The wallet the private notes belong to."},
      "chain": {"type": "integer", "description": "The chain to unshield on."},
      "token": {"type": "string", "description": "The ERC-20 to unshield. Required: the private pool holds no native coin, so use WETH for the native equivalent."},
      "collection": {"type": "string", "description": "Unshield one NFT instead: the collection's contract."},
      "token_id": {"type": "string", "description": "The NFT's id, as a whole number written as a string."},
      "standard": {"type": "string", "enum": ["erc721", "erc1155"], "description": "The collection's standard. ERC-1155 unshielding is refused by the flow."},
      "decimals": {"type": "integer", "description": "The token's decimals, as wallet_get reports them. Only for a fungible asset."},
      "amount": {"type": "string", "description": "How much to unshield, in whole units, as a decimal string such as 1.25. A fungible asset only."},
      "to": {"type": "string", "description": "The public recipient, full and 0x-prefixed. A contract address is refused."},
      "broadcast": {"type": "string", "enum": ["bundler", "self"], "description": "Which lane: bundler by default. self submits from your own wallet, which links it to the recipient."},
      "bundler_url": {"type": "string", "description": "Submit through this bundler instead of the stored one. Needs settings.unshield.write, because it changes the setting the app keeps."},
      "memo": {"type": "string", "description": "A note attached to the transfer, kept in the wallet's private history."},
      "unwrap_to_eth": {"type": "boolean", "description": "Unwrap the WETH to ETH on the way out. A fungible choice only."},
      "dry_run": {"type": "boolean", "description": "Refused for an unshield: the proof and the bundler call are the flow's, so there is nothing to simulate before it."},
      "priority": {"type": "string", "enum": ["normal", "fast", "very_fast"], "description": "The fee tier; fast by default."},
      "action_id": {"type": "string", "description": "A client-chosen id. Repeating a call with the same id and arguments replays the first answer instead of unshielding twice."}
   }, "required": ["wallet", "chain", "to"], "additionalProperties": false}"#,
   annotations: Annotations::DESTRUCTIVE,
   capability: Some("wallet.write.unshield"),
   call: unshield,
};

pub const MERGE_NOTES: ToolSpec = ToolSpec {
   name: "railgun_merge_notes",
   title: "Merge private notes",
   description: "Combine a wallet's own notes of one ERC-20 into fewer, larger ones, so later unshields cost less. Nothing leaves the wallet and no recipient is named. It has no dry run: the merge is proved inside the flow.",
   schema: r#"{"type": "object", "properties": {
      "wallet": {"type": "string", "description": "The wallet whose notes are merged."},
      "chain": {"type": "integer", "description": "The chain the notes live on."},
      "token": {"type": "string", "description": "The ERC-20 whose notes are merged. Required: the pool holds no native coin."},
      "decimals": {"type": "integer", "description": "The token's decimals, as wallet_get reports them."},
      "amount": {"type": "string", "description": "How much to consolidate, in whole units, as a decimal string such as 2.5."},
      "dry_run": {"type": "boolean", "description": "Refused for a merge: the transaction is proved inside the flow, so nothing earlier can be simulated."},
      "priority": {"type": "string", "enum": ["normal", "fast", "very_fast"], "description": "The fee tier; fast by default."},
      "action_id": {"type": "string", "description": "A client-chosen id. Repeating a call with the same id and arguments replays the first answer instead of merging twice."}
   }, "required": ["wallet", "chain", "token", "decimals", "amount"], "additionalProperties": false}"#,
   annotations: Annotations::DESTRUCTIVE,
   capability: Some("wallet.write.notes_merge"),
   call: merge_notes,
};

fn status(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let wallet = wallet_argument(&arguments)?;

      let value = call
         .ctx
         .read(|ctx| read::railgun(ctx, &call.profile, wallet))
         .map_err(read_error)?;

      Ok(ToolOutput::new(value))
   })
}

fn shield(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let (token, nft, decimals) = private_asset(&arguments)?;
      let ledger = Ledger {
         chain: required_chain(&arguments)?,
         from: wallet_argument(&arguments)?,
         token,
         nft,
         decimals,
      };
      let wanted = ExecCall::Shield(ShieldRequest {
         chain: ledger.chain,
         from: ledger.from,
         token: ledger.token,
         amount: ledger.amount(&arguments)?,
         decimals: ledger.decimals,
         nft: ledger.nft,
         recipient: zk_recipient(&arguments)?,
         priority: priority_argument(&arguments)?,
      });
      let capability = wanted.capability();

      if dry_run_argument(&arguments)? {
         let value = exec::preview(call.ctx.clone(), &call.profile, &wanted)
            .await
            .map_err(|error| exec_error(error, capability))?;

         return Ok(action::preview(value, &arguments));
      }

      let held = call.clone();

      action::run(call, &arguments, async move {
         Ok(super::tx::perform(&held, "railgun_shield", capability, wanted).await?)
      })
      .await
   })
}

fn unshield(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let (token, nft, decimals) = private_asset(&arguments)?;
      let ledger = Ledger {
         chain: required_chain(&arguments)?,
         from: wallet_argument(&arguments)?,
         token,
         nft,
         decimals,
      };

      if token.is_zero() && nft.is_none() {
         return Err(ToolError::validation(
            "the private pool holds no native coin: name the token (WETH for the native equivalent)",
         ));
      }

      let broadcast = broadcast_argument(&arguments)?;
      let wanted = ExecCall::Unshield(UnshieldRequest {
         chain: ledger.chain,
         from: ledger.from,
         token: ledger.token,
         amount: ledger.amount(&arguments)?,
         decimals: ledger.decimals,
         nft: ledger.nft,
         to: required_address(&arguments, "to")?,
         broadcast: broadcast.clone(),
         unwrap_to_eth: boolean_argument(&arguments, "unwrap_to_eth")?,
         memo: optional_string(&arguments, "memo"),
         priority: priority_argument(&arguments)?,
      });
      let capability = wanted.capability();

      if dry_run_argument(&arguments)? {
         let value = exec::preview(call.ctx.clone(), &call.profile, &wanted)
            .await
            .map_err(|error| exec_error(error, capability))?;

         return Ok(action::preview(value, &arguments));
      }

      // A URL the app does not already hold is a change to the setting it keeps, not a choice for this call.
      // It is written after the dry run has been answered, so a preview leaves the setting alone.
      if exec::overrides_broadcast(flows(call), &broadcast) {
         change_bundler(call, &broadcast).await?;
      }

      let mut output = ToolOutput::new(Value::Null);
      let held = call.clone();

      let answer = action::run(call, &arguments, async move {
         Ok(super::tx::perform(&held, "railgun_unshield", capability, wanted).await?)
      })
      .await?;

      output.data = answer.data;
      output.recorded = answer.recorded;

      // The privacy consequence is not grantable, so it is said: the app's own warning, beside the answer.
      if broadcast.self_broadcast {
         output.warnings.push(Warning {
            code: "ANONYMITY_BROKEN",
            message: SELF_BROADCAST_NOTICE.to_owned(),
            effect: "the transaction is linkable to the wallet that submitted it",
         });
      }

      Ok(output)
   })
}

fn merge_notes(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let wanted = ExecCall::MergeNotes(MergeNotesRequest {
         chain: required_chain(&arguments)?,
         from: wallet_argument(&arguments)?,
         currency: required_address(&arguments, "token")?,
         amount: wei_amount(
            string_argument(&arguments, "amount")?,
            decimals_argument(&arguments, true)?,
         )?,
         decimals: decimals_argument(&arguments, true)?,
         priority: priority_argument(&arguments)?,
      });
      let capability = wanted.capability();

      if dry_run_argument(&arguments)? {
         let value = exec::preview(call.ctx.clone(), &call.profile, &wanted)
            .await
            .map_err(|error| exec_error(error, capability))?;

         return Ok(action::preview(value, &arguments));
      }

      let held = call.clone();

      action::run(call, &arguments, async move {
         Ok(super::tx::perform(&held, "railgun_merge_notes", capability, wanted).await?)
      })
      .await
   })
}

/// The zk address a shield's note goes to.
///
/// The schema requires it and the generic "recipient is required" would leave a model guessing what to
/// put there: for a shield the recipient is the wallet's own zk address, which `railgun_status` answers.
fn zk_recipient(arguments: &Value) -> Result<String, ToolError> {
   let Some(text) = arguments.get("recipient").and_then(Value::as_str) else {
      return Err(ToolError::validation(
         "recipient is required: the wallet's zk address, as railgun_status shows it",
      ));
   };

   let text = text.trim();

   if text.is_empty() {
      return Err(ToolError::validation(
         "recipient is empty: the wallet's zk address, as railgun_status shows it",
      ));
   }

   Ok(text.to_owned())
}

/// What moves, before the amount is read: an id when a collection was named, the token otherwise.
fn private_asset(arguments: &Value) -> Result<(Address, Option<NftMovement>, u8), ToolError> {
   let Some(collection) = optional_address(arguments, "collection")? else {
      let token = optional_address(arguments, "token")?.unwrap_or(Address::ZERO);
      let decimals = decimals_argument(arguments, !token.is_zero())?;

      return Ok((token, None, decimals));
   };

   let standard = match string_argument(arguments, "standard")? {
      "erc721" => NftStandard::Erc721,
      "erc1155" => NftStandard::Erc1155,
      other => {
         return Err(ToolError::validation(format!(
            "standard is erc721 or erc1155; not {other}"
         )));
      }
   };

   let nft = NftMovement {
      token_id: u256_argument(arguments, "token_id")?,
      standard,
   };

   Ok((collection, Some(nft), 0))
}

/// The asset a private call names, and the amount it asks to move.
struct Ledger {
   chain: u64,
   from: Address,
   token: Address,
   nft: Option<NftMovement>,
   decimals: u8,
}

impl Ledger {
   /// The amount, in the units the asset moves in: an ERC-721 moves exactly one, whatever was typed.
   fn amount(&self, arguments: &Value) -> Result<zeus_eth::alloy_primitives::U256, ToolError> {
      if matches!(
         self.nft,
         Some(NftMovement {
            standard: NftStandard::Erc721,
            ..
         })
      ) {
         return Ok(zeus_eth::alloy_primitives::U256::from(1));
      }

      let text = match arguments.get("amount") {
         Some(value) => value
            .as_str()
            .ok_or_else(|| ToolError::validation("amount is a decimal string"))?,
         None if self.nft.is_some() => "1",
         None => return Err(ToolError::validation("amount is required")),
      };

      wei_amount(text, self.decimals)
   }
}

/// The broadcast an unshield takes: the lane, and the bundler it would rather use.
fn broadcast_argument(arguments: &Value) -> Result<Broadcast, ToolError> {
   let self_broadcast = match arguments.get("broadcast").map(Value::as_str) {
      None | Some(None) | Some(Some("bundler")) => false,
      Some(Some("self")) => true,
      Some(Some(other)) => {
         return Err(ToolError::validation(format!(
            "broadcast is bundler or self; not {other}"
         )));
      }
   };

   Ok(Broadcast {
      self_broadcast,
      bundler_url: optional_string(arguments, "bundler_url")
         .map(|url| url.trim().to_owned())
         .filter(|url| !url.is_empty()),
   })
}

/// A stored setting behind a call: changing the bundler needs the grant that may write it.
async fn change_bundler(call: &Call, broadcast: &Broadcast) -> Result<(), ToolError> {
   let Some(url) = &broadcast.bundler_url else {
      return Ok(());
   };

   let title = "Let the agent change the unshield bundler URL?".to_owned();

   super::gate_write(
      call,
      "railgun_unshield",
      &Intent::GlobalWrite {
         capability: "settings.unshield.write",
      },
      &title,
   )
   .await?;

   let screen = settings::screen("unshield").ok_or_else(|| {
      ToolError::new(
         crate::error::ErrorCode::Unsupported,
         "this build cannot write the unshield screen",
      )
   })?;

   let patch = json!({ "bundler_url": url });
   let ctx = call.ctx.clone();

   // The store is a file, and none of them is written on the runtime the request is answered on.
   tokio::task::spawn_blocking(move || (screen.write)(&ctx, &patch))
      .await
      .map_err(|error| ToolError::internal(format!("the settings write did not run: {error}")))?
      .map_err(settings_error)?;

   super::record_allowed(
      call,
      "railgun_unshield",
      "settings.unshield.write",
   );

   let host = zeus_core::platform::url_host(url).unwrap_or("the configured bundler");
   zeus_core::ui::announce(&format!(
      "Agent changed where unshields are submitted: {host}"
   ));

   Ok(())
}

/// The port's fund-moving half, for the one question that needs it.
fn flows(call: &Call) -> &dyn zeus_core::agent::exec::TxFlows {
   call.ops.as_ref()
}

/// A boolean argument with a false default.
fn boolean_argument(arguments: &Value, key: &str) -> Result<bool, ToolError> {
   match arguments.get(key) {
      None | Some(Value::Null) => Ok(false),
      Some(value) => value
         .as_bool()
         .ok_or_else(|| ToolError::validation(format!("{key} is true or false"))),
   }
}

/// A string argument a call may leave out.
fn optional_string(arguments: &Value, key: &str) -> Option<String> {
   arguments.get(key).and_then(Value::as_str).map(str::to_owned)
}
