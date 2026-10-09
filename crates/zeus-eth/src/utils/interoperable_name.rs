//! ERC-7828 *Interoperable Names*: `<address>@<chain>[#<checksum>]`.
//!
//! **Policy: every ENS lookup goes through [`ens`].** This module never calls an `alloy-ens`
//! helper itself, so the onchain-only, mainnet-only policy — the refusing CCIP gateway, the one
//! mainnet client — stays enforced in a single file and cannot be bypassed from here. The
//! ERC-7930 codec, the text grammar and the checksum are pure; only [`resolve`] does I/O.
//!
//! The binary half is ERC-7930, whose components are
//! `Version | ChainType | ChainReferenceLength | ChainReference | AddressLength | Address`.
//! ERC-7828 only adds the human-readable envelope around it and defines the checksum over those
//! fields with the `Version` excluded.

use super::ens;
use alloy_network::Ethereum;
use alloy_primitives::{Address, keccak256};
use alloy_provider::Provider;

/// ERC-7930 `Version` this module writes and accepts. Version 1 is the only one defined.
const VERSION: u16 = 1;

/// ERC-7930 `ChainType` for EVM chains. Every chain Zeus supports is `0x0000`.
const CHAIN_TYPE_EVM: u16 = 0x0000;

/// A parsed ERC-7828 *Interoperable Name*.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteroperableName {
   pub address: AddressPart,
   pub chain: ChainPart,
   /// Present only when the input carried `#<checksum>`.
   pub checksum: Option<[u8; 4]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddressPart {
   /// An ENS name (contains a `.`), already normalized.
   Name(String),
   /// A raw `0x` address.
   Raw(Address),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChainPart {
   /// A CAIP-2/CAIP-350 EVM chain identifier (`eip155:8453`) — needs no ENS lookup.
   Caip2 { chain_id: u64 },
   /// A label under the `on.eth` namespace (`base` → `base.on.eth`), already normalized.
   Label(String),
}

/// Why a string is not a usable *Interoperable Name*.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteroperableNameError {
   /// No single `@`, or more than one.
   NotAnInteroperableName,
   /// Nothing before the `@`.
   MissingAddress,
   /// Nothing after the `@`.
   MissingChain,
   /// The address part is neither a name (no `.`) nor a `0x` address.
   InvalidAddress,
   /// The name did not survive [`ens::normalize_name`].
   InvalidName,
   /// The chain label did not survive [`ens::normalize_name`].
   InvalidChainLabel,
   /// The chain part used a namespace other than `eip155`.
   UnsupportedChainNamespace(String),
   /// The chain reference is not a number.
   InvalidChainId(String),
   /// The checksum is not 8 hex characters.
   InvalidChecksum(String),
   /// A checksum was supplied for an ENS name. The hash covers address bytes, so it is only
   /// defined for a raw address.
   ChecksumWithoutAddress,
   /// The checksum does not match the address and chain.
   ChecksumMismatch { expected: [u8; 4], found: [u8; 4] },
   /// The ERC-7930 envelope is too short, or a length field runs past the end.
   MalformedEnvelope,
   /// The ERC-7930 version is not [`VERSION`].
   UnsupportedVersion(u16),
   /// The ERC-7930 chain type is not an EVM chain.
   NotAnEvmChain(u16),
   /// A lookup could not be performed at all (transport, revert, refused offchain gateway).
   ResolveFailed(String),
}

impl std::fmt::Display for InteroperableNameError {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      match self {
         InteroperableNameError::NotAnInteroperableName => {
            write!(f, "not an Interoperable Name")
         }
         InteroperableNameError::MissingAddress => write!(f, "no address before '@'"),
         InteroperableNameError::MissingChain => write!(f, "no chain after '@'"),
         InteroperableNameError::InvalidAddress => write!(f, "not a name or a 0x address"),
         InteroperableNameError::InvalidName => write!(f, "invalid ENS name"),
         InteroperableNameError::InvalidChainLabel => write!(f, "invalid chain label"),
         InteroperableNameError::UnsupportedChainNamespace(namespace) => {
            write!(f, "unsupported chain namespace {:?}", namespace)
         }
         InteroperableNameError::InvalidChainId(reference) => {
            write!(f, "invalid chain id {:?}", reference)
         }
         InteroperableNameError::InvalidChecksum(checksum) => {
            write!(f, "invalid checksum {:?}", checksum)
         }
         InteroperableNameError::ChecksumWithoutAddress => {
            write!(f, "a checksum is only defined for a raw address")
         }
         InteroperableNameError::ChecksumMismatch { expected, found } => write!(
            f,
            "checksum mismatch: expected {}, found {}",
            format_checksum(expected),
            format_checksum(found)
         ),
         InteroperableNameError::MalformedEnvelope => write!(f, "malformed ERC-7930 envelope"),
         InteroperableNameError::UnsupportedVersion(version) => {
            write!(f, "unsupported ERC-7930 version {}", version)
         }
         InteroperableNameError::NotAnEvmChain(chain_type) => {
            write!(
               f,
               "ERC-7930 chain type {:#06x} is not an EVM chain",
               chain_type
            )
         }
         InteroperableNameError::ResolveFailed(reason) => {
            write!(f, "could not resolve: {}", reason)
         }
      }
   }
}

impl std::error::Error for InteroperableNameError {}

// ---------------------------------------------------------------------------------------------
// ERC-7930 codec
// ---------------------------------------------------------------------------------------------

/// Raw bytes of the ERC-7930 *Interoperable Address* for a chain-only identifier
/// (`AddressLength` = 0) — the encoding `on.eth` publishes for a chain label.
///
/// The layout is `Version(2) | ChainType(2) | ChainReferenceLength(1) | ChainReference(N) |
/// AddressLength(1) | Address(M)`; both length fields are a single byte.
///
/// `encode_chain(1)` is `0x00010000010100` and `encode_chain(8453)` is `0x00010000022105`, which
/// is byte-for-byte what the mainnet `on.eth` registry returns.
pub fn encode_chain(chain_id: u64) -> Vec<u8> {
   let reference = minimal_be(chain_id);

   let mut out = Vec::with_capacity(5 + reference.len());
   out.extend_from_slice(&VERSION.to_be_bytes());
   out.extend_from_slice(&CHAIN_TYPE_EVM.to_be_bytes());
   out.push(reference.len() as u8);
   out.extend_from_slice(&reference);
   out.push(0);
   out
}

/// Chain id out of an ERC-7930 *Interoperable Address*.
///
/// Only EVM chain identifiers are accepted: a non-`0x0000` `ChainType` is a chain Zeus cannot send
/// to, and a `ChainReference` wider than 8 bytes cannot be an EIP-155 chain id.
pub fn decode_chain_id(bytes: &[u8]) -> Result<u64, InteroperableNameError> {
   if bytes.len() < 5 {
      return Err(InteroperableNameError::MalformedEnvelope);
   }

   let version = u16::from_be_bytes([bytes[0], bytes[1]]);
   if version != VERSION {
      return Err(InteroperableNameError::UnsupportedVersion(
         version,
      ));
   }

   let chain_type = u16::from_be_bytes([bytes[2], bytes[3]]);
   if chain_type != CHAIN_TYPE_EVM {
      return Err(InteroperableNameError::NotAnEvmChain(chain_type));
   }

   let reference_len = bytes[4] as usize;
   let reference = bytes
      .get(5..5 + reference_len)
      .ok_or(InteroperableNameError::MalformedEnvelope)?;

   if reference.len() > 8 {
      return Err(InteroperableNameError::MalformedEnvelope);
   }

   let mut id = [0u8; 8];
   id[8 - reference.len()..].copy_from_slice(reference);
   Ok(u64::from_be_bytes(id))
}

/// Minimal big-endian encoding of a value (`1` → `01`, `8453` → `2105`, `0` → empty).
fn minimal_be(value: u64) -> Vec<u8> {
   let full = value.to_be_bytes();
   let first = full.iter().position(|b| *b != 0).unwrap_or(full.len());
   full[first..].to_vec()
}

// ---------------------------------------------------------------------------------------------
// Checksum
// ---------------------------------------------------------------------------------------------

/// ERC-7828 checksum: the first 4 bytes of `keccak256` over the ERC-7930 fields with the `Version`
/// field **excluded** — `ChainType | ChainReferenceLength | ChainReference | AddressLength |
/// Address`.
///
/// The `Version` is deliberately outside the hash so the checksum survives an ERC-7930 version
/// bump. Verified against the EIP's own example: chain 1 + `0xFe89…44b7` ⇒ `80B12379`.
pub fn checksum(chain_id: u64, address: Address) -> [u8; 4] {
   let reference = minimal_be(chain_id);

   let mut hashed = Vec::with_capacity(5 + reference.len() + 20);
   hashed.extend_from_slice(&CHAIN_TYPE_EVM.to_be_bytes());
   hashed.push(reference.len() as u8);
   hashed.extend_from_slice(&reference);
   hashed.push(20);
   hashed.extend_from_slice(address.as_slice());

   let digest = keccak256(&hashed);
   [digest[0], digest[1], digest[2], digest[3]]
}

/// Checksum as the 8 uppercase hex characters ERC-7828 prints.
pub fn format_checksum(checksum: &[u8; 4]) -> String {
   checksum.iter().map(|b| format!("{:02X}", b)).collect()
}

// ---------------------------------------------------------------------------------------------
// The text grammar
// ---------------------------------------------------------------------------------------------

/// Parse `<address>@<chain>[#<checksum>]`.
///
/// `@` cannot appear in an ENS name or a normalized label, so a single `@` is an unambiguous
/// separator. Anything that is not a well-formed *Interoperable Name* is an error rather than a
/// best-effort interpretation: the caller falls back to the plain-name path, so a wrong guess here
/// would be worse than a rejection.
pub fn parse(input: &str) -> Result<InteroperableName, InteroperableNameError> {
   let input = input.trim();

   let (address_part, rest) =
      input.split_once('@').ok_or(InteroperableNameError::NotAnInteroperableName)?;

   // A second `@` means this is not an Interoperable Name (`foo@bar@baz`).
   if rest.contains('@') {
      return Err(InteroperableNameError::NotAnInteroperableName);
   }

   let (chain_part, checksum_part) = match rest.split_once('#') {
      Some((chain, checksum)) => (chain, Some(checksum)),
      None => (rest, None),
   };

   let address = parse_address(address_part)?;
   let chain = parse_chain(chain_part)?;
   let checksum = checksum_part.map(parse_checksum).transpose()?;

   // The checksum hashes the address bytes, so it is undefined for a name — and silently ignoring
   // one the user pasted would be worse than refusing the input.
   if checksum.is_some() && !matches!(address, AddressPart::Raw(_)) {
      return Err(InteroperableNameError::ChecksumWithoutAddress);
   }

   Ok(InteroperableName {
      address,
      chain,
      checksum,
   })
}

/// `.` decides name vs address, exactly as ERC-7828 specifies.
fn parse_address(part: &str) -> Result<AddressPart, InteroperableNameError> {
   if part.is_empty() {
      return Err(InteroperableNameError::MissingAddress);
   }

   if part.contains('.') {
      let name = ens::normalize_name(part).map_err(|_| InteroperableNameError::InvalidName)?;
      return Ok(AddressPart::Name(name));
   }

   // Require the `0x` prefix explicitly: a bare 40-hex string is not what ERC-7828 shows, and
   // `Address::from_str` is only local (alloy-primitives has no `ens` feature here) — but being
   // explicit keeps the parse independent of that.
   let hex = part
      .strip_prefix("0x")
      .or_else(|| part.strip_prefix("0X"))
      .ok_or(InteroperableNameError::InvalidAddress)?;

   if hex.len() != 40 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
      return Err(InteroperableNameError::InvalidAddress);
   }

   part
      .parse::<Address>()
      .map(AddressPart::Raw)
      .map_err(|_| InteroperableNameError::InvalidAddress)
}

/// A colon means CAIP-2 (`eip155:<id>`); anything else is a label under `on.eth`.
fn parse_chain(part: &str) -> Result<ChainPart, InteroperableNameError> {
   if part.is_empty() {
      return Err(InteroperableNameError::MissingChain);
   }

   if let Some((namespace, reference)) = part.split_once(':') {
      if !namespace.eq_ignore_ascii_case("eip155") {
         return Err(InteroperableNameError::UnsupportedChainNamespace(
            namespace.to_string(),
         ));
      }

      let chain_id: u64 = reference
         .parse()
         .map_err(|_| InteroperableNameError::InvalidChainId(reference.to_string()))?;

      return Ok(ChainPart::Caip2 { chain_id });
   }

   // Normalized like a name: the label becomes `<label>.on.eth`, and alloy-ens hashes whatever it
   // is given, so an un-normalized label would resolve a *different* chain.
   let label = ens::normalize_name(part).map_err(|_| InteroperableNameError::InvalidChainLabel)?;
   Ok(ChainPart::Label(label))
}

/// ERC-7828 writes the checksum uppercase; accept either case when reading.
fn parse_checksum(part: &str) -> Result<[u8; 4], InteroperableNameError> {
   if part.len() != 8 || !part.bytes().all(|b| b.is_ascii_hexdigit()) {
      return Err(InteroperableNameError::InvalidChecksum(
         part.to_string(),
      ));
   }

   let mut checksum = [0u8; 4];
   for (index, byte) in checksum.iter_mut().enumerate() {
      *byte = u8::from_str_radix(&part[index * 2..index * 2 + 2], 16)
         .map_err(|_| InteroperableNameError::InvalidChecksum(part.to_string()))?;
   }
   Ok(checksum)
}

/// Is this search-bar text worth an ERC-7828 round-trip?
///
/// Allocation-free and cheap on purpose, like [`ens::looks_like_name`]: it runs on the egui frame
/// path for every keystroke. [`parse`] stays the authority on validity — this only decides whether
/// to spend an RPC call. It rejects plain names (no `@`), a half-typed pair, and text whose address
/// part is neither a name nor a `0x` address.
pub fn looks_like_interoperable_name(query: &str) -> bool {
   let query = query.trim();

   let Some((address, chain)) = query.split_once('@') else {
      return false;
   };

   if address.is_empty() || chain.is_empty() || chain.contains('@') {
      return false;
   }

   // Same discrimination `parse` makes, without allocating: a name has a dot, a raw EVM address is
   // 42 characters starting with `0x`.
   let address_ok = (address.contains('.') && address.len() >= 3)
      || (address.len() == 42 && (address.starts_with("0x") || address.starts_with("0X")));

   if !address_ok {
      return false;
   }

   address
      .bytes()
      .chain(chain.bytes())
      .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b':' | b'#'))
}

// ---------------------------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------------------------

/// A resolved *Interoperable Name*: the address is what gets sent, the chain says where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
   /// The ENS name when the address part was a name; `None` for a raw address.
   pub name: Option<String>,
   pub address: Address,
   pub chain_id: u64,
   /// True when the address came from the name's ENSIP-19 *default EVM chain* record instead of a
   /// record set for this chain — a weaker claim the UI should surface.
   pub from_default_evm_record: bool,
   /// The name's registration expiry, when the address part was a name. Chain-independent: the
   /// same registration whichever coin type answered. `None` for a raw address, or a name with no
   /// onchain expiry (non-`.eth`).
   pub expiry: Option<ens::NameExpiry>,
}

/// Reject a checksum that does not match the address and chain.
///
/// A mismatch is an error rather than a warning: the address is still usable, but the user asked
/// Zeus to check it and the check failed, which is the one thing a checksum exists to catch.
fn verify(
   claimed: Option<[u8; 4]>,
   chain_id: u64,
   address: &Address,
) -> Result<(), InteroperableNameError> {
   let Some(claimed) = claimed else {
      return Ok(());
   };

   let expected = checksum(chain_id, *address);

   if expected == claimed {
      return Ok(());
   }

   Err(InteroperableNameError::ChecksumMismatch {
      expected,
      found: claimed,
   })
}

/// Resolve an *Interoperable Name*, onchain only.
///
/// An `eip155:<id>` chain part needs no ENS call, so `0x…@eip155:8453` costs zero round-trips. A
/// label chain part costs one (`<label>.on.eth`), and a name address part one more.
///
/// `Ok(None)` means "nothing to offer": the chain label is unknown, or the name has no address for
/// that chain (and no default EVM address). `Err` means the input was unusable or a lookup failed.
pub async fn resolve<P>(client: &P, input: &str) -> Result<Option<Resolved>, InteroperableNameError>
where
   P: Provider<Ethereum>,
{
   let parsed = parse(input)?;

   let chain_id = match &parsed.chain {
      ChainPart::Caip2 { chain_id } => *chain_id,
      ChainPart::Label(label) => {
         match ens::resolve_chain_label(client, label)
            .await
            .map_err(|e| InteroperableNameError::ResolveFailed(e.to_string()))?
         {
            Some(chain_id) => chain_id,
            None => return Ok(None),
         }
      }
   };

   match &parsed.address {
      // A raw address needs no lookup at all — which also makes the checksum verifiable here.
      AddressPart::Raw(address) => {
         verify(parsed.checksum, chain_id, address)?;

         Ok(Some(Resolved {
            name: None,
            address: *address,
            chain_id,
            from_default_evm_record: false,
            expiry: None,
         }))
      }
      AddressPart::Name(name) => {
         let Some(resolved) = ens::resolve_name_for_chain(client, name, chain_id)
            .await
            .map_err(|e| InteroperableNameError::ResolveFailed(e.to_string()))?
         else {
            return Ok(None);
         };

         Ok(Some(Resolved {
            name: Some(name.clone()),
            address: resolved.address,
            chain_id,
            from_default_evm_record: resolved.from_default_evm_record,
            expiry: resolved.expiry,
         }))
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_primitives::address;

   #[test]
   fn encodes_evm_chain_identifiers_like_the_registry_does() {
      // Byte-for-byte what `ethereum.on.eth` / `base.on.eth` returned on mainnet.
      assert_eq!(
         encode_chain(1),
         vec![0x00, 0x01, 0x00, 0x00, 0x01, 0x01, 0x00]
      );
      assert_eq!(
         encode_chain(8453),
         vec![0x00, 0x01, 0x00, 0x00, 0x02, 0x21, 0x05, 0x00]
      );
   }

   #[test]
   fn decodes_chain_identifiers() {
      assert_eq!(
         decode_chain_id(&[0x00, 0x01, 0x00, 0x00, 0x01, 0x01, 0x00]).unwrap(),
         1
      );
      assert_eq!(
         decode_chain_id(&[0x00, 0x01, 0x00, 0x00, 0x02, 0x21, 0x05, 0x00]).unwrap(),
         8453
      );
      // A full interoperable address (with an Address component) still yields its chain.
      let full = decode_chain_id(&[
         0x00, 0x01, 0x00, 0x00, 0x01, 0x01, 0x14, 0xd8, 0xda, 0x6b, 0xf2, 0x69, 0x64, 0xaf, 0x9d,
         0x7e, 0xed, 0x9e, 0x03, 0xe5, 0x34, 0x15, 0xd3, 0x7a, 0xa9, 0x60, 0x45,
      ])
      .unwrap();
      assert_eq!(full, 1);
   }

   #[test]
   fn rejects_non_evm_and_malformed_envelopes() {
      // ChainType 0x0002 is Solana — Zeus has nothing to do with it.
      assert_eq!(
         decode_chain_id(&[0x00, 0x01, 0x00, 0x02, 0x20, 0x00]),
         Err(InteroperableNameError::NotAnEvmChain(0x0002))
      );
      assert_eq!(
         decode_chain_id(&[0x00, 0x02, 0x00, 0x00, 0x01, 0x01, 0x00]),
         Err(InteroperableNameError::UnsupportedVersion(2))
      );
      assert_eq!(
         decode_chain_id(&[0x00, 0x01, 0x00]),
         Err(InteroperableNameError::MalformedEnvelope)
      );
      // A 9-byte ChainReference cannot be a u64 chain id.
      assert_eq!(
         decode_chain_id(&[
            0x00, 0x01, 0x00, 0x00, 0x09, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0x00
         ]),
         Err(InteroperableNameError::MalformedEnvelope)
      );
      // A length field that runs past the end.
      assert_eq!(
         decode_chain_id(&[0x00, 0x01, 0x00, 0x00, 0x04, 0x01, 0x00]),
         Err(InteroperableNameError::MalformedEnvelope)
      );
   }

   #[test]
   fn checksum_matches_the_eip_fixture() {
      // ERC-7828: 0xFe89cc7aBB2C4183683ab71653C4cdc9B02D44b7@eip155:1#80B12379
      let address = address!("Fe89cc7aBB2C4183683ab71653C4cdc9B02D44b7");
      assert_eq!(checksum(1, address), [0x80, 0xB1, 0x23, 0x79]);
      assert_eq!(format_checksum(&checksum(1, address)), "80B12379");
   }

   #[test]
   fn parses_the_specs_forms() {
      let parsed = parse("vitalik.eth@base").unwrap();
      assert_eq!(
         parsed.address,
         AddressPart::Name("vitalik.eth".into())
      );
      assert_eq!(parsed.chain, ChainPart::Label("base".into()));
      assert_eq!(parsed.checksum, None);

      let parsed = parse("vitalik.eth@eip155:8453").unwrap();
      assert_eq!(parsed.chain, ChainPart::Caip2 { chain_id: 8453 });

      let parsed = parse("0xFe89cc7aBB2C4183683ab71653C4cdc9B02D44b7@eip155:1#80B12379").unwrap();
      assert_eq!(parsed.chain, ChainPart::Caip2 { chain_id: 1 });
      assert_eq!(parsed.checksum, Some([0x80, 0xB1, 0x23, 0x79]));
      assert_eq!(
         parsed.address,
         AddressPart::Raw(address!(
            "Fe89cc7aBB2C4183683ab71653C4cdc9B02D44b7"
         ))
      );

      // The `ethereum` label form of the same identity.
      let parsed = parse("0xFe89cc7aBB2C4183683ab71653C4cdc9B02D44b7@ethereum#80B12379").unwrap();
      assert_eq!(parsed.chain, ChainPart::Label("ethereum".into()));

      // Surrounding whitespace is tolerated (people paste).
      assert!(parse("  vitalik.eth@base  ").is_ok());
   }

   #[test]
   fn normalizes_names_and_labels_before_they_can_be_hashed() {
      // The same ENSIP-15 rule the plain-name path already enforces: alloy-ens hashes what it is
      // given, so an un-normalized name would silently hash to a different one.
      assert_eq!(
         parse("Vitalik.ETH@BASE").unwrap().address,
         AddressPart::Name("vitalik.eth".into())
      );
      assert_eq!(
         parse("vitalik.eth@BASE").unwrap().chain,
         ChainPart::Label("base".into())
      );

      // Non-ASCII is rejected outright (homographs).
      assert_eq!(
         parse("vital\u{456}k.eth@base"),
         Err(InteroperableNameError::InvalidName)
      );
      assert_eq!(
         parse("vitalik.eth@b\u{456}se"),
         Err(InteroperableNameError::InvalidChainLabel)
      );
   }

   #[test]
   fn rejects_what_is_not_an_interoperable_name() {
      // No chain: a plain name, handled by the existing path.
      assert_eq!(
         parse("vitalik.eth"),
         Err(InteroperableNameError::NotAnInteroperableName)
      );
      assert_eq!(
         parse("vitalik.eth@"),
         Err(InteroperableNameError::MissingChain)
      );
      assert_eq!(
         parse("@base"),
         Err(InteroperableNameError::MissingAddress)
      );
      assert_eq!(
         parse("vitalik.eth@base@gmx"),
         Err(InteroperableNameError::NotAnInteroperableName)
      );
      // Walletbeat's shorthand is not valid ERC-7828: `user` is neither a name nor an address.
      assert_eq!(
         parse("user@l2chain.eth"),
         Err(InteroperableNameError::InvalidAddress)
      );
      // `user.eth:l2chain` is ERC-7831 notation, also not ERC-7828.
      assert_eq!(
         parse("user.eth:l2chain"),
         Err(InteroperableNameError::NotAnInteroperableName)
      );
      assert_eq!(
         parse("vitalik.eth@eip155:8453#80B1237"),
         Err(InteroperableNameError::InvalidChecksum(
            "80B1237".into()
         ))
      );
      assert_eq!(
         parse("vitalik.eth@eip155:notanumber"),
         Err(InteroperableNameError::InvalidChainId(
            "notanumber".into()
         ))
      );
      assert_eq!(
         parse("vitalik.eth@eip155:"),
         Err(InteroperableNameError::InvalidChainId(
            String::new()
         ))
      );
      assert_eq!(
         parse("vitalik.eth@solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp"),
         Err(InteroperableNameError::UnsupportedChainNamespace(
            "solana".into()
         ))
      );
      // Not a 0x address, and not a name.
      assert_eq!(
         parse("0xnotanaddress@eip155:1"),
         Err(InteroperableNameError::InvalidAddress)
      );
      // A checksum belongs to a raw address.
      assert_eq!(
         parse("vitalik.eth@eip155:1#80B12379"),
         Err(InteroperableNameError::ChecksumWithoutAddress)
      );
      assert_eq!(
         parse("vitalik.eth@base#80B12379"),
         Err(InteroperableNameError::ChecksumWithoutAddress)
      );
   }

   #[test]
   fn gate_matches_what_can_be_resolved() {
      assert!(looks_like_interoperable_name("vitalik.eth@base"));
      assert!(looks_like_interoperable_name(
         " Vitalik.ETH@eip155:8453 "
      ));
      assert!(looks_like_interoperable_name(
         "0xFe89cc7aBB2C4183683ab71653C4cdc9B02D44b7@eip155:1#80B12379"
      ));
      // Plain name: the other path owns it.
      assert!(!looks_like_interoperable_name("vitalik.eth"));
      // Still typing.
      assert!(!looks_like_interoperable_name("vitalik.eth@"));
      assert!(!looks_like_interoperable_name("@base"));
      // Neither a name nor an address.
      assert!(!looks_like_interoperable_name("vitalik@base"));
      // Whitespace inside.
      assert!(!looks_like_interoperable_name("vitalik eth@base"));
      // Two separators.
      assert!(!looks_like_interoperable_name("a.eth@base@base"));
   }

   #[test]
   fn verify_accepts_a_match_and_rejects_a_mismatch() {
      let address = address!("Fe89cc7aBB2C4183683ab71653C4cdc9B02D44b7");

      // No checksum: nothing to check.
      assert!(verify(None, 1, &address).is_ok());
      // The EIP's fixture.
      assert!(verify(Some([0x80, 0xB1, 0x23, 0x79]), 1, &address).is_ok());
      // The same checksum for a different chain is a mismatch.
      assert_eq!(
         verify(Some([0x80, 0xB1, 0x23, 0x79]), 8453, &address),
         Err(InteroperableNameError::ChecksumMismatch {
            expected: checksum(8453, address),
            found: [0x80, 0xB1, 0x23, 0x79],
         })
      );
   }
}
