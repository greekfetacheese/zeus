//! Catalog of everything Zeus writes under `data/`.
//!
//! Filenames and directory names live here — not as parallel `&str` constants
//! in the modules that happen to read or write them. Adding a persisted thing
//! means adding a variant; exhaustive matches (export policy, tree allow-rules,
//! optional-export flags) fail to compile until the new item is classified.
//!
//! `logs/` sits next to `data/` and is not part of this catalog.

use crate::utils::restrict_dir_to_owner;
use std::path::{Component, Path, PathBuf};

/// Directory name for the portable data tree (cwd-relative).
pub const DATA_DIR_NAME: &str = "data";

/// ERC-7730 index files cached under [`PersistedTree::ClearSigning`].
pub const CLEAR_SIGNING_INDEX_EIP712: &str = "index.eip712.json";
pub const CLEAR_SIGNING_INDEX_CALLDATA: &str = "index.calldata.json";

/// Downloaded token-icon basenames under [`PersistedTree::TokenIcons`].
pub const TOKEN_ICON_X32: &str = "x32.png";
pub const TOKEN_ICON_X24: &str = "x24.png";

macro_rules! persisted_files {
   ($($variant:ident => $name:literal),* $(,)?) => {
      /// A single known file at the root of `data/`.
      #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
      pub enum PersistedFile {
         $($variant,)*
      }

      impl PersistedFile {
         pub const ALL: &[Self] = &[$(Self::$variant,)*];

         pub const fn name(self) -> &'static str {
            match self {
               $(Self::$variant => $name,)*
            }
         }

         pub fn from_name(name: &str) -> Option<Self> {
            match name {
               $($name => Some(Self::$variant),)*
               _ => None,
            }
         }
      }
   };
}

macro_rules! persisted_trees {
   ($($variant:ident => $name:literal),* $(,)?) => {
      /// A known directory tree under `data/` with generated child names.
      #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
      pub enum PersistedTree {
         $($variant,)*
      }

      impl PersistedTree {
         pub const ALL: &[Self] = &[$(Self::$variant,)*];

         pub const fn dir_name(self) -> &'static str {
            match self {
               $(Self::$variant => $name,)*
            }
         }

         pub fn from_dir_name(name: &str) -> Option<Self> {
            match name {
               $($name => Some(Self::$variant),)*
               _ => None,
            }
         }
      }
   };
}

persisted_files! {
   Vault => "vault.data",
   WalletState => "wallet_state.data",
   Tokens => "tokens.data",
   PoolData => "pool_data.data",
   Providers => "providers.data",
   BundlerUrl => "bundler_url.data",
   AddressBook => "address_book.data",
   PriceData => "price_data.json",
   Theme => "theme.json",
   ServerPort => "server_port.json",
   DisabledChains => "disabled_chains.json",
   RailgunConfig => "railgun_config.json",
   AcrossSettings => "across_settings.json",
   Connector => "connector.json",
   NativeHostManifest => "io.github.zeus_wallet.json",
   NativeHostWrapperUnix => "zeus_connector_host.sh",
   NativeHostWrapperWindows => "zeus_connector_host.cmd",
}

persisted_trees! {
   Railgun => "railgun",
   ClearSigning => "clear_signing",
   TokenIcons => "token_icons",
}

/// Everything Zeus persists under `data/`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Persisted {
   File(PersistedFile),
   Tree(PersistedTree),
}

/// How a catalog entry is treated by export / import.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportPolicy {
   /// Packed if present; allowed in an import zip.
   Core,
   /// Packed only when the matching [`ExportOptions`] flag is set.
   Optional,
   /// Written at runtime, never backed up (`connector.json`, native-host files).
   Never,
}

impl Persisted {
   pub const fn export_policy(self) -> ExportPolicy {
      match self {
         Self::File(
            PersistedFile::Vault
            | PersistedFile::WalletState
            | PersistedFile::Tokens
            | PersistedFile::PoolData
            | PersistedFile::Providers
            | PersistedFile::BundlerUrl
            | PersistedFile::AddressBook
            | PersistedFile::PriceData
            | PersistedFile::Theme
            | PersistedFile::ServerPort
            | PersistedFile::DisabledChains
            | PersistedFile::RailgunConfig
            | PersistedFile::AcrossSettings,
         ) => ExportPolicy::Core,
         Self::File(
            PersistedFile::Connector
            | PersistedFile::NativeHostManifest
            | PersistedFile::NativeHostWrapperUnix
            | PersistedFile::NativeHostWrapperWindows,
         ) => ExportPolicy::Never,
         Self::Tree(_) => ExportPolicy::Optional,
      }
   }
}

impl PersistedFile {
   pub const fn export_policy(self) -> ExportPolicy {
      Persisted::File(self).export_policy()
   }
}

impl PersistedTree {
   pub const fn export_policy(self) -> ExportPolicy {
      Persisted::Tree(self).export_policy()
   }

   /// Checkbox copy in the export UI. Required for every tree so a new optional
   /// tree cannot ship without a label.
   pub const fn export_label(self) -> &'static str {
      match self {
         Self::Railgun => "Include Railgun data",
         Self::ClearSigning => "Include clear signing cache",
         Self::TokenIcons => "Include downloaded token icons",
      }
   }

   /// Whether `rel` (relative to this tree's directory) is a known child path.
   pub fn allows_rel(self, rel: &Path) -> bool {
      match self {
         Self::Railgun => {
            let Some(name) = single_normal_name(rel) else {
               return false;
            };
            is_allowed_railgun_file(name)
         }
         Self::ClearSigning => {
            let Some(name) = single_normal_name(rel) else {
               return false;
            };
            is_allowed_clear_signing_file(name)
         }
         Self::TokenIcons => is_allowed_token_icon_rel(rel),
      }
   }
}

/// Optional trees the user can add on top of the core files.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExportOptions {
   pub clear_signing: bool,
   pub railgun: bool,
   pub token_icons: bool,
}

impl ExportOptions {
   pub fn include_tree(self, tree: PersistedTree) -> bool {
      match tree {
         PersistedTree::Railgun => self.railgun,
         PersistedTree::ClearSigning => self.clear_signing,
         PersistedTree::TokenIcons => self.token_icons,
      }
   }

   pub fn flag_mut(&mut self, tree: PersistedTree) -> &mut bool {
      match tree {
         PersistedTree::Railgun => &mut self.railgun,
         PersistedTree::ClearSigning => &mut self.clear_signing,
         PersistedTree::TokenIcons => &mut self.token_icons,
      }
   }
}

/// Zeus `data/` directory (cwd-relative). Creates it if missing.
pub fn data_dir() -> Result<PathBuf, anyhow::Error> {
   let dir = std::env::current_dir()?.join(DATA_DIR_NAME);

   if !dir.exists() {
      std::fs::create_dir_all(dir.clone())?;
   }

   if let Err(e) = restrict_dir_to_owner(&dir) {
      tracing::warn!("Failed to restrict data/ permissions: {e}");
   }

   Ok(dir)
}

pub fn file_path(file: PersistedFile) -> Result<PathBuf, anyhow::Error> {
   Ok(data_dir()?.join(file.name()))
}

/// Tree directory under `data/`. Creates it if missing.
pub fn tree_dir(tree: PersistedTree) -> Result<PathBuf, anyhow::Error> {
   let dir = data_dir()?.join(tree.dir_name());
   if !dir.exists() {
      std::fs::create_dir_all(&dir)?;
   }
   match tree {
      PersistedTree::Railgun => {
         if let Err(e) = restrict_dir_to_owner(&dir) {
            tracing::warn!(
               "Failed to restrict data/{}/ permissions: {e}",
               tree.dir_name()
            );
         }
      }
      PersistedTree::ClearSigning | PersistedTree::TokenIcons => {}
   }
   Ok(dir)
}

pub fn railgun_dir() -> Result<PathBuf, anyhow::Error> {
   tree_dir(PersistedTree::Railgun)
}

pub fn railgun_db_file(chain: u64) -> Result<PathBuf, anyhow::Error> {
   Ok(railgun_dir()?.join(format!("railgun:{}.db", chain)))
}

pub fn theme_kind_dir() -> Result<PathBuf, anyhow::Error> {
   file_path(PersistedFile::Theme)
}

pub fn server_port_dir() -> Result<PathBuf, anyhow::Error> {
   file_path(PersistedFile::ServerPort)
}

pub fn disabled_chains_dir() -> Result<PathBuf, anyhow::Error> {
   file_path(PersistedFile::DisabledChains)
}

pub fn railgun_config_dir() -> Result<PathBuf, anyhow::Error> {
   file_path(PersistedFile::RailgunConfig)
}

pub fn pool_data_dir() -> Result<PathBuf, anyhow::Error> {
   file_path(PersistedFile::PoolData)
}

pub fn bundler_url_dir() -> Result<PathBuf, anyhow::Error> {
   file_path(PersistedFile::BundlerUrl)
}

pub fn is_allowed_clear_signing_file(name: &str) -> bool {
   name == CLEAR_SIGNING_INDEX_EIP712
      || name == CLEAR_SIGNING_INDEX_CALLDATA
      || is_keccak256_json(name)
}

pub fn is_allowed_railgun_file(name: &str) -> bool {
   if let Some(rest) = name.strip_prefix("railgun:") {
      return rest.strip_suffix(".db").is_some_and(is_chain_id);
   }
   if let Some(rest) = name.strip_prefix("events-snapshot:") {
      return rest.strip_suffix(".data").is_some_and(is_chain_id)
         || rest.strip_suffix(".meta").is_some_and(is_chain_id);
   }
   false
}

pub fn is_allowed_token_icon_rel(rel: &Path) -> bool {
   let Some(parts) = normal_components(rel) else {
      return false;
   };
   if parts.len() != 3 {
      return false;
   }
   is_chain_id(&parts[0]) && is_token_address_dir(&parts[1]) && is_icon_file(&parts[2])
}

/// Whether `parts` (relative to `data/`) is a known persisted path that may
/// appear in an export zip.
pub fn is_allowed_rel_parts(parts: &[String]) -> bool {
   match parts {
      [name] => {
         PersistedFile::from_name(name).is_some_and(|f| f.export_policy() != ExportPolicy::Never)
      }
      [dir, rest @ ..] => {
         let Some(tree) = PersistedTree::from_dir_name(dir) else {
            return false;
         };
         if tree.export_policy() == ExportPolicy::Never {
            return false;
         }
         let rel: PathBuf = rest.iter().collect();
         tree.allows_rel(&rel)
      }
      _ => false,
   }
}

pub(crate) fn normal_components(path: &Path) -> Option<Vec<String>> {
   let mut parts = Vec::new();
   for component in path.components() {
      match component {
         Component::Normal(s) => {
            let s = s.to_str()?;
            if s.is_empty() || s == "." || s == ".." {
               return None;
            }
            parts.push(s.to_string());
         }
         Component::CurDir => {}
         _ => return None,
      }
   }
   Some(parts)
}

fn single_normal_name(rel: &Path) -> Option<&str> {
   let mut comps = rel.components();
   let Component::Normal(name) = comps.next()? else {
      return None;
   };
   if comps.next().is_some() {
      return None;
   }
   let name = name.to_str()?;
   if name.is_empty() || name == "." || name == ".." {
      return None;
   }
   Some(name)
}

fn is_chain_id(s: &str) -> bool {
   !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

fn is_keccak256_json(name: &str) -> bool {
   let Some(stem) = name.strip_suffix(".json") else {
      return false;
   };
   stem.len() == 64 && stem.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_token_address_dir(s: &str) -> bool {
   let Some(hex) = s.strip_prefix("0x") else {
      return false;
   };
   hex.len() == 40 && hex.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_icon_file(name: &str) -> bool {
   name == TOKEN_ICON_X32 || name == TOKEN_ICON_X24
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn file_names_are_unique() {
      let mut names: Vec<_> = PersistedFile::ALL.iter().map(|f| f.name()).collect();
      names.sort_unstable();
      names.dedup();
      assert_eq!(names.len(), PersistedFile::ALL.len());
   }

   #[test]
   fn tree_dir_names_are_unique() {
      let mut names: Vec<_> = PersistedTree::ALL.iter().map(|t| t.dir_name()).collect();
      names.sort_unstable();
      names.dedup();
      assert_eq!(names.len(), PersistedTree::ALL.len());
   }

   #[test]
   fn core_files_include_vault_and_wallet_state() {
      let names: Vec<_> = PersistedFile::ALL
         .iter()
         .copied()
         .filter(|f| f.export_policy() == ExportPolicy::Core)
         .map(PersistedFile::name)
         .collect();
      assert!(names.contains(&"vault.data"));
      assert!(names.contains(&"wallet_state.data"));
      assert!(!names.contains(&"connector.json"));
   }

   #[test]
   fn machine_local_files_are_never_exported() {
      for file in [
         PersistedFile::Connector,
         PersistedFile::NativeHostManifest,
         PersistedFile::NativeHostWrapperUnix,
         PersistedFile::NativeHostWrapperWindows,
      ] {
         assert_eq!(file.export_policy(), ExportPolicy::Never);
         assert!(!is_allowed_rel_parts(&[file.name().to_string()]));
      }
   }

   #[test]
   fn from_name_roundtrips() {
      for file in PersistedFile::ALL {
         assert_eq!(PersistedFile::from_name(file.name()), Some(*file));
      }
      assert!(PersistedFile::from_name("secret.txt").is_none());
   }
}
