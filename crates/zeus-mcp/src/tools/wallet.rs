//! The wallets a grant gives this profile, what may be done with one, and the two ways a wallet arrives.
//!
//! A wallet the grant does not name does not exist as far as an agent can tell: it is not listed, it is
//! not addressable, and no tool will mention it. That is the point of granting per wallet rather than per
//! profile — a profile that may read one address cannot enumerate the others.
//!
//! Deriving and importing both *add* a wallet, and neither grants it: a derived wallet has to be granted
//! like any other, and an imported one is typed by the owner into Zeus's own field, so its key never
//! reaches an argument, a log or this conversation.

use super::{
   Call, ToolFuture, ToolOutput, ToolSpec, Warning, chain_argument, decide, gate_write,
   integer_argument, read_error, record_allowed, refusal, wallet_argument,
};
use crate::error::{ErrorCode, ToolError};
use crate::ops::Handback;
use crate::protocol::types::Annotations;
use serde_json::{Value, json};
use zeus_core::agent::{Decision, Intent, read};
use zeus_core::ui;
use zeus_eth::alloy_primitives::Address;

/// The switch deriving a wallet needs: the screen's own write capability, from the catalog.
const DERIVE_CAPABILITY: &str = "settings.wallets.derive";

/// The switch opening the import form needs.
const IMPORT_CAPABILITY: &str = "settings.wallets.import";

/// How many children one call may derive.
///
/// A per-call bound, not a budget: it keeps one call's Argon2 save from standing in for a hundred
/// derivations. There is no daily cap, because the grant document has none (D25).
const MAX_DERIVED_PER_CALL: u64 = 5;

pub const LIST: ToolSpec = ToolSpec {
   name: "wallet_list",
   title: "Wallets",
   description: "Every wallet this profile was granted, with its address, its name and the zk \
      address of its private side. A wallet that was not granted does not appear at all.",
   schema: r#"{"type": "object", "additionalProperties": false}"#,
   annotations: Annotations::READ,
   capability: Some("settings.wallets.read"),
   call: list,
};

pub const GET: ToolSpec = ToolSpec {
   name: "wallet_get",
   title: "One wallet",
   description: "One granted wallet, plus the capabilities this profile holds on it — so a caller \
      can tell what it may do with this address without trying.",
   schema: r#"{"type": "object", "properties": {"wallet": {"type": "string",
      "description": "The wallet's address, as wallet_list shows it."}}, "required": ["wallet"],
      "additionalProperties": false}"#,
   annotations: Annotations::READ,
   capability: Some("settings.wallets.read"),
   call: get,
};

pub const DELEGATE: ToolSpec = ToolSpec {
   name: "delegate_status",
   title: "Delegation",
   description: "Whether one wallet is EIP-7702 delegated on a chain, and to which contract. The \
      answer is read from the chain when the cached one is stale; a chain with no node answers an \
      error rather than a guess.",
   schema: r#"{"type": "object", "properties": {"wallet": {"type": "string",
      "description": "The wallet's address, as wallet_list shows it."}, "chain": {"type": "integer",
      "description": "The chain to ask about; the chain selected in Zeus otherwise."}},
      "required": ["wallet"], "additionalProperties": false}"#,
   annotations: Annotations::READ,
   capability: Some("settings.wallets.read"),
   call: delegate,
};

pub const DERIVE: ToolSpec = ToolSpec {
   name: "wallets_derive",
   title: "Derive a wallet",
   description: "Derive up to five child wallets from the master wallet and add them to the vault. \
      The new wallets are not granted to this profile — deriving is 'add a wallet', never 'restore a \
      permission' — so they appear in this answer and not in wallet_list until the grant names them. \
      Never a key: a derived wallet's key stays in the vault.",
   schema: r#"{"type": "object", "properties": {"count": {"type": "integer", "minimum": 1,
      "maximum": 5, "description": "How many to derive; one when left out."}, "name":
      {"type": "string", "description": "The name to give it. Only with count 1: a name names one wallet."}}, "additionalProperties": false}"#,
   annotations: Annotations::DESTRUCTIVE,
   capability: Some(DERIVE_CAPABILITY),
   call: derive,
};

pub const IMPORT: ToolSpec = ToolSpec {
   name: "wallets_import",
   title: "Import a wallet",
   description: "Open the wallet import form in Zeus. The key or the seed phrase is typed there, into \
      Zeus's own secure field: it is never an argument of any tool and never enters this conversation. \
      The call opens the form and answers that the user has to finish it.",
   schema: r#"{"type": "object", "properties": {"kind": {"type": "string", "enum":
      ["private_key", "mnemonic"], "description": "Which form to open: a single key, or a seed phrase. A private key form opens when it is left out."}}, "additionalProperties": false}"#,
   annotations: Annotations::DESTRUCTIVE,
   capability: Some(IMPORT_CAPABILITY),
   call: import,
};

fn list(call: &Call, _arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move { Ok(ToolOutput::new(json!({"wallets": rows(call)}))) })
}

fn get(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let wallet = wallet_argument(&arguments)?;

      let value = call
         .ctx
         .read(|ctx| read::wallet(ctx, &call.profile, wallet))
         .map_err(read_error)?;

      Ok(ToolOutput::new(value))
   })
}

fn delegate(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let wallet = wallet_argument(&arguments)?;

      // An address the grant does not name is refused before a node is asked about it.
      if !read::wallet_granted(&call.profile, wallet) {
         return Err(read_error(read::unknown_wallet(wallet)));
      }

      let chain = chain_argument(&arguments)?.unwrap_or_else(|| call.ctx.chain().id());

      // The cached answer is what the account panel shows; it is re-read when stale, and a node that
      // cannot answer is reported rather than turned into "not delegated".
      if call.ctx.should_check_delegated_wallet_status(chain, wallet) {
         call.ctx.check_delegated_wallet_status(chain, wallet).await.map_err(|error| {
            ToolError::new(
               ErrorCode::Network,
               format!("could not read the account on chain {chain}: {error}"),
            )
         })?;
      }

      let delegated = call.ctx.read(|ctx| ctx.delegated_wallets.get(chain, wallet));

      let value = call
         .ctx
         .read(|ctx| read::delegate(ctx, &call.profile, wallet, chain, delegated))
         .map_err(read_error)?;

      Ok(ToolOutput::new(value))
   })
}

fn derive(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let count = integer_argument(&arguments, "count", 1)?;

      if !(1..=MAX_DERIVED_PER_CALL).contains(&count) {
         return Err(ToolError::validation(format!(
            "count must be between 1 and {MAX_DERIVED_PER_CALL}"
         )));
      }

      let name = arguments
         .get("name")
         .and_then(Value::as_str)
         .map(str::trim)
         .filter(|text| !text.is_empty())
         .map(str::to_owned);

      if name.is_some() && count > 1 {
         return Err(ToolError::validation(
            "name names one wallet: derive them one at a time to name them",
         ));
      }

      // The vault refuses a duplicate name itself, but it does so after the derivation work is set up,
      // and a name the app already holds is the caller's mistake rather than a store failure.
      if let Some(name) = &name
         && call.ctx.get_all_wallets_info().iter().any(|wallet| wallet.name() == *name)
      {
         return Err(ToolError::validation(format!(
            "a wallet named {name} already exists"
         )));
      }

      gate_write(
         call,
         "wallets_derive",
         &Intent::GlobalWrite {
            capability: DERIVE_CAPABILITY,
         },
         "Let the agent derive a new wallet?",
      )
      .await?;

      let ctx = call.ctx.clone();

      let addresses = tokio::task::spawn_blocking(move || derive_wallets(&ctx, name, count))
         .await
         .map_err(|error| ToolError::internal(format!("the wallet was not derived: {error}")))?
         .map_err(|error| {
            ToolError::internal(format!("the wallet was not derived: {error}"))
               .with_hint("Nothing was added to the vault.")
         })?;

      record_allowed(call, "wallets_derive", DERIVE_CAPABILITY);

      ui::announce(&match addresses.len() {
         1 => "Agent derived a new wallet".to_owned(),
         count => format!("Agent derived {count} new wallets"),
      });

      let infos = call.ctx.get_all_wallets_info();

      let rows: Vec<Value> = infos
         .iter()
         .filter(|wallet| addresses.contains(&wallet.address))
         .map(read::wallet_row)
         .collect();

      let mut output = ToolOutput::new(json!({"count": rows.len(), "wallets": rows}));

      // The one thing a caller is likely to get wrong next: a new wallet is not a granted one.
      output.warnings.push(Warning {
         code: "WALLET_NOT_GRANTED",
         message: "the wallets this call derived are not in this profile's grant".to_owned(),
         effect: "they do not appear in wallet_list, and no tool can act on them until the app owner                   grants them",
      });

      Ok(output)
   })
}

fn import(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let mnemonic = match arguments.get("kind").and_then(Value::as_str) {
         None | Some("private_key") => false,
         Some("mnemonic") => true,
         Some(other) => {
            return Err(ToolError::validation(format!(
               "kind {other} is not one of private_key, mnemonic"
            )));
         }
      };

      let intent = Intent::NeedsHuman {
         capability: IMPORT_CAPABILITY,
      };

      match decide(call, "wallets_import", &intent) {
         Decision::NeedsUser { reason } => {
            call.ops.open_handback(Handback::WalletImport { mnemonic }).map_err(|error| {
               ToolError::internal(format!(
                  "the wallet import form could not be opened: {error}"
               ))
               .with_hint("Nothing was started.")
            })?;

            Err(ToolError::new(ErrorCode::UserActionRequired, reason).with_hint(
               "Zeus is showing the import form: the key or phrase goes there, never here. Call                 wallet_list when the user is done.",
            ))
         }
         decision => {
            refusal(&intent, decision)?;

            // A NeedsHuman intent the grant covers answers NeedsUser; anything else is a denial, which
            // the line above has already returned.
            Err(ToolError::internal(
               "the import was allowed without the human step it needs",
            ))
         }
      }
   })
}

/// Derive `count` children, saving the vault once for the batch.
///
/// The derivation happens on a copy: a failure halfway leaves the app's vault exactly as it was, because
/// the copy is only committed after every child was derived and the sealed file was written.
fn derive_wallets(
   ctx: &zeus_core::ZeusCtx,
   name: Option<String>,
   count: u64,
) -> Result<Vec<Address>, String> {
   let mut vault = ctx.get_vault();
   let mut addresses = Vec::with_capacity(count as usize);

   for _ in 0..count {
      let address = vault
         .derive_child_wallet(name.clone().unwrap_or_default())
         .map_err(|error| error.to_string())?;

      addresses.push(address);
   }

   // One seal for the batch: the vault write runs Argon2, so a save per wallet would make five children
   // cost five derivations of the same key.
   ctx.encrypt_and_save_vault(Some(vault.clone()), None)
      .map_err(|error| error.to_string())?;
   ctx.set_vault(vault);
   ctx.build_wallet_info_cache();

   Ok(addresses)
}

/// The granted wallets, as both this tool and the zeus://wallets resource answer.
pub fn rows(call: &Call) -> Vec<Value> {
   call.ctx.read(|ctx| read::wallets(ctx, &call.profile))
}
