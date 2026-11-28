//! The documents an agent can pull by URI instead of calling a tool.
//!
//! Resources are the read surface that never changes shape: the grant the caller is acting under, the
//! wallets it covers, the catalog that explains both, and — through templates — one wallet's portfolio
//! or approvals, one transaction, one settings screen. They are what a harness re-reads when it wants
//! context without spending a call, and none of them is secret material: the grant is exactly the
//! document the policy enforces, and the caller already presented the credential it belongs to.
//!
//! A resource the grant does not cover answers "unknown resource" rather than an empty document, because
//! an empty document reads as "there is nothing there" and hides the refusal.

use crate::error::{ErrorCode, ProtocolError, ToolError};
use crate::protocol::types;
use crate::tools::{Call, catalog, guard, read_error};
use serde_json::{Value, json};
use zeus_core::agent::{AgentProfile, Intent, read};
use zeus_eth::alloy_primitives::Address;

/// One document this server serves by a fixed URI.
pub struct Resource {
   pub uri: &'static str,
   pub name: &'static str,
   pub description: &'static str,
   /// The capability a read needs, or nothing when the document describes the caller's own grant.
   pub capability: Option<&'static str>,
}

/// One family of documents addressed by a path segment.
pub struct Template {
   pub uri: &'static str,
   pub name: &'static str,
   pub description: &'static str,
}

const PROFILE: Resource = Resource {
   uri: "zeus://profile",
   name: "The grant",
   description: "The grant document of the credential this request presented, exactly as it is \
      enforced.",
   capability: None,
};

const WALLETS: Resource = Resource {
   uri: "zeus://wallets",
   name: "Granted wallets",
   description: "The wallets this profile may see, with their addresses and zk addresses.",
   capability: Some("settings.wallets.read"),
};

const PERMISSIONS: Resource = Resource {
   uri: "zeus://permissions",
   name: "The capability catalog",
   description: "Every capability Zeus can grant, its risk tier, its privacy badge, and whether this \
      profile holds it.",
   capability: None,
};

/// Every resource this server serves by a fixed URI, in a stable order.
pub const RESOURCES: &[Resource] = &[PROFILE, PERMISSIONS, WALLETS];

/// Every family of documents addressed by an argument, as RFC 6570 templates.
pub const TEMPLATES: &[Template] = &[
   Template {
      uri: "zeus://wallet/{address}/portfolio",
      name: "One wallet's portfolio",
      description: "The portfolio portfolio_get answers for one address.",
   },
   Template {
      uri: "zeus://wallet/{address}/approvals",
      name: "One wallet's approvals",
      description: "The approvals approvals_list answers for one address.",
   },
   Template {
      uri: "zeus://tx/{hash}",
      name: "One transaction",
      description: "The row tx_get answers for one hash, from whichever granted wallet recorded it.",
   },
   Template {
      uri: "zeus://settings/{screen}",
      name: "One settings screen",
      description: "The values settings_get answers for one screen.",
   },
];

/// A resources/list result, scoped to the grant.
pub fn list(call: &Call, modern: bool) -> Value {
   let resources = RESOURCES
      .iter()
      .filter(|resource| {
         resource
            .capability
            .is_none_or(|name| crate::tools::granted(&call.profile, name))
      })
      .map(|resource| types::resource(resource.uri, resource.name, resource.description))
      .collect();

   types::resource_list(resources, modern)
}

/// A resources/templates/list result.
///
/// A template is a promise that a URI of that shape answers, so it follows the grant like everything else:
/// a profile that may not read a portfolio is not handed the shape that would only ever answer "unknown".
/// The gate below is the same catalog every tool reads, so the two cannot drift.
pub fn templates(profile: &AgentProfile, modern: bool) -> Value {
   let templates = TEMPLATES
      .iter()
      .filter(|template| match template.uri {
         "zeus://wallet/{address}/portfolio" => {
            crate::tools::granted(profile, "wallet.read.portfolio")
         }
         "zeus://wallet/{address}/approvals" => {
            crate::tools::granted(profile, "wallet.read.approvals")
         }
         "zeus://tx/{hash}" => crate::tools::granted(profile, "wallet.read.history"),
         // A screen's switch is per screen, so the family answers while any one of them is readable.
         "zeus://settings/{screen}" => profile.screens.values().any(|grant| grant.read),
         _ => false,
      })
      .map(|template| types::resource_template(template.uri, template.name, template.description))
      .collect();

   types::resource_template_list(templates, modern)
}

/// Read one resource by its URI.
pub fn read(call: &Call, uri: &str, modern: bool) -> Result<Value, ProtocolError> {
   let document = match RESOURCES.iter().find(|resource| resource.uri == uri) {
      Some(resource) => fixed(call, resource)?,
      None => keyed(call, uri)?,
   };

   Ok(types::resource_contents(uri, &document, modern))
}

/// The grant document a model is shown: everything the policy enforces, and not the verifier.
///
/// The profile carries a token hash because the store needs it to authenticate the next request; a model
/// does not, and a hash of its own bearer token is the one field in this document that is credential
/// material rather than grant. Dropping it keeps "the grant, exactly as enforced" true without handing
/// the caller the second half of the credential it presented.
fn public_profile(profile: &AgentProfile) -> Value {
   let mut document = serde_json::to_value(profile).unwrap_or(Value::Null);

   if let Value::Object(fields) = &mut document {
      fields.remove("token_hash");
   }

   document
}

/// One document served at a fixed URI.
fn fixed(call: &Call, resource: &Resource) -> Result<Value, ProtocolError> {
   // An ungranted document is answered as unknown rather than empty: whether it exists is itself part of
   // what the grant decides.
   if let Some(capability) = resource.capability {
      guard(
         call,
         resource.uri,
         &Intent::GlobalRead { capability },
      )
      .map_err(|error| refused(resource.uri, error))?;
   }

   Ok(match resource.uri {
      "zeus://profile" => public_profile(&call.profile),
      "zeus://wallets" => json!({"wallets": crate::tools::wallet::rows(call)}),
      _ => json!({
         "catalog": catalog(&call.profile),
         "switch": crate::protocol::GRANT_LOCATION,
      }),
   })
}

/// One document addressed by a path segment.
fn keyed(call: &Call, uri: &str) -> Result<Value, ProtocolError> {
   if let Some((address, kind)) = wallet_uri(uri) {
      let capability = match kind {
         "portfolio" => "wallet.read.portfolio",
         "approvals" => "wallet.read.approvals",
         _ => return Err(ProtocolError::unknown_resource(uri)),
      };

      guard(
         call,
         uri,
         &Intent::WalletRead {
            capability,
            wallet: address,
         },
      )
      .map_err(|error| refused(uri, error))?;

      return call
         .ctx
         .read(|ctx| match kind {
            "portfolio" => read::portfolio(ctx, &call.profile, address, None),
            _ => read::approvals(ctx, &call.profile, address, None),
         })
         .map_err(|error| refused(uri, read_error(error)));
   }

   if let Some(hash) = uri.strip_prefix("zeus://tx/") {
      return transaction(call, uri, hash);
   }

   if let Some(screen) = uri.strip_prefix("zeus://settings/") {
      let Some(capability) = read::screen_read(screen) else {
         return Err(ProtocolError::unknown_resource(uri));
      };

      guard(call, uri, &Intent::GlobalRead { capability }).map_err(|error| refused(uri, error))?;

      return call
         .ctx
         .read(|ctx| read::screen(ctx, &call.profile, screen))
         .map_err(|error| refused(uri, read_error(error)));
   }

   Err(ProtocolError::unknown_resource(uri))
}

/// One transaction, from whichever granted wallet recorded it.
///
/// A hash does not name its wallet, so the search runs over the wallets whose grant allows history; a hash
/// no granted wallet recorded is unknown, exactly as an ungranted document is.
fn transaction(call: &Call, uri: &str, hash: &str) -> Result<Value, ProtocolError> {
   let wallets: Vec<Address> = call
      .profile
      .wallets
      .iter()
      .filter(|(_, grant)| grant.enabled && grant.caps.contains("wallet.read.history"))
      .map(|(address, _)| *address)
      .collect();

   let Some(&first) = wallets.first() else {
      return Err(ProtocolError::unknown_resource(uri));
   };

   guard(
      call,
      uri,
      &Intent::WalletRead {
         capability: "wallet.read.history",
         wallet: first,
      },
   )
   .map_err(|error| refused(uri, error))?;

   let hash = hash.parse().map_err(|_| ProtocolError::unknown_resource(uri))?;

   for wallet in wallets {
      if let Ok(document) = call.ctx.read(|ctx| read::transaction(ctx, &call.profile, wallet, hash))
      {
         return Ok(document);
      }
   }

   Err(ProtocolError::unknown_resource(uri))
}

/// A refusal a resource read cannot carry as a tool error.
///
/// A locked vault is reported as unavailable rather than unknown, because "unknown resource" would be a
/// lie a user could act on: the document exists, it is simply unreadable until the app is unlocked.
fn refused(uri: &str, error: ToolError) -> ProtocolError {
   match error.code {
      ErrorCode::VaultLocked => ProtocolError::new(
         ProtocolError::INTERNAL_ERROR,
         format!("Zeus is locked: {uri} is unreadable until it is unlocked"),
      )
      .with_data(json!({"code": "VAULT_LOCKED"})),
      _ => ProtocolError::unknown_resource(uri),
   }
}

/// The wallet and the document a wallet URI names.
fn wallet_uri(uri: &str) -> Option<(Address, &str)> {
   let rest = uri.strip_prefix("zeus://wallet/")?;
   let (address, kind) = rest.split_once('/')?;
   Some((address.parse().ok()?, kind))
}
