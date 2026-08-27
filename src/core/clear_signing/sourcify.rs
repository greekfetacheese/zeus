use serde_json::Value;
use std::str::FromStr;
use std::sync::OnceLock;
use std::time::Duration;
use zeus_eth::alloy_primitives::Address;

const SOURCIFY_BASE: &str = "https://sourcify.dev/server";
const FETCH_TIMEOUT: Duration = Duration::from_secs(3);

fn http_client() -> &'static reqwest::Client {
   static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
   CLIENT.get_or_init(|| {
      reqwest::Client::builder()
         .user_agent("zeus-wallet")
         .timeout(FETCH_TIMEOUT)
         .build()
         .unwrap_or_else(|_| reqwest::Client::new())
   })
}

/// If `address` is a proxy verified on Sourcify, return the implementation address.
pub async fn implementation_address(chain: u64, address: Address) -> Option<Address> {
   let url = format!("{SOURCIFY_BASE}/v2/contract/{chain}/{address:#x}?fields=proxyResolution");
   let resp = http_client().get(&url).send().await.ok()?;
   if !resp.status().is_success() {
      return None;
   }
   let value: Value = resp.json().await.ok()?;
   parse_implementation(&value)
}

pub fn parse_implementation(value: &Value) -> Option<Address> {
   let proxy = value.get("proxyResolution").unwrap_or(value);

   if let Some(s) = proxy
      .pointer("/implementation/address")
      .and_then(|v| v.as_str())
      .or_else(|| proxy.get("implementation").and_then(|v| v.as_str()))
   {
      if let Ok(addr) = Address::from_str(s) {
         if !addr.is_zero() {
            return Some(addr);
         }
      }
   }

   if let Some(arr) = proxy.get("implementations").and_then(|v| v.as_array()) {
      for item in arr {
         let s = item.get("address").and_then(|v| v.as_str()).or_else(|| item.as_str());
         if let Some(s) = s {
            if let Ok(addr) = Address::from_str(s) {
               if !addr.is_zero() {
                  return Some(addr);
               }
            }
         }
      }
   }

   None
}
