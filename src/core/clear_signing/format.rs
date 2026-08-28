use super::descriptor::{Descriptor, FieldSpec, FormatSpec, Metadata, ParsedCallFormat};
use super::display::{ClearDisplay, ClearSource, DisplayField, FormattedValue};
use super::path::{self, Container};
use crate::utils::TimeStamp;
use serde_json::{Map, Value};
use std::collections::HashMap;
use zeus_eth::{
   alloy_dyn_abi::DynSolValue,
   alloy_primitives::{Address, U256, hex},
   currency::ERC20Token,
   utils::NumericValue,
};

#[derive(Debug, Clone, Default)]
pub struct FormatData {
   pub tokens: HashMap<(u64, Address), ERC20Token>,
   pub names: HashMap<(u64, Address), String>,
}

impl FormatData {
   pub fn token(&self, chain: u64, address: Address) -> Option<&ERC20Token> {
      self.tokens.get(&(chain, address))
   }

   pub fn name(&self, chain: u64, address: Address) -> Option<&str> {
      self.names.get(&(chain, address)).map(|s| s.as_str())
   }
}

pub fn format_eip712(
   descriptor: &Descriptor,
   spec: &FormatSpec,
   message: &Value,
   container: &Container,
   data: &FormatData,
   source_path: String,
) -> ClearDisplay {
   let mut warnings = Vec::new();
   let mut fields = Vec::new();
   let mut formatted_by_path: HashMap<String, String> = HashMap::new();

   for field in &spec.fields {
      if field.is_hidden() {
         continue;
      }
      if field.format == "calldata" || field.path.contains("[]") {
         warnings.push(format!(
            "{}: nested calldata not shown",
            field.label
         ));
         continue;
      }
      match format_field(
         field,
         message,
         &descriptor.metadata,
         container,
         data,
      ) {
         Ok(display_field) => {
            formatted_by_path.insert(field.path.clone(), display_field.value.as_text());
            fields.push(display_field);
         }
         Err(e) => {
            warnings.push(format!("{}: {e}", field.label));
            if let Some(raw) = path::resolve_path(
               &field.path,
               message,
               &descriptor.metadata.constants,
               container,
            ) {
               let text = json_to_text(&raw);
               formatted_by_path.insert(field.path.clone(), text.clone());
               fields.push(DisplayField {
                  label: field.label.clone(),
                  value: FormattedValue::Text(text),
               });
            }
         }
      }
   }

   let interpolated = spec
      .interpolated_intent
      .as_ref()
      .and_then(|tmpl| interpolate(tmpl, spec, &formatted_by_path).ok());

   let intent = spec.intent.clone();
   ClearDisplay {
      heading: intent.heading(),
      intent,
      interpolated_intent: interpolated,
      owner: descriptor.metadata.owner.clone(),
      contract_name: descriptor.metadata.contract_name.clone(),
      info_url: descriptor.metadata.info_url.clone(),
      fields,
      source: ClearSource::Registry { path: source_path },
      warnings,
   }
}

pub fn collect_token_addresses(
   spec: &FormatSpec,
   message: &Value,
   metadata: &Metadata,
   container: &Container,
) -> Vec<(u64, Address)> {
   let mut out = Vec::new();
   for field in &spec.fields {
      if field.format != "tokenAmount" {
         continue;
      }
      let chain = token_chain(field, message, metadata, container);
      if let Some(addr) = token_address(field, message, metadata, container) {
         out.push((chain, addr));
      }
   }
   out
}

fn format_field(
   field: &FieldSpec,
   message: &Value,
   metadata: &Metadata,
   container: &Container,
   data: &FormatData,
) -> Result<DisplayField, anyhow::Error> {
   let raw = path::resolve_path(
      &field.path,
      message,
      &metadata.constants,
      container,
   )
   .ok_or_else(|| anyhow::anyhow!("path not found"))?;

   let value = match field.format.as_str() {
      "addressName" => format_address(&raw, container.chain_id, data),
      "tokenAmount" => format_token_amount(field, &raw, message, metadata, container, data)?,
      "date" => format_date(field, &raw)?,
      "enum" => format_enum(field, &raw, metadata)?,
      _ => FormattedValue::Text(json_to_text(&raw)),
   };

   Ok(DisplayField {
      label: field.label.clone(),
      value,
   })
}

fn format_address(raw: &Value, chain: u64, data: &FormatData) -> FormattedValue {
   if let Some(addr) = path::value_as_address(raw) {
      if let Some(name) = data.name(chain, addr) {
         return FormattedValue::Text(name.to_string());
      }
      return FormattedValue::Address(addr);
   }
   FormattedValue::Text(json_to_text(raw))
}

fn format_token_amount(
   field: &FieldSpec,
   raw: &Value,
   message: &Value,
   metadata: &Metadata,
   container: &Container,
   data: &FormatData,
) -> Result<FormattedValue, anyhow::Error> {
   let amount = path::value_as_u256(raw).ok_or_else(|| anyhow::anyhow!("not an integer"))?;
   let chain = token_chain(field, message, metadata, container);
   let token_addr = token_address(field, message, metadata, container);

   let unlimited = is_unlimited(field, amount, message, metadata, container);

   if let Some(addr) = token_addr {
      if let Some(token) = data.token(chain, addr) {
         let formatted = NumericValue::format_wei(amount, token.decimals);
         return Ok(FormattedValue::TokenAmount {
            amount: formatted,
            token: token.clone(),
            unlimited,
         });
      }
   }

   if unlimited {
      return Ok(FormattedValue::Text("Unlimited".to_string()));
   }
   Err(anyhow::anyhow!("Unknown token"))
}

fn format_date(field: &FieldSpec, raw: &Value) -> Result<FormattedValue, anyhow::Error> {
   let encoding = field.params.get("encoding").and_then(|v| v.as_str()).unwrap_or("timestamp");
   let n = path::value_as_u256(raw).ok_or_else(|| anyhow::anyhow!("not a timestamp"))?;
   let secs = u64::try_from(n).unwrap_or(0);
   let ts = match encoding {
      "blockheight" => TimeStamp::Seconds(secs),
      _ => TimeStamp::Seconds(secs),
   };
   Ok(FormattedValue::Date(ts))
}

fn format_enum(
   field: &FieldSpec,
   raw: &Value,
   metadata: &Metadata,
) -> Result<FormattedValue, anyhow::Error> {
   let key = json_to_text(raw);
   if let Some(r) = field.params.get("$ref").and_then(|v| v.as_str()) {
      if let Some(name) = r.strip_prefix("$.metadata.enums.") {
         if let Some(map) = metadata.enums.get(name) {
            if let Some(label) = map.get(&key) {
               return Ok(FormattedValue::Text(label.clone()));
            }
         }
      }
   }
   Ok(FormattedValue::Text(key))
}

fn token_address(
   field: &FieldSpec,
   message: &Value,
   metadata: &Metadata,
   container: &Container,
) -> Option<Address> {
   if let Some(p) = field.params.get("tokenPath").and_then(|v| v.as_str()) {
      let v = path::resolve_path(p, message, &metadata.constants, container)?;
      return path::value_as_address(&v);
   }
   if let Some(t) = field.params.get("token").and_then(|v| v.as_str()) {
      return t.parse().ok();
   }
   container.to
}

fn token_chain(
   field: &FieldSpec,
   message: &Value,
   metadata: &Metadata,
   container: &Container,
) -> u64 {
   if let Some(p) = field.params.get("chainIdPath").and_then(|v| v.as_str()) {
      if let Some(v) = path::resolve_path(p, message, &metadata.constants, container) {
         if let Some(n) = path::value_as_u256(&v) {
            return u64::try_from(n).unwrap_or(container.chain_id);
         }
      }
   }
   if let Some(n) = field.params.get("chainId").and_then(json_u64_ref) {
      return n;
   }
   container.chain_id
}

fn json_u64_ref(value: &Value) -> Option<u64> {
   path::value_as_u256(value).and_then(|n| u64::try_from(n).ok())
}

fn is_unlimited(
   field: &FieldSpec,
   amount: U256,
   message: &Value,
   metadata: &Metadata,
   container: &Container,
) -> bool {
   if amount == U256::MAX {
      return true;
   }
   let Some(th_val) = field.params.get("threshold") else {
      return false;
   };
   let resolved = match th_val {
      Value::String(s) if s.starts_with('$') || s.starts_with('#') || s.starts_with('@') => {
         path::resolve_path(s, message, &metadata.constants, container)
      }
      other => Some(other.clone()),
   };
   if let Some(th) = resolved.as_ref().and_then(path::value_as_u256) {
      return amount >= th;
   }
   false
}

fn interpolate(
   tmpl: &str,
   spec: &FormatSpec,
   formatted: &HashMap<String, String>,
) -> Result<String, ()> {
   let mut out = String::new();
   let mut rest = tmpl;
   while let Some(start) = rest.find('{') {
      let end = rest[start + 1..].find('}').ok_or(())? + start + 1;
      let path = rest[start + 1..end].trim();
      if !spec.fields.iter().any(|f| f.path == path || f.path == format!("#.{path}")) {
         return Err(());
      }
      let value = formatted.get(path).or_else(|| formatted.get(&format!("#.{path}"))).ok_or(())?;
      out.push_str(&rest[..start]);
      out.push_str(value);
      rest = &rest[end + 1..];
   }
   out.push_str(rest);
   Ok(out)
}

fn json_to_text(value: &Value) -> String {
   match value {
      Value::String(s) => s.clone(),
      Value::Number(n) => n.to_string(),
      Value::Bool(b) => b.to_string(),
      Value::Null => "null".to_string(),
      other => other.to_string(),
   }
}

/// ABI-decode calldata args (without selector) into a JSON object keyed by argument name.
pub fn decode_call_args(parsed: &ParsedCallFormat, data: &[u8]) -> Result<Value, anyhow::Error> {
   let tuple = zeus_eth::alloy_dyn_abi::DynSolType::Tuple(parsed.arg_types.clone());
   let decoded = tuple.abi_decode_params(data).map_err(|e| anyhow::anyhow!("{e}"))?;
   let values = match decoded {
      DynSolValue::Tuple(v) => v,
      other => vec![other],
   };
   if values.len() != parsed.arg_names.len() {
      return Err(anyhow::anyhow!("arg count mismatch"));
   }
   let mut map = Map::new();
   for (name, value) in parsed.arg_names.iter().zip(values.into_iter()) {
      map.insert(name.clone(), dyn_to_json(&value));
   }
   Ok(Value::Object(map))
}

fn dyn_to_json(value: &DynSolValue) -> Value {
   match value {
      DynSolValue::Bool(b) => Value::Bool(*b),
      DynSolValue::Uint(n, _) => Value::String(n.to_string()),
      DynSolValue::Int(n, _) => Value::String(n.to_string()),
      DynSolValue::Address(a) => Value::String(a.to_string()),
      DynSolValue::Bytes(b) => Value::String(format!("0x{}", hex::encode(b))),
      DynSolValue::FixedBytes(w, size) => Value::String(format!("0x{}", hex::encode(&w[..*size]))),
      DynSolValue::String(s) => Value::String(s.clone()),
      DynSolValue::Array(items) | DynSolValue::FixedArray(items) | DynSolValue::Tuple(items) => {
         Value::Array(items.iter().map(dyn_to_json).collect())
      }
      other => Value::String(format!("{other:?}")),
   }
}
