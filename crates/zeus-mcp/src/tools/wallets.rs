//! The wallets a grant gives this profile.
//!
//! A wallet the grant does not name does not exist as far as an agent can tell: it is not listed, it is
//! not addressable, and no tool will mention it. That is the point of granting per wallet rather than per
//! profile — a profile that may read one address cannot enumerate the others.

use super::{Call, ToolFuture, ToolOutput, ToolSpec};
use crate::protocol::types::Annotations;
use serde_json::{Value, json};
use zeus_core::agent::AgentProfile;
use zeus_core::truncate_address;
use zeus_eth::alloy_primitives::Address;

/// Every granted wallet.
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

fn list(call: &Call, _arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move { Ok(ToolOutput::new(json!({"wallets": wallets(call)}))) })
}

/// The granted wallets, as both this tool and the `zeus://wallets` resource answer.
pub fn wallets(call: &Call) -> Vec<Value> {
   call
      .ctx
      .get_all_wallets_info()
      .into_iter()
      .filter(|wallet| visible(&call.profile, wallet.address))
      .map(|wallet| {
         let address = format!("{:#x}", wallet.address);

         json!({
            "address": address,
            "display": truncate_address(address),
            "name": {"value": wallet.name(), "source": "wallet"},
            "zk_address": wallet.railgun_address.as_ref().map(ToString::to_string),
            "master": wallet.is_master,
            "child": wallet.is_child,
            "imported": wallet.is_imported,
         })
      })
      .collect()
}

/// Whether the grant gives this profile the wallet at all.
fn visible(profile: &AgentProfile, address: Address) -> bool {
   profile.wallets.get(&address).is_some_and(|grant| grant.enabled)
}
