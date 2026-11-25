//! The invariants each tool descriptor keeps, checked over the whole table.
//!
//! A schema is hand-written data beside its handler, so the things that must never drift — a parameter
//! that could carry a secret, an open object where a closed one belongs, a capability the catalog does
//! not know, a list that is not sorted — are checked here rather than trusted.

use super::TOOLS;
use serde_json::Value;
use zeus_core::agent::capability;

/// The words no parameter name may contain, anywhere in a schema.
///
/// A tool that took a private key, a seed phrase or a password would put it in the model's context and in
/// the harness's transcript. The operations that need one open a window inside Zeus instead (D11), which
/// is why nothing here may name such a thing — and why this test exists to catch the next one.
const FORBIDDEN: &[&str] = &[
   "key",
   "seed",
   "mnemonic",
   "password",
   "passphrase",
   "phrase",
   "secret",
];

#[test]
fn no_tool_takes_a_secret_as_an_argument() {
   for spec in TOOLS {
      let schema = schema_of(spec.name, spec.schema);
      let mut names = Vec::new();
      property_names(&schema, &mut names);

      for name in names {
         let lowercase = name.to_lowercase();

         for word in FORBIDDEN {
            assert!(
               !lowercase.contains(word),
               "{}'s {name} reads like a secret parameter: nothing may carry one",
               spec.name
            );
         }
      }
   }
}

#[test]
fn every_schema_is_a_closed_object() {
   for spec in TOOLS {
      let schema = schema_of(spec.name, spec.schema);

      assert_eq!(
         schema["type"], "object",
         "{}'s arguments are not an object schema",
         spec.name
      );

      assert_eq!(
         schema["additionalProperties"], false,
         "{} accepts fields it does not declare: a typo would be ignored instead of refused",
         spec.name
      );

      // A required name the schema does not declare is a descriptor a harness cannot satisfy.
      if let Some(required) = schema["required"].as_array() {
         for entry in required {
            let name = entry.as_str().expect("a required name is a string");

            assert!(
               schema["properties"].get(name).is_some(),
               "{} requires {name}, which it does not declare",
               spec.name
            );
         }
      }
   }
}

#[test]
fn the_table_is_sorted_and_each_tool_is_named_once() {
   let names: Vec<&str> = TOOLS.iter().map(|spec| spec.name).collect();
   let mut sorted = names.clone();
   sorted.sort_unstable();

   assert_eq!(names, sorted, "tools/list is sorted by name");

   for pair in names.windows(2) {
      assert_ne!(pair[0], pair[1], "{} is listed twice", pair[0]);
   }

   for spec in TOOLS {
      assert!(
         !spec.name.is_empty()
            && spec
               .name
               .chars()
               .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
         "{} is not a snake_case tool name",
         spec.name
      );

      assert!(spec.title.len() > 3, "{} has no title", spec.name);
      assert!(
         spec.description.len() > 20,
         "{} has no description worth reading",
         spec.name
      );
   }
}

#[test]
fn every_tool_names_a_capability_the_catalog_knows() {
   for spec in TOOLS {
      if let Some(name) = spec.capability {
         assert!(
            capability(name).is_some(),
            "{} names {name}, which is not in the capability catalog",
            spec.name
         );
      }
   }
}

/// One tool's schema, parsed, with a failure that names the tool rather than a byte offset.
fn schema_of(name: &str, text: &str) -> Value {
   serde_json::from_str(text)
      .unwrap_or_else(|error| panic!("{name} has an unparseable schema: {error}"))
}

/// Every property name a schema declares, at any depth.
fn property_names(schema: &Value, names: &mut Vec<String>) {
   let Some(object) = schema.as_object() else {
      // `items` and the combinators hold a schema or a list of them.
      if let Some(items) = schema.as_array() {
         for item in items {
            property_names(item, names);
         }
      }

      return;
   };

   if let Some(Value::Object(properties)) = object.get("properties") {
      for (name, child) in properties {
         names.push(name.clone());
         property_names(child, names);
      }
   }

   for key in ["items", "oneOf", "anyOf", "allOf"] {
      if let Some(child) = object.get(key) {
         property_names(child, names);
      }
   }
}
