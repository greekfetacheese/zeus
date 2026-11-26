//! The edge must never touch key material.
//!
//! The grant is enforced in `zeus_core`, and a tool here holds a `ZeusCtx` — which holds the vault. Every
//! tool therefore works from the projections in `zeus_core::agent::read` and from the flows the binary
//! implements, and a wallet's key never crosses into a shape this crate can name. That is a property of
//! the code as it stands; this file is the tripwire that notices when a later change stops holding it, the
//! way `src/tools/schemas.rs` notices a secret-shaped parameter.
//!
//! It is a scan, not a proof. A determined caller can reach the vault through a method the list below does
//! not name — the point is that the names one *would* reach for are the ones it checks, so an appearance
//! is a question to answer in review rather than a bug on its own.

use std::fs;
use std::path::{Path, PathBuf};

/// The calls and type names that would put a key, or the vault it is sealed in, in this crate's hands.
const FORBIDDEN: &[&str] = &[
   "credentials(",
   ".seed()",
   "secure_key",
   "SecureKey",
   "read_vault",
   "write_vault",
   "get_wallet(",
];

/// The fewest source files a real scan finds, so a broken path fails loudly instead of passing empty.
const FEWEST_SOURCES: usize = 10;

#[test]
fn no_source_in_this_crate_touches_key_material() {
   let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
   let sources = sources(&crate_root.join("src"));

   assert!(
      sources.len() >= FEWEST_SOURCES,
      "the scan read {} files under {}: the path is wrong, and a scan of nothing passes",
      sources.len(),
      crate_root.display()
   );

   let mut found: Vec<String> = sources
      .iter()
      .flat_map(|(path, text)| {
         let relative = path.strip_prefix(crate_root).unwrap_or(path);

         offences(text)
            .into_iter()
            .map(move |(line, offence)| format!("{}:{line}: `{offence}`", relative.display()))
      })
      .collect();

   found.sort();

   assert!(
      found.is_empty(),
      "{} call(s) in this crate could reach key material, which it must never hold:\n  {}",
      found.len(),
      found.join("\n  ")
   );
}

#[test]
fn the_scan_finds_a_key_where_one_was_planted() {
   let planted = "let wallet = ctx.get_wallet(from)?.key;\nlet key = vault.read_vault();\n";

   assert_eq!(
      offences(planted),
      [(1, "get_wallet("), (2, "read_vault")],
      "the scan must notice the calls it exists to catch"
   );
}

#[test]
fn the_scan_leaves_an_ordinary_projection_alone() {
   let clean = "let row = read::wallet(ctx, profile, address)?;\nlet name = \"a wallet name\";\n";

   assert!(
      offences(clean).is_empty(),
      "a projection is not key material, and a scan that says it is would be switched off"
   );
}

/// Every forbidden name in `text`, with the line it appears on, first appearance of each name.
fn offences(text: &str) -> Vec<(usize, &'static str)> {
   let mut found = Vec::new();

   for name in FORBIDDEN {
      if let Some(index) = text.find(name) {
         let line = text[..index].matches('\n').count() + 1;
         found.push((line, *name));
      }
   }

   found.sort();
   found
}

/// Every `.rs` file under `dir`, with its text, in a stable order.
fn sources(dir: &Path) -> Vec<(PathBuf, String)> {
   let mut found = Vec::new();
   walk(dir, &mut found);
   found.sort();

   found
}

fn walk(dir: &Path, found: &mut Vec<(PathBuf, String)>) {
   let Ok(entries) = fs::read_dir(dir) else {
      return;
   };

   for entry in entries.flatten() {
      let path = entry.path();

      match path.is_dir() {
         true => walk(&path, found),
         false if path.extension().and_then(|extension| extension.to_str()) == Some("rs") => {
            if let Ok(text) = fs::read_to_string(&path) {
               found.push((path, text));
            }
         }
         false => {}
      }
   }
}
