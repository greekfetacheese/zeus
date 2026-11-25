//! The read projections: one shape per object, decided once.
//!
//! The edge must not invent its own idea of what a balance or a portfolio is — a second shape would drift
//! from the GUI's and from the next slice's. Every read tool therefore calls a function here, which takes
//! the context and the grant and answers the JSON an agent sees. The functions never lock the context
//! themselves: the caller already holds it, which is also what lets the shapes be tested without HTTP.
//!
//! A wallet the grant does not name is **not found**, not "not granted": whether it exists is itself part
//! of what the grant decides. A wallet it does name, with too narrow a grant, is [`ReadError::NotGranted`]
//! and names the switch — the same rule the policy follows, so a refusal reads the same however it
//! arrived.

use super::profile::{AgentProfile, WalletGrant};
use super::settings;
use crate::context::{WalletPortfolio, ZeusContext};
use crate::platform::{truncate_address, url_host};
use crate::tx::events::{NftApproveParams, PermitParams, TokenApproveParams};
use crate::types::BundlerUrl;
use crate::{DecodedEvent, TransactionRich, WalletInfo};
use serde_json::{Value, json};
use zeus_eth::alloy_primitives::{Address, TxHash};
use zeus_eth::amm::uniswap::{AnyUniswapPool, UniswapPool};
use zeus_eth::currency::{Currency, ERC20Token};
use zeus_eth::nft::{NftStandard, NftToken};
use zeus_eth::types::ChainId;
use zeus_eth::utils::NumericValue;

/// Why a projection could not answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadError {
   /// The profile does not hold the capability, on this wallet or on the app.
   NotGranted { capability: &'static str },
   /// No such object, or one outside the grant. The two are the same answer on purpose.
   NotFound { what: &'static str, id: String },
}

/// One settings screen a read tool can answer for.
pub struct Screen {
   /// The name a tool takes, and the segment a resource URI carries.
   pub name: &'static str,
   /// The read capability that reveals it.
   pub read: &'static str,
}

/// The screens this build can read, in the order the settings page shows them.
///
/// Only the screens whose values are projected here appear. The three bin-resident screens (`keyboard`,
/// `appearance`, `bridge`) are typed in the binary, so their values come through the port instead; a
/// screen listed here is a promise that `screen` answers for it.
pub const SCREENS: &[Screen] = &[
   Screen {
      name: "general",
      read: "settings.general.read",
   },
   Screen {
      name: "security",
      read: "settings.security.read",
   },
   Screen {
      name: "networks",
      read: "settings.networks.read",
   },
   Screen {
      name: "contacts",
      read: "settings.contacts.read",
   },
   Screen {
      name: "railgun",
      read: "settings.railgun.read",
   },
   Screen {
      name: "unshield",
      read: "settings.unshield.read",
   },
   Screen {
      name: "pools",
      read: "settings.pools.read",
   },
   Screen {
      name: "dapps",
      read: "settings.dapps.read",
   },
   Screen {
      name: "wallets",
      read: "settings.wallets.read",
   },
];

/// The screen with that name, if this build can read it.
pub fn screen_entry(name: &str) -> Option<&'static Screen> {
   SCREENS.iter().find(|screen| screen.name == name)
}

/// The read capability one screen needs, for a caller that has to decide before it projects.
pub fn screen_read(name: &str) -> Option<&'static str> {
   screen_entry(name).map(|screen| screen.read)
}

/// Every granted wallet, as `wallet_list` and `zeus://wallets` answer it.
pub fn wallets(ctx: &ZeusContext, profile: &AgentProfile) -> Vec<Value> {
   ctx.wallet_info_cache
      .ordered_vec
      .iter()
      .filter(|wallet| wallet_grant(profile, wallet.address).is_ok())
      .map(wallet_row)
      .collect()
}

/// One wallet plus the capabilities the grant gives it, so an agent knows what it may do *with this
/// wallet* rather than only that it exists.
pub fn wallet(
   ctx: &ZeusContext,
   profile: &AgentProfile,
   address: Address,
) -> Result<Value, ReadError> {
   let wallet = find_wallet(ctx, address)?;
   let grant = wallet_grant(profile, address)?;

   let capabilities: Vec<&str> = grant.caps.iter().map(String::as_str).collect();

   let mut row = wallet_row(wallet);
   row["capabilities"] = json!(capabilities);

   Ok(row)
}

/// One wallet's balances and USD value, public and — when granted — private.
pub fn portfolio(
   ctx: &ZeusContext,
   profile: &AgentProfile,
   owner: Address,
   chain: Option<u64>,
) -> Result<Value, ReadError> {
   let grant = wallet_grant(profile, owner)?;

   if !grant.caps.contains("wallet.read.portfolio") {
      return Err(ReadError::NotGranted {
         capability: "wallet.read.portfolio",
      });
   }

   let private = grant.caps.contains("wallet.read.private");

   let chains: Vec<u64> = match chain {
      Some(chain) => vec![chain],
      None => ctx.read_wallet_state(|state| {
         let mut chains: Vec<u64> = state
            .portfolio_db
            .portfolios
            .keys()
            .filter(|(_, holder)| *holder == owner)
            .map(|(chain, _)| *chain)
            .collect();
         chains.sort_unstable();
         chains.dedup();
         chains
      }),
   };

   let portfolios: Vec<Value> = chains
      .into_iter()
      .map(|chain| ctx.read_wallet_state(|state| state.portfolio_db.get(chain, owner)))
      .map(|portfolio| portfolio_row(&portfolio, private))
      .collect();

   Ok(json!({"wallet": hex(owner), "portfolios": portfolios}))
}

/// One wallet's recorded transactions, newest first, paged.
pub fn history(
   ctx: &ZeusContext,
   profile: &AgentProfile,
   owner: Address,
   chain: Option<u64>,
   page: usize,
   per_page: usize,
) -> Result<Value, ReadError> {
   wallet_capability(profile, owner, "wallet.read.history")?;

   let per_page = per_page.clamp(1, 100);
   let db = &ctx.tx_db;

   let (total, rows) = match chain {
      Some(chain) => {
         let total = db.get_tx_count(chain, owner);
         let rows = db.get_txs_paged(chain, owner, page, per_page).unwrap_or_default();
         (total, rows)
      }
      None => {
         let mut all: Vec<TransactionRich> = ChainId::supported_chains()
            .into_iter()
            .filter_map(|chain| db.get_txs(chain.id(), owner))
            .flatten()
            .collect();

         all.sort_by(|a, b| b.block.cmp(&a.block).then_with(|| b.timestamp.cmp(&a.timestamp)));

         let total = all.len();
         let rows = all.into_iter().skip(page.saturating_mul(per_page)).take(per_page).collect();

         (total, rows)
      }
   };

   let transactions: Vec<Value> =
      rows.iter().map(|transaction| transaction_row(ctx, transaction)).collect();

   Ok(json!({
       "wallet": hex(owner),
       "page": page,
       "per_page": per_page,
       "total": total,
       "transactions": transactions,
   }))
}

/// One transaction the wallet recorded, by hash.
pub fn transaction(
   ctx: &ZeusContext,
   profile: &AgentProfile,
   owner: Address,
   hash: TxHash,
) -> Result<Value, ReadError> {
   wallet_capability(profile, owner, "wallet.read.history")?;

   for chain in ChainId::supported_chains() {
      let Some(transactions) = ctx.tx_db.get_txs(chain.id(), owner) else {
         continue;
      };

      if let Some(transaction) = transactions.iter().find(|transaction| transaction.hash == hash) {
         return Ok(transaction_row(ctx, transaction));
      }
   }

   Err(ReadError::NotFound {
      what: "transaction",
      id: hex_hash(hash),
   })
}

/// One wallet's live ERC-20, Permit2 and NFT approvals.
pub fn approvals(
   ctx: &ZeusContext,
   profile: &AgentProfile,
   owner: Address,
   chain: Option<u64>,
) -> Result<Value, ReadError> {
   wallet_capability(profile, owner, "wallet.read.approvals")?;

   let manager = ctx.read_wallet_state(|state| state.approval_manager.clone());
   let mut tokens = Vec::new();
   let mut permits = Vec::new();
   let mut nfts = Vec::new();

   for chain in chains_or_all(chain) {
      for approval in manager.get_token_approvals(chain, owner) {
         tokens.push(token_approval_row(&approval));
      }
      for permit in manager.get_active_permits(chain, owner) {
         permits.push(permit_row(&permit));
      }
      for approval in manager.get_nft_approvals(chain, owner) {
         nfts.push(nft_approval_row(&approval));
      }
   }

   Ok(json!({
       "wallet": hex(owner),
       "tokens": tokens,
       "permits": permits,
       "nfts": nfts,
   }))
}

/// The wallet's private side, from the status the app keeps for every Railgun chain.
pub fn railgun(
   ctx: &ZeusContext,
   profile: &AgentProfile,
   owner: Address,
) -> Result<Value, ReadError> {
   wallet_capability(profile, owner, "wallet.read.private")?;

   let wallet = find_wallet(ctx, owner)?;
   let status = &ctx.railgun_status;

   let chains: Vec<Value> = ChainId::supported_chains()
      .iter()
      .map(|chain| {
         let id = chain.id();

         json!({
             "chain": id,
             "name": chain.name(),
             "enabled": ctx.railgun_config.is_enabled(id),
             "synced": status.synced(id),
             "synced_block": status.synced_block(id),
             "syncing": status.sync_in_progress(id),
             "resyncing": status.resync_in_progress(id),
             "operation_in_progress": status.op_in_progress(id),
             "error": status.sync_error(id),
         })
      })
      .collect();

   Ok(json!({
       "wallet": hex(owner),
       "zk_address": wallet.railgun_address.as_ref().map(ToString::to_string),
       "registered": wallet.railgun_address.is_some(),
       "chains": chains,
   }))
}

/// Whether one wallet is EIP-7702 delegated on one chain.
///
/// The caller resolves the address first — the app's own check reads the account's code and caches the
/// answer — so this projection only says what was found.
pub fn delegate(
   ctx: &ZeusContext,
   profile: &AgentProfile,
   owner: Address,
   chain: u64,
   delegated: Option<Address>,
) -> Result<Value, ReadError> {
   wallet_grant(profile, owner)?;

   Ok(json!({
       "wallet": hex(owner),
       "chain": chain,
       "delegated": delegated.is_some(),
       "delegate": delegated.map(hex),
       "display": delegated.map(|address| address_row(ctx, chain, address)),
   }))
}

/// The screens, each with the switches the grant holds for it and whether this build can write it.
///
/// A screen with no writer still appears — its values are readable — and says so, because "there is no
/// patch here" is the answer that stops a caller trying to invent one.
pub fn screens(profile: &AgentProfile) -> Value {
   let screens: Vec<Value> = SCREENS
      .iter()
      .map(|screen| {
         let writable = settings::screen(screen.name);

         screen_row(
            screen.name,
            profile.screen(screen.read).is_some_and(|grant| grant.read),
            writable.is_some_and(|settings| profile.allows_global(settings.capability)),
            writable.is_some(),
         )
      })
      .collect();

   json!({"screens": screens})
}

/// One row of `settings_list`.
///
/// Built here rather than in the edge because the binary's own screens are listed beside these, and two
/// shapes for one list is exactly the drift this module exists to prevent.
pub fn screen_row(name: &str, read: bool, write: bool, writable: bool) -> Value {
   json!({
       "screen": name,
       "read": read,
       "write": write,
       "writable": writable,
   })
}

/// One screen's current values, in the same shape its write takes.
pub fn screen(ctx: &ZeusContext, profile: &AgentProfile, name: &str) -> Result<Value, ReadError> {
   let screen = screen_entry(name).ok_or_else(|| ReadError::NotFound {
      what: "screen",
      id: name.to_owned(),
   })?;

   if !profile.allows_global(screen.read) {
      return Err(ReadError::NotGranted {
         capability: screen.read,
      });
   }

   let values = match screen.name {
      "general" => general(ctx),
      "security" => security(ctx),
      "networks" => networks(ctx),
      "contacts" => json!({"contacts": contacts(ctx)}),
      "railgun" => railgun_settings(ctx),
      "unshield" => unshield_settings(ctx),
      "pools" => pools(ctx, None),
      "dapps" => dapps(ctx),
      "wallets" => json!({"wallets": wallets(ctx, profile)}),
      other => {
         return Err(ReadError::NotFound {
            what: "screen",
            id: other.to_owned(),
         });
      }
   };

   Ok(json!({"screen": screen.name, "values": values}))
}

/// The user's address book.
///
/// Only the contacts the user stored: the wallet and well-known names are not rows of their own here —
/// they ride on the wallet and address objects a tool already returns.
pub fn contacts(ctx: &ZeusContext) -> Vec<Value> {
   let mut contacts: Vec<Value> = ctx.read_wallet_state(|state| {
      state
         .contacts
         .iter()
         .map(|contact| {
            json!({
                "name": {"value": contact.name, "source": "contact", "untrusted": true},
                "address": contact.evm_address.to_lowercase(),
                "zk_address": contact.zk_address,
                "display": {"zk_address": contact.zk_address_truncated()},
            })
         })
         .collect()
   });

   contacts.sort_by(|a, b| a["name"]["value"].as_str().cmp(&b["name"]["value"].as_str()));

   contacts
}

/// The chains, their endpoints and their health.
pub fn networks(ctx: &ZeusContext) -> Value {
   let manager = &ctx.client_manager;

   let chains: Vec<Value> = ChainId::supported_chains()
      .iter()
      .map(|chain| {
         let id = chain.id();
         let mut rpcs: Vec<Value> = manager
            .get_rpcs(id)
            .into_iter()
            .map(|(url, rpc)| {
               json!({
                   "url": url.as_ref(),
                   "default": rpc.default,
                   "enabled": rpc.enabled,
                   "working": rpc.check.working,
                   "archive": rpc.check.archive,
                   "mev_protect": rpc.mev_protect,
                   "latency_ms": rpc.latency_ms(),
               })
            })
            .collect();

         rpcs.sort_by(|a, b| a["url"].as_str().cmp(&b["url"].as_str()));

         json!({
             "chain": id,
             "name": chain.name(),
             "symbol": chain.coin_symbol(),
             "disabled": ctx.disabled_chains.is_disabled(id),
             "rpc_available": manager.rpc_available(id),
             "rpcs": rpcs,
         })
      })
      .collect();

   json!({"chains": chains, "concurrency": manager.concurrency()})
}

/// The tracked Uniswap pools, on one chain or on every chain that has any.
pub fn pools(ctx: &ZeusContext, chain: Option<u64>) -> Value {
   let mut rows: Vec<Value> = Vec::new();

   for chain in chains_or_all(chain) {
      for pool in ctx.pool_manager.get_pools_for_chain(chain) {
         rows.push(pool_row(&pool));
      }
   }

   rows.sort_by(|a, b| {
      a["chain"]
         .as_u64()
         .cmp(&b["chain"].as_u64())
         .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
   });

   json!({"pools": rows})
}

/// The origins connected to the wallet connector, and the account each was given.
pub fn dapps(ctx: &ZeusContext) -> Value {
   let mut origins: Vec<Value> = ctx
      .connected_dapps
      .dapps
      .iter()
      .map(|origin| {
         let account = ctx.read_wallet_state(|state| state.dapp_accounts.get(origin));

         json!({
             "display": {"origin": origin, "untrusted": true},
             "account": account.map(hex),
         })
      })
      .collect();

   origins.sort_by(|a, b| a["display"]["origin"].as_str().cmp(&b["display"]["origin"].as_str()));

   json!({"origins": origins})
}

/// The settings screen `general` reads: the flags that survive a restart.
///
/// The agent endpoint's own switch is deliberately absent: starting or stopping the server is the user's
/// in both directions, so it is not part of a screen an agent can read or write.
fn general(ctx: &ZeusContext) -> Value {
   let config = &ctx.misc_config;

   json!({
       "fetch_asset_images": config.fetch_asset_images(),
       "fetch_contract_names": config.fetch_contract_names(),
       "check_for_updates": config.check_for_updates(),
       "minimize_to_tray": config.minimize_to_tray(),
       "desktop_integration": config.desktop_integration(),
   })
}

/// The settings screen `security` reads: the idle lock and the attempts left.
///
/// No credential field can appear here, by construction: the settings type holds none, and its own serde
/// skips the Argon2 parameters.
fn security(ctx: &ZeusContext) -> Value {
   json!({
       "autolock": {
           "label": ctx.security.autolock.label(),
           "seconds": ctx.security.autolock.idle_secs(),
       },
       "autolock_changed": ctx.security.autolock_changed,
       "credential_attempts": ctx.security.credential_attempts,
       "load_failed": ctx.security.load_failed,
   })
}

/// The settings screen `railgun` reads: the syncer knobs, per chain where they are per chain.
fn railgun_settings(ctx: &ZeusContext) -> Value {
   let config = &ctx.railgun_config;

   let chains: Vec<Value> = ChainId::supported_chains()
      .iter()
      .map(|chain| {
         json!({
             "chain": chain.id(),
             "name": chain.name(),
             "enabled": config.is_enabled(chain.id()),
             "block_range": config.rpc_syncer_block_range(chain.id()),
         })
      })
      .collect();

   json!({
       "syncer_concurrency": config.rpc_syncer_concurrency(),
       "chains": chains,
       "allow_circuit_download": config.allow_circuit_download(),
       "state_update_interval_minutes": config.state_update_interval_secs() / 60,
   })
}

/// The settings screen `unshield` reads: the bundler that would submit an unshield UserOperation.
///
/// Only the host is answered. The full URL usually carries a provider key in its path or its query — the
/// page shows the host for the same reason — so a grant on this screen never hands a credential over.
fn unshield_settings(ctx: &ZeusContext) -> Value {
   json!({
       "bundler": {
           "host": url_host(&ctx.bundler_url.url),
           "stored": BundlerUrl::exists().unwrap_or(false),
       },
   })
}

/// Whether the grant names this wallet at all.
///
/// A caller that would otherwise spend a network call before projecting — the delegation status is the one
/// — asks this first, so an address outside the grant is refused without touching a node.
pub fn wallet_granted(profile: &AgentProfile, address: Address) -> bool {
   wallet_grant(profile, address).is_ok()
}

/// The refusal for an address the grant does not name, for a caller that refuses it itself.
pub fn unknown_wallet(address: Address) -> ReadError {
   ReadError::NotFound {
      what: "wallet",
      id: hex(address),
   }
}

/// The one wallet `wallet_get` addresses, or `NotFound`.
fn find_wallet(ctx: &ZeusContext, address: Address) -> Result<&WalletInfo, ReadError> {
   ctx.wallet_info_cache.get(&address).ok_or_else(|| ReadError::NotFound {
      what: "wallet",
      id: hex(address),
   })
}

/// The grant on one wallet, if the profile has one that is switched on.
fn wallet_grant(profile: &AgentProfile, address: Address) -> Result<&WalletGrant, ReadError> {
   profile
      .wallets
      .get(&address)
      .filter(|grant| grant.enabled)
      .ok_or_else(|| ReadError::NotFound {
         what: "wallet",
         id: hex(address),
      })
}

/// The grant on one wallet, if it also carries `capability`.
fn wallet_capability<'a>(
   profile: &'a AgentProfile,
   address: Address,
   capability: &'static str,
) -> Result<&'a WalletGrant, ReadError> {
   let grant = wallet_grant(profile, address)?;

   if !grant.caps.contains(capability) {
      return Err(ReadError::NotGranted { capability });
   }

   Ok(grant)
}

/// One chain when the caller named one, every supported chain otherwise.
fn chains_or_all(chain: Option<u64>) -> Vec<u64> {
   match chain {
      Some(chain) => vec![chain],
      None => ChainId::supported_chains().into_iter().map(|chain| chain.id()).collect(),
   }
}

/// A wallet as every list shows it.
fn wallet_row(wallet: &WalletInfo) -> Value {
   let address = hex(wallet.address);

   json!({
       "address": address,
       "display": truncate_address(address.clone()),
       "name": {"value": wallet.name(), "source": "wallet"},
       "zk_address": wallet.railgun_address.as_ref().map(ToString::to_string),
       "master": wallet.is_master,
       "child": wallet.is_child,
       "imported": wallet.is_imported,
   })
}

/// An address with the name the address book knows it by, and a shortened form to read aloud.
fn address_row(ctx: &ZeusContext, chain: u64, address: Address) -> Value {
   let text = hex(address);
   let name = ctx.address_book.get(chain, address);

   json!({
       "address": text,
       "display": truncate_address(text.clone()),
       "name": name.map(|name| json!({"value": name.as_ref(), "source": "address_book"})),
   })
}

/// An amount in the shape every answer carries.
///
/// Never a JSON number: a `u256` does not survive a double, and a model that reads "1.2345" back has to be
/// able to be exact. `usd.known` is false when no price exists, so an agent cannot mistake "unknown" for
/// "worth nothing".
fn amount(value: &NumericValue, decimals: u8, symbol: &str, usd: Option<&NumericValue>) -> Value {
   let known = usd.is_some_and(|usd| !usd.is_zero() || value.is_zero());

   json!({
       "raw": value.wei().to_string(),
       "decimals": decimals,
       "amount": value.flatten(),
       "symbol": symbol,
       "usd": {
           "value": usd.map(NumericValue::f64),
           "known": known,
       },
   })
}

/// A token as a row names it: the address and decimals are the contract's, the symbol and name are the
/// contract's own text and therefore data, not something to follow.
fn token_row(token: &ERC20Token) -> Value {
   json!({
       "address": hex(token.address),
       "decimals": token.decimals,
       "display": {
           "symbol": token.symbol.as_ref(),
           "name": token.name.as_ref(),
           "untrusted": true,
       },
   })
}

/// A currency as an action would name it. The native token is the zero address.
fn currency_row(currency: &Currency) -> Value {
   json!({
       "address": hex(currency.address()),
       "decimals": currency.decimals(),
       "display": {
           "symbol": currency.symbol(),
           "name": currency.name(),
           "untrusted": !currency.is_native(),
       },
   })
}

/// One portfolio, public always and private when the grant carries it.
fn portfolio_row(portfolio: &WalletPortfolio, private: bool) -> Value {
   let public_tokens: Vec<Value> = portfolio
      .public_tokens()
      .iter()
      .map(|(token, balance, value, price)| {
         json!({
             "token": token_row(token),
             "balance": amount(balance, token.decimals, &token.symbol, Some(value)),
             "price_usd": price.f64(),
             "value_usd": value.f64(),
         })
      })
      .collect();

   let mut row = json!({
       "chain": portfolio.chain_id(),
       "public": {
           "value_usd": portfolio.public_value().f64(),
           "tokens": public_tokens,
           "nfts": portfolio.nfts().iter().map(nft_row).collect::<Vec<_>>(),
       },
       "private": Value::Null,
   });

   if private {
      let private_tokens: Vec<Value> = portfolio
         .private_tokens()
         .iter()
         .map(|(token, balance, value, price)| {
            json!({
                "token": token_row(token),
                "balance": amount(balance, token.decimals, &token.symbol, Some(value)),
                "price_usd": price.f64(),
                "value_usd": value.f64(),
            })
         })
         .collect();

      let private_nfts: Vec<Value> = portfolio
         .private_nfts()
         .iter()
         .map(|nft| {
            let held = portfolio
               .private_nft_amounts()
               .get(&(nft.collection, nft.token_id))
               .copied()
               .unwrap_or_default();

            let mut row = nft_row(nft);
            row["amount"] = json!(held);
            row
         })
         .collect();

      row["private"] = json!({
          "value_usd": portfolio.private_value().f64(),
          "tokens": private_tokens,
          "nfts": private_nfts,
      });
   }

   row
}

/// One NFT. There is no name to show: the store keeps an identity and, at most, a metadata URI Zeus itself
/// fetches, so an answer names the collection and the id and nothing the harness should go and read.
fn nft_row(nft: &NftToken) -> Value {
   let collection = hex(nft.collection);

   json!({
       "chain": nft.chain_id,
       "collection": {
           "address": collection,
           "display": truncate_address(collection.clone()),
       },
       "token_id": nft.token_id.to_string(),
       "standard": standard_name(nft.standard),
   })
}

/// One recorded transaction.
fn transaction_row(ctx: &ZeusContext, transaction: &TransactionRich) -> Value {
   let currency = event_currency(&transaction.main_event);

   let (decimals, symbol) = match currency {
      Some(currency) => (currency.decimals(), currency.symbol().to_owned()),
      None => (18, native_symbol(transaction.chain)),
   };

   json!({
       "hash": hex_hash(transaction.hash),
       "chain": transaction.chain,
       "block": transaction.block,
       "timestamp": transaction.timestamp.timestamp(),
       "success": transaction.success,
       "summary": transaction.summary_name(),
       "event": transaction.main_event.name(),
       "to": address_row(ctx, transaction.chain, transaction.interact_to()),
       "contract_interact": transaction.contract_interact,
       "value": amount(
           &transaction.value_sent,
           decimals,
           &symbol,
           Some(&transaction.value_sent_usd),
       ),
       "fee": amount(
           &transaction.tx_cost,
           18,
           &native_symbol(transaction.chain),
           Some(&transaction.tx_cost_usd),
       ),
       "authorised_by": transaction.authorised_by.as_ref().map(ToString::to_string),
   })
}

/// The currency an event moved, when the event names one. A transaction whose main event is unknown keeps
/// the native token, which is what its `value` field holds.
fn event_currency(event: &DecodedEvent) -> Option<&Currency> {
   match event {
      DecodedEvent::Transfer(params) => Some(&params.currency),
      DecodedEvent::SwapToken(params) => Some(&params.input_currency),
      DecodedEvent::Bridge(params) => Some(&params.input_currency),
      _ => None,
   }
}

/// One live token approval.
fn token_approval_row(approval: &TokenApproveParams) -> Value {
   json!({
       "kind": "token",
       "chain": approval.token.chain_id,
       "token": token_row(&approval.token),
       "owner": hex(approval.owner),
       "spender": hex(approval.spender),
       "allowance": amount(
           &approval.amount,
           approval.token.decimals,
           &approval.token.symbol,
           approval.amount_usd.as_ref(),
       ),
       "unlimited": approval.is_unlimited(),
   })
}

/// One live Permit2 approval.
fn permit_row(permit: &PermitParams) -> Value {
   json!({
       "kind": "permit",
       "chain": permit.chain,
       "event": permit.event_name,
       "token": currency_row(&permit.token),
       "owner": hex(permit.owner),
       "spender": hex(permit.spender),
       "allowance": amount(
           &permit.amount,
           permit.token.decimals(),
           permit.token.symbol(),
           permit.amount_usd.as_ref(),
       ),
       "unlimited": permit.is_unlimited(),
       "expires_at": permit.expiration.timestamp(),
   })
}

/// One live NFT approval.
fn nft_approval_row(approval: &NftApproveParams) -> Value {
   json!({
       "kind": "nft",
       "chain": approval.chain,
       "collection": hex(approval.collection),
       "token_id": approval.token_id.map(|id| id.to_string()),
       "owner": hex(approval.owner),
       "operator": hex(approval.operator),
       "standard": approval.standard.map(standard_name),
       "approval_for_all": approval.token_id.is_none(),
       "approved": approval.approved,
       "allowance": approval.amount.map(|amount| amount.to_string()),
       "revoked": approval.is_revoke(),
   })
}

/// One tracked pool.
fn pool_row(pool: &AnyUniswapPool) -> Value {
   json!({
       "chain": pool.chain_id(),
       "id": hex_hash(pool.id()),
       "address": hex(pool.address()),
       "dex": pool.dex_kind().as_str(),
       "fee": pool.fee(),
       "token0": currency_row(pool.currency0()),
       "token1": currency_row(pool.currency1()),
   })
}

/// The standard's name, as an agent would say it.
fn standard_name(standard: NftStandard) -> &'static str {
   if standard.is_erc721() {
      "erc721"
   } else {
      "erc1155"
   }
}

/// A chain's native symbol, for an amount that has no token of its own.
fn native_symbol(chain: u64) -> String {
   ChainId::new(chain)
      .map(|chain| chain.coin_symbol().to_owned())
      .unwrap_or_else(|_| "ETH".to_owned())
}

/// The full, lowercased form of an address, which is the only form an action may consume.
fn hex(address: Address) -> String {
   format!("{address:#x}")
}

/// A hash, full and lowercased.
fn hex_hash(hash: impl std::fmt::LowerHex) -> String {
   format!("{hash:#x}")
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::agent::profile::{ScreenGrant, WalletGrant};
   use crate::{TimeStamp, TransactionRich, ZeusCtx};
   use zeus_eth::alloy_primitives::U256;
   use zeus_eth::currency::ERC20Token;

   fn alice() -> Address {
      Address::repeat_byte(0xa1)
   }

   fn bob() -> Address {
      Address::repeat_byte(0xb0)
   }

   /// An open context with two wallets known to the app and no grant on either.
   fn context() -> ZeusCtx {
      let ctx = ZeusCtx::new(crate::platform::runtime::test_handle());

      ctx.write(|ctx| {
         ctx.vault_unlocked = true;
         ctx.locked = false;

         for address in [alice(), bob()] {
            let mut info = WalletInfo::default();
            info.address = address;
            ctx.wallet_info_cache.map.insert(address, info.clone());
            ctx.wallet_info_cache.ordered_vec.push(info);
         }
      });

      ctx
   }

   fn profile(wallets: &[(Address, &[&str])]) -> AgentProfile {
      let mut profile = AgentProfile::new("tests", [0u8; 32], 0);

      for (address, caps) in wallets {
         let mut grant = WalletGrant {
            enabled: true,
            ..Default::default()
         };

         for capability in *caps {
            grant.caps.insert((*capability).to_owned());
         }

         profile.wallets.insert(*address, grant);
      }

      profile
   }

   fn usdc() -> ERC20Token {
      ERC20Token::from_components(
         1,
         Address::repeat_byte(0x5c),
         "USDC",
         "USD Coin",
         6,
         U256::from(1_000_000_000_000u64),
      )
   }

   #[test]
   fn a_wallet_outside_the_grant_is_absent_and_not_found() {
      let ctx = context();
      let profile = profile(&[(alice(), &[])]);

      let rows = ctx.read(|ctx| wallets(ctx, &profile));

      assert_eq!(rows.len(), 1);
      assert_eq!(
         rows[0]["address"].as_str(),
         Some(format!("{:#x}", alice()).as_str())
      );

      assert_eq!(
         ctx.read(|ctx| wallet(ctx, &profile, bob())),
         Err(ReadError::NotFound {
            what: "wallet",
            id: format!("{:#x}", bob()),
         }),
         "an address the grant does not name is not addressable, and its existence is not confirmed"
      );
   }

   #[test]
   fn a_wallet_without_the_capability_is_not_granted() {
      let ctx = context();
      let profile = profile(&[(alice(), &["wallet.read.history"])]);

      assert_eq!(
         ctx.read(|ctx| portfolio(ctx, &profile, alice(), Some(1))),
         Err(ReadError::NotGranted {
            capability: "wallet.read.portfolio",
         })
      );
   }

   #[test]
   fn the_private_side_is_null_until_the_private_grant_is_given() {
      let ctx = context();

      let mut holdings = WalletPortfolio::new(alice(), 1);
      holdings.add_token(usdc());
      holdings.update_public_data(ctx.clone());
      ctx.write_wallet_state(|state| {
         state.portfolio_db.insert_portfolio(1, alice(), holdings);
      });

      let public_only = profile(&[(alice(), &["wallet.read.portfolio"])]);
      let value = ctx
         .read(|ctx| portfolio(ctx, &public_only, alice(), Some(1)))
         .expect("a portfolio");

      assert!(
         value["portfolios"][0]["private"].is_null(),
         "a null private side means the grant does not carry it, never that the wallet is empty"
      );

      let with_private = profile(&[(
         alice(),
         &["wallet.read.portfolio", "wallet.read.private"],
      )]);
      let value = ctx
         .read(|ctx| portfolio(ctx, &with_private, alice(), Some(1)))
         .expect("a portfolio");

      assert!(value["portfolios"][0]["private"].is_object());
   }

   #[test]
   fn history_is_newest_first_and_paged() {
      let ctx = context();
      let profile = profile(&[(alice(), &["wallet.read.history"])]);

      for (byte, block) in [(1u8, 10u64), (2, 20), (3, 30)] {
         let transaction = TransactionRich {
            chain: 1,
            hash: TxHash::repeat_byte(byte),
            block,
            timestamp: TimeStamp::Seconds(block),
            ..Default::default()
         };

         ctx.tx_db().add_tx(1, alice(), transaction).expect("the history records it");
      }

      let value = ctx.read(|ctx| history(ctx, &profile, alice(), Some(1), 0, 2)).expect("a page");

      assert_eq!(value["total"], 3);

      let rows = value["transactions"].as_array().expect("rows");
      assert_eq!(rows.len(), 2);
      assert_eq!(rows[0]["block"], 30);
      assert_eq!(rows[1]["block"], 20);
      assert!(
         rows[0]["value"]["raw"].is_string(),
         "an amount is never a JSON number: a u256 does not survive a double"
      );
   }

   #[test]
   fn a_screen_needs_its_own_switch_and_an_unknown_one_is_not_found() {
      let ctx = context();
      let mut profile = profile(&[]);

      assert_eq!(
         ctx.read(|ctx| screen(ctx, &profile, "general")),
         Err(ReadError::NotGranted {
            capability: "settings.general.read",
         })
      );

      profile.screens.insert(
         "settings.general.read".to_owned(),
         ScreenGrant {
            read: true,
            write: false,
         },
      );

      let value = ctx.read(|ctx| screen(ctx, &profile, "general")).expect("the screen");

      assert_eq!(value["screen"], "general");
      assert!(value["values"]["minimize_to_tray"].is_boolean());

      assert_eq!(
         ctx.read(|ctx| screen(ctx, &profile, "nowhere")),
         Err(ReadError::NotFound {
            what: "screen",
            id: "nowhere".to_owned(),
         })
      );
   }

   /// Every screen the list offers has a projection: a name added to the table without one answers an
   /// error, and this test is what turns that into a red build.
   #[test]
   fn every_listed_screen_answers_when_its_switch_is_granted() {
      let ctx = context();
      let mut profile = profile(&[(alice(), &["wallet.read.portfolio"])]);

      for screen in SCREENS {
         profile.screens.insert(
            screen.read.to_owned(),
            ScreenGrant {
               read: true,
               write: false,
            },
         );
      }

      for screen in SCREENS {
         let value = ctx
            .read(|ctx| self::screen(ctx, &profile, screen.name))
            .unwrap_or_else(|error| panic!("{}: {error:?}", screen.name));

         assert_eq!(value["screen"], screen.name);
         assert!(
            value["values"].is_object(),
            "{} must answer an object",
            screen.name
         );
      }
   }

   #[test]
   fn an_amount_says_when_its_usd_value_is_unknown() {
      let value = NumericValue::format_wei(U256::from(1_500_000u64), 6);
      let row = amount(&value, 6, "USDC", None);

      assert_eq!(row["raw"], "1500000");
      assert_eq!(row["amount"], "1.5");
      assert_eq!(row["decimals"], 6);
      assert_eq!(
         row["usd"]["known"], false,
         "no price is unknown, not worth nothing"
      );
   }

   #[test]
   fn delegation_outside_the_grant_is_refused_without_a_network_call() {
      let ctx = context();
      let profile = profile(&[(alice(), &[])]);

      assert!(wallet_granted(&profile, alice()));
      assert!(!wallet_granted(&profile, bob()));

      assert_eq!(
         ctx.read(|ctx| delegate(ctx, &profile, bob(), 1, None)),
         Err(unknown_wallet(bob()))
      );
   }
}
