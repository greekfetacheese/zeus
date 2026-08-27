use serde_json::Value;
use std::str::FromStr;
use zeus_eth::alloy_primitives::{Address, U256};

/// Resolve an ERC-7730 path against the EIP-712 message / container / metadata.
///
/// Roots:
/// - `#.` or bare → decoded message
/// - `@.` → container (`to` = verifying contract, `chainId`, `from`, `value`)
/// - `$.metadata.constants.NAME`
pub fn resolve_path<'a>(
   path: &str,
   message: &'a Value,
   metadata_constants: &'a serde_json::Map<String, Value>,
   container: &Container,
) -> Option<Value> {
   let path = path.trim();
   if path.is_empty() {
      return None;
   }

   if let Some(rest) = path.strip_prefix("$.metadata.constants.") {
      return metadata_constants.get(rest).cloned();
   }

   if let Some(rest) = path.strip_prefix("@.") {
      return container.get(rest);
   }

   let rest = path.strip_prefix("#.").unwrap_or(path);
   walk_json(message, rest)
}

#[derive(Debug, Clone)]
pub struct Container {
   pub from: Option<Address>,
   pub to: Option<Address>,
   pub chain_id: u64,
}

impl Container {
   fn get(&self, key: &str) -> Option<Value> {
      match key {
         "from" => self.from.map(|a| Value::String(a.to_string())),
         "to" | "verifyingContract" => self.to.map(|a| Value::String(a.to_string())),
         "chainId" => Some(Value::from(self.chain_id)),
         "value" => Some(Value::from(0u64)),
         _ => None,
      }
   }
}

fn walk_json(value: &Value, path: &str) -> Option<Value> {
   if path.is_empty() {
      return Some(value.clone());
   }
   let mut cur = value;
   for seg in path.split('.') {
      let seg = seg.trim();
      if seg.is_empty() || seg == "[]" {
         return None;
      }
      if let Some(idx) = parse_index(seg) {
         cur = cur.as_array()?.get(idx)?;
         continue;
      }
      cur = cur.get(seg)?;
   }
   Some(cur.clone())
}

fn parse_index(seg: &str) -> Option<usize> {
   if let Some(inner) = seg.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
      return inner.parse().ok();
   }
   None
}

pub fn value_as_u256(value: &Value) -> Option<U256> {
   if let Some(s) = value.as_str() {
      return parse_u256(s);
   }
   if let Some(n) = value.as_u64() {
      return Some(U256::from(n));
   }
   if let Some(n) = value.as_i64() {
      if n >= 0 {
         return Some(U256::from(n as u64));
      }
   }
   if let Some(n) = value.as_number() {
      return parse_u256(&n.to_string());
   }
   None
}

pub fn parse_u256(s: &str) -> Option<U256> {
   let s = s.trim();
   if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
      return U256::from_str_radix(hex, 16).ok();
   }
   U256::from_str(s).ok()
}

pub fn value_as_address(value: &Value) -> Option<Address> {
   let s = value.as_str()?;
   Address::from_str(s).ok()
}
