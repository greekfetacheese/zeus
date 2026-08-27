use super::display::Intent;
use anyhow::anyhow;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::str::FromStr;
use zeus_eth::{
   alloy_dyn_abi::TypedData,
   alloy_primitives::{Address, B256, U256, keccak256},
};

#[derive(Debug, Clone)]
pub struct Descriptor {
   pub context: Context,
   pub metadata: Metadata,
   pub formats: HashMap<String, FormatSpec>,
}

#[derive(Debug, Clone, Default)]
pub struct Context {
   pub eip712: Option<Eip712Context>,
}

#[derive(Debug, Clone, Default)]
pub struct Eip712Context {
   pub domain: Map<String, Value>,
   pub deployments: Vec<Deployment>,
   pub domain_separator: Option<B256>,
}

#[derive(Debug, Clone)]
pub struct Deployment {
   pub chain_id: u64,
   pub address: Address,
}

#[derive(Debug, Clone, Default)]
pub struct Metadata {
   pub owner: Option<String>,
   pub contract_name: Option<String>,
   pub info_url: Option<String>,
   pub constants: Map<String, Value>,
   pub enums: HashMap<String, HashMap<String, String>>,
}

#[derive(Debug, Clone)]
pub struct FormatSpec {
   pub intent: Intent,
   pub interpolated_intent: Option<String>,
   pub fields: Vec<FieldSpec>,
}

#[derive(Debug, Clone)]
pub struct FieldSpec {
   pub path: String,
   pub label: String,
   pub format: String,
   pub params: Map<String, Value>,
   pub visible: Option<String>,
}

impl FieldSpec {
   pub fn is_hidden(&self) -> bool {
      matches!(self.visible.as_deref(), Some("never"))
   }
}

/// Deep-merge two descriptor JSON documents. `including` wins on conflicts.
/// Field arrays that look like `display.formats.*.fields` are merged by `path`.
pub fn merge_descriptor_json(included: Value, including: Value) -> Value {
   merge_values(included, including)
}

fn merge_values(base: Value, overlay: Value) -> Value {
   match (base, overlay) {
      (Value::Object(mut base_map), Value::Object(over_map)) => {
         for (key, over_val) in over_map {
            if let Some(base_val) = base_map.remove(&key) {
               base_map.insert(key, merge_values(base_val, over_val));
            } else {
               base_map.insert(key, over_val);
            }
         }
         Value::Object(base_map)
      }
      (Value::Array(base_arr), Value::Array(over_arr))
         if is_field_array(&base_arr) && is_field_array(&over_arr) =>
      {
         Value::Array(merge_fields(base_arr, over_arr))
      }
      (_, overlay) => overlay,
   }
}

fn is_field_array(arr: &[Value]) -> bool {
   !arr.is_empty() && arr.iter().all(|v| v.get("path").and_then(|p| p.as_str()).is_some())
}

fn merge_fields(base: Vec<Value>, overlay: Vec<Value>) -> Vec<Value> {
   let mut out = base;
   for over in overlay {
      let path = over.get("path").and_then(|p| p.as_str()).unwrap_or("");
      if let Some(existing) =
         out.iter_mut().find(|v| v.get("path").and_then(|p| p.as_str()) == Some(path))
      {
         *existing = merge_values(existing.clone(), over);
      } else {
         out.push(over);
      }
   }
   out
}

pub fn schema_major(schema: &str) -> Option<u32> {
   // e.g. .../erc7730-v2.schema.json  or  .../erc7730-v1.schema.json
   let idx = schema.find("erc7730-v")?;
   let rest = &schema[idx + "erc7730-v".len()..];
   let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
   digits.parse().ok()
}

pub fn parse_descriptor(value: &Value) -> Result<Descriptor, anyhow::Error> {
   if let Some(schema) = value.get("$schema").and_then(|v| v.as_str()) {
      if let Some(major) = schema_major(schema) {
         if major != 2 {
            return Err(anyhow!(
               "unsupported ERC-7730 schema major version {major}"
            ));
         }
      }
   }

   let context = parse_context(value.get("context"))?;
   let metadata = parse_metadata(value.get("metadata"));
   let formats = parse_formats(value.pointer("/display/formats"))?;

   if formats.is_empty() {
      return Err(anyhow!("descriptor has no display.formats"));
   }

   Ok(Descriptor {
      context,
      metadata,
      formats,
   })
}

fn parse_context(value: Option<&Value>) -> Result<Context, anyhow::Error> {
   let Some(value) = value else {
      return Ok(Context::default());
   };
   let eip712 = value.get("eip712").map(parse_eip712_context).transpose()?;
   Ok(Context { eip712 })
}

fn parse_eip712_context(value: &Value) -> Result<Eip712Context, anyhow::Error> {
   let domain = value.get("domain").and_then(|v| v.as_object()).cloned().unwrap_or_default();

   let mut deployments = Vec::new();
   if let Some(arr) = value.get("deployments").and_then(|v| v.as_array()) {
      for item in arr {
         let chain_id =
            json_u64(item.get("chainId")).ok_or_else(|| anyhow!("deployment chainId"))?;
         let address = item
            .get("address")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("deployment missing address"))?;
         let address = Address::from_str(address)?;
         deployments.push(Deployment { chain_id, address });
      }
   }

   let domain_separator = value
      .get("domainSeparator")
      .and_then(|v| v.as_str())
      .map(|s| B256::from_str(s))
      .transpose()?;

   Ok(Eip712Context {
      domain,
      deployments,
      domain_separator,
   })
}

fn parse_metadata(value: Option<&Value>) -> Metadata {
   let Some(value) = value else {
      return Metadata::default();
   };
   let owner = value.get("owner").and_then(|v| v.as_str()).map(|s| s.to_string());
   let contract_name = value.get("contractName").and_then(|v| v.as_str()).map(|s| s.to_string());
   let info_url = value.pointer("/info/url").and_then(|v| v.as_str()).map(|s| s.to_string());
   let constants = value.get("constants").and_then(|v| v.as_object()).cloned().unwrap_or_default();

   let mut enums = HashMap::new();
   if let Some(obj) = value.get("enums").and_then(|v| v.as_object()) {
      for (name, entries) in obj {
         if let Some(map) = entries.as_object() {
            let mut inner = HashMap::new();
            for (k, v) in map {
               if let Some(label) = v.as_str() {
                  inner.insert(k.clone(), label.to_string());
               }
            }
            enums.insert(name.clone(), inner);
         }
      }
   }

   Metadata {
      owner,
      contract_name,
      info_url,
      constants,
      enums,
   }
}

fn parse_formats(value: Option<&Value>) -> Result<HashMap<String, FormatSpec>, anyhow::Error> {
   let Some(obj) = value.and_then(|v| v.as_object()) else {
      return Ok(HashMap::new());
   };
   let mut out = HashMap::new();
   for (key, spec) in obj {
      out.insert(key.clone(), parse_format_spec(spec)?);
   }
   Ok(out)
}

fn parse_format_spec(value: &Value) -> Result<FormatSpec, anyhow::Error> {
   let intent = parse_intent(value.get("intent"))?;
   let interpolated_intent =
      value.get("interpolatedIntent").and_then(|v| v.as_str()).map(|s| s.to_string());

   let mut fields = Vec::new();
   if let Some(arr) = value.get("fields").and_then(|v| v.as_array()) {
      for item in arr {
         let path = item
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("field missing path"))?
            .to_string();
         let label = item.get("label").and_then(|v| v.as_str()).unwrap_or("").to_string();
         let format = item.get("format").and_then(|v| v.as_str()).unwrap_or("raw").to_string();
         let params = item.get("params").and_then(|v| v.as_object()).cloned().unwrap_or_default();
         let visible = item.get("visible").and_then(|v| v.as_str()).map(|s| s.to_string());
         fields.push(FieldSpec {
            path,
            label,
            format,
            params,
            visible,
         });
      }
   }

   Ok(FormatSpec {
      intent,
      interpolated_intent,
      fields,
   })
}

fn parse_intent(value: Option<&Value>) -> Result<Intent, anyhow::Error> {
   match value {
      Some(Value::String(s)) => Ok(Intent::Text(s.clone())),
      Some(Value::Object(map)) => {
         let mut pairs = Vec::new();
         for (k, v) in map {
            let val = v.as_str().unwrap_or("").to_string();
            pairs.push((k.clone(), val));
         }
         Ok(Intent::Pairs(pairs))
      }
      Some(_) => Err(anyhow!("invalid intent")),
      None => Ok(Intent::Text("Sign".to_string())),
   }
}

fn json_u64(value: Option<&Value>) -> Option<u64> {
   let value = value?;
   if let Some(n) = value.as_u64() {
      return Some(n);
   }
   if let Some(s) = value.as_str() {
      return s.parse().ok();
   }
   None
}

/// Bind an EIP-712 message to this descriptor and return the matching format spec.
pub fn bind_eip712<'a>(
   descriptor: &'a Descriptor,
   typed: &TypedData,
) -> Result<&'a FormatSpec, anyhow::Error> {
   let eip712 = descriptor
      .context
      .eip712
      .as_ref()
      .ok_or_else(|| anyhow!("descriptor is not bound to an EIP-712 context"))?;

   for (key, expected) in &eip712.domain {
      let actual =
         domain_field(typed, key).ok_or_else(|| anyhow!("message domain missing key {key}"))?;
      if !domain_values_match(expected, &actual) {
         return Err(anyhow!(
            "domain.{key} does not match descriptor constraint"
         ));
      }
   }

   if !eip712.deployments.is_empty() {
      let chain_id =
         typed.domain.chain_id.ok_or_else(|| anyhow!("message domain missing chainId"))?;
      let chain_id = u64::try_from(chain_id).unwrap_or(0);
      let verifying = typed
         .domain
         .verifying_contract
         .ok_or_else(|| anyhow!("message domain missing verifyingContract"))?;
      let matched = eip712
         .deployments
         .iter()
         .any(|d| d.chain_id == chain_id && d.address == verifying);
      if !matched {
         return Err(anyhow!(
            "verifyingContract/chainId is not in descriptor deployments"
         ));
      }
   }

   if let Some(expected) = eip712.domain_separator {
      if typed.domain.separator() != expected {
         return Err(anyhow!("domainSeparator does not match"));
      }
   }

   let encode_type = typed.encode_type().map_err(|e| anyhow!("{e}"))?;
   let want = keccak256(encode_type.as_bytes());
   for (key, spec) in &descriptor.formats {
      if keccak256(key.as_bytes()) == want {
         return Ok(spec);
      }
   }

   Err(anyhow!(
      "no display.formats entry matches encodeType"
   ))
}

fn domain_field(typed: &TypedData, key: &str) -> Option<String> {
   match key {
      "name" => typed.domain.name.as_ref().map(|s| s.to_string()),
      "version" => typed.domain.version.as_ref().map(|s| s.to_string()),
      "chainId" => typed.domain.chain_id.map(|id| id.to_string()),
      "verifyingContract" => typed.domain.verifying_contract.map(|a| a.to_string()),
      "salt" => typed.domain.salt.map(|s| s.to_string()),
      _ => None,
   }
}

fn domain_values_match(expected: &Value, actual: &str) -> bool {
   match expected {
      Value::String(s) => {
         if let (Ok(a), Ok(b)) = (Address::from_str(s), Address::from_str(actual)) {
            return a == b;
         }
         if let (Ok(a), Ok(b)) = (parse_u256(s), parse_u256(actual)) {
            return a == b;
         }
         s.eq_ignore_ascii_case(actual)
      }
      Value::Number(n) => {
         if let (Ok(a), Ok(b)) = (parse_u256(&n.to_string()), parse_u256(actual)) {
            a == b
         } else {
            n.to_string() == actual
         }
      }
      other => other.to_string().trim_matches('"') == actual,
   }
}

fn parse_u256(s: &str) -> Result<U256, ()> {
   U256::from_str(s).map_err(|_| ())
}
