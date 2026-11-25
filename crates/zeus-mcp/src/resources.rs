//! The documents an agent can pull by URI instead of calling a tool.
//!
//! Resources are the read surface that never changes shape: the grant the caller is acting under, the
//! wallets it covers, and the catalog that explains both. They are what a harness re-reads when it wants
//! context without spending a call, and none of them is secret material — the grant is exactly the
//! document the policy enforces, and the caller already presented the credential it belongs to.
//!
//! A resource the grant does not cover answers "unknown resource" rather than an empty document, because
//! an empty document reads as "there is nothing there" and hides the refusal.

use crate::error::ProtocolError;
use crate::protocol::types;
use crate::tools::{Call, catalog, guard, wallets};
use serde_json::{Value, json};
use zeus_core::agent::Intent;

/// One document this server serves.
pub struct Resource {
   pub uri: &'static str,
   pub name: &'static str,
   pub description: &'static str,
   /// The capability a read needs, or nothing when the document describes the caller's own grant.
   pub capability: Option<&'static str>,
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

/// Every resource this server serves, in a stable order.
pub const RESOURCES: &[Resource] = &[PROFILE, PERMISSIONS, WALLETS];

/// A `resources/list` result, scoped to the grant.
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

/// A `resources/templates/list` result.
///
/// Empty until a tool has an object an agent can address by name rather than by argument.
pub fn templates(modern: bool) -> Value {
   types::resource_template_list(Vec::new(), modern)
}

/// Read one resource by its URI.
pub fn read(call: &Call, uri: &str, modern: bool) -> Result<Value, ProtocolError> {
   let resource = RESOURCES
      .iter()
      .find(|resource| resource.uri == uri)
      .ok_or_else(|| ProtocolError::unknown_resource(uri))?;

   // An ungranted document is answered as unknown rather than empty: whether it exists is itself part of
   // what the grant decides.
   if let Some(capability) = resource.capability {
      guard(call, uri, &Intent::GlobalRead { capability })
         .map_err(|_| ProtocolError::unknown_resource(uri))?;
   }

   let document = match resource.uri {
      "zeus://profile" => serde_json::to_value(&call.profile).unwrap_or(Value::Null),
      "zeus://wallets" => json!({"wallets": wallets::wallets(call)}),
      _ => json!({
         "catalog": catalog(&call.profile),
         "switch": crate::protocol::GRANT_LOCATION,
      }),
   };

   Ok(types::resource_contents(uri, &document, modern))
}
