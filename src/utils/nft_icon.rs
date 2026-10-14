//! NFT images: on-chain metadata URI → metadata document → image bytes → two renderings.
//!
//! There is no keyless NFT CDN — SmolDapp serves token logos only — so the dependency-free source
//! is the chain itself: `tokenURI`/`uri` and then whatever that URI points at. Everything here is
//! behind the user's `fetch_asset_images` opt-in, checked at the single entry point
//! [`spawn_fetch_nft_icon`].
//!
//! Failures are deliberately quiet: a token whose art cannot be fetched or decoded shows the
//! placeholder and is not retried for the rest of the session, so a broken collection cannot turn
//! into a request on every repaint.
//!
//! Where a URI may point is bounded by [`ensure_fetchable`]: art is fetched over `https` only, and
//! never from a host that is — or resolves to — a loopback, private, link-local or otherwise
//! non-public address. That holds for every redirect hop too, which is why [`get_with_cap`] follows
//! them itself rather than leaving them to the HTTP client. A metadata URI comes from a contract, so
//! "fetch this" is really "make the user's machine send a GET there", and that is the whole of the
//! attack.

use crate::assets::icons::{NftIconData, save_nft_icon};
use crate::core::urls::ZeusUrl;
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use anyhow::anyhow;
use flate2::read::GzDecoder;
use image::imageops::FilterType;
use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg;
use std::borrow::Cow;
use std::io::{Cursor, Read};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use zeus_eth::alloy_primitives::{Address, U256};
use zeus_eth::nft::{NftToken, expand_id_placeholder};

/// Edge of the list thumbnail. Larger pictures are shrunk to fit inside this.
const THUMB_EDGE: u32 = 64;
/// Edge of the copy kept for inspecting a single NFT.
const LARGE_EDGE: u32 = 250;
/// Cap on a single response, image or metadata document alike.
///
/// NFT art is much bigger than a token logo, but this still stops an oversized or hostile response
/// before it is handed to the decoder.
const MAX_BYTES: usize = 4 * 1024 * 1024;
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);
/// How many redirect hops one fetch follows before giving up (reqwest's own default).
const MAX_REDIRECTS: usize = 10;

/// Cap on a metadata URI *string*, before it is expanded or fetched.
///
/// The string is attacker-controlled — any contract the wallet reads can return a multi-megabyte
/// `tokenURI` — and it is what a `data:` decode and an `{id}` expansion are sized from. Generous
/// enough for any `data:` URI that decodes within [`MAX_BYTES`] (base64 is about three quarters of its
/// input), and applied to the *raw* string in [`fetch_nft_icon`] as well as to the resolved one here,
/// so a template cannot be multiplied before anything looks at it.
const MAX_URI_LEN: usize = MAX_BYTES * 4;

/// Cap on either edge of the source art handed to the raster decoder, and on what that decode may
/// allocate.
///
/// [`MAX_BYTES`] bounds the *transfer*, not the decode: a ≤4 MiB PNG that declares 8192×8192 decodes
/// to ~256 MiB, and one list load starts [`NFT_ART_FETCH_PER_LOAD`] fetches at once — see
/// [`MAX_CONCURRENT_ART_DECODES`] for what bounds how many of them decode together. `image`'s own
/// defaults bound only `max_alloc` (512 MiB) and leave both dimensions open, so both are set here.
/// `MAX_ART_ALLOC` is exactly what a `MAX_ART_EDGE`-square 32-bit source needs — the renderings are
/// [`THUMB_EDGE`] and [`LARGE_EDGE`], so the edge cap is already far above anything a view can use.
const MAX_ART_EDGE: u32 = 4096;
const MAX_ART_ALLOC: u64 = (MAX_ART_EDGE as u64) * (MAX_ART_EDGE as u64) * 4;

/// How many pixels' worth of nested raster images one document may declare, in total.
///
/// [`MAX_ART_EDGE`] bounds each nested payload, not how many there are: a 4 MiB document holds ~48
/// 4096×4096 payloads at ~87 KB of base64 each, and a renderer that decoded them would work through
/// ~3 GiB of pixels — twice, since [`render_svg`] renders both sizes. This is the budget they share
/// per document: one full-size source.
const MAX_NESTED_ART_PIXELS: usize = (MAX_ART_EDGE as usize) * (MAX_ART_EDGE as usize);

/// Decode limits for art: both edges bounded (a *strict* limit, so a decoder that cannot uphold it
/// must refuse rather than proceed), and an allocation budget a [`MAX_ART_EDGE`]-square source fits.
fn art_limits() -> image::Limits {
   let mut limits = image::Limits::default();
   limits.max_image_width = Some(MAX_ART_EDGE);
   limits.max_image_height = Some(MAX_ART_EDGE);
   limits.max_alloc = Some(MAX_ART_ALLOC);
   limits
}

/// How many art downloads one list load starts.
///
/// The cap is what keeps a wallet tracking hundreds of ids from firing hundreds of concurrent
/// gateway requests at once (the throttle that follows marks tokens failed for the session, so the
/// art would never appear). Tokens already tried are skipped, so successive loads work through a long
/// list instead of retrying the first few forever. How many of these *decode* at once is a separate
/// bound — see [`MAX_CONCURRENT_ART_DECODES`].
const NFT_ART_FETCH_PER_LOAD: usize = 24;

/// How many art decodes may run at once.
///
/// Every bound in this module is per *document*, and a list load starts [`NFT_ART_FETCH_PER_LOAD`]
/// of them: twenty-four decoded rasters of [`MAX_ART_ALLOC`] each, or twenty-four documents each
/// spending the ~155 MB usvg allows before its own one-million-node cap trips, is what the aggregate
/// would be. Fetching twenty-four at a time is what that constant is for — a throttle on the
/// gateways — so the decode, which is the expensive half, queues here instead.
///
/// A `const` rather than a `OnceLock`: it is never replaced, and `acquire` on it cannot fail.
const MAX_CONCURRENT_ART_DECODES: usize = 3;
static ART_DECODE_SLOTS: tokio::sync::Semaphore =
   tokio::sync::Semaphore::const_new(MAX_CONCURRENT_ART_DECODES);

// The IPFS gateways and the Arweave host live in the endpoint catalog
// (`crate::core::urls`), which also carries the note on why gateway order matters.

/// Whether an address is one a collection's URI has no business pointing the wallet at.
///
/// Loopback and the private ranges are how art becomes a probe of whatever else is listening on the
/// user's machine — the wallet's own dapp server included — and `169.254.169.254` is the cloud
/// metadata address, which is worth a request only to whoever put the URI on chain.
fn is_public_ip(ip: IpAddr) -> bool {
   match ip {
      IpAddr::V4(v4) => is_public_v4(v4),
      IpAddr::V6(v6) => match embedded_v4(v6) {
         // The transition forms carry an IPv4 destination: `::a.b.c.d` and `::ffff:a.b.c.d` are the
         // same address in another spelling, and NAT64 / 6to4 / IPv4-translated wrap one too. Only
         // the mapped form used to be covered, so `[::127.0.0.1]` — and every private range spelled
         // that way — read as public.
         Some(v4) => is_public_v4(v4),
         None => {
            let o = v6.octets();

            !(v6.is_loopback()
               || v6.is_unspecified()
               || v6.is_multicast()
               || v6.is_unique_local()
               || v6.is_unicast_link_local()
               // `fec0::/10` is site-local (RFC 3879): deprecated and never routed off the local
               // network, which is exactly the scope a contract's URI has no business reaching.
               || (o[0] == 0xfe && (o[1] & 0xc0) == 0xc0))
         }
      },
   }
}

/// The IPv4 address an IPv6 one carries, when it is one of the transition forms that embed one.
///
/// Every one of these means an IPv4 destination reached over a network that routes IPv6, so reading
/// them as "some public IPv6" hands `[64:ff9b::127.0.0.1]` — or a 6to4 address wrapping
/// `169.254.169.254` — straight past the check. Whether the connection arrives depends on the
/// network: a NAT64 gateway or a 6to4 relay delivers it, an ordinary IPv6-only host fails the
/// attempt. Zeus refuses the address rather than betting on which network it is on.
fn embedded_v4(v6: Ipv6Addr) -> Option<Ipv4Addr> {
   // `::a.b.c.d` (IPv4-compatible) and `::ffff:a.b.c.d` (IPv4-mapped) both reduce here; so do `::`
   // and `::1`, whose embedded addresses `is_public_v4` refuses on their own terms.
   if let Some(v4) = v6.to_ipv4() {
      return Some(v4);
   }

   let o = v6.octets();

   // `::ffff:0:a.b.c.d` — IPv4-translated (RFC 6145).
   if o[..8].iter().all(|b| *b == 0) && o[8] == 0xff && o[9] == 0xff && o[10] == 0 && o[11] == 0 {
      return Some(Ipv4Addr::new(o[12], o[13], o[14], o[15]));
   }

   // `64:ff9b::a.b.c.d` — NAT64 (RFC 6052).
   if o[0] == 0x00
      && o[1] == 0x64
      && o[2] == 0xff
      && o[3] == 0x9b
      && o[4..12].iter().all(|b| *b == 0)
   {
      return Some(Ipv4Addr::new(o[12], o[13], o[14], o[15]));
   }

   // `64:ff9b:1::/48` — the local-use translation prefix (RFC 8215). RFC 6052 lays a /48 out as
   // bits 48-63, the `u` octet, then bits 72-87, so the two halves of the embedded address sit
   // either side of `u`; the octet itself is reserved and its value is not worth trusting, so the
   // halves are read regardless and a non-conforming spelling is refused rather than waved through.
   // The whole /48 is local-use, so this is the same bet the well-known prefix above is not worth.
   if o[..6] == [0x00, 0x64, 0xff, 0x9b, 0x00, 0x01] {
      return Some(Ipv4Addr::new(o[6], o[7], o[9], o[10]));
   }

   // `2002:a.b.c.d::` — 6to4 (RFC 3056): the embedded address is the 6to4 prefix's own, which is the
   // host the address routes to.
   if o[0] == 0x20 && o[1] == 0x02 {
      return Some(Ipv4Addr::new(o[2], o[3], o[4], o[5]));
   }

   None
}

fn is_public_v4(v4: Ipv4Addr) -> bool {
   let [a, b, _, _] = v4.octets();

   !(v4.is_loopback()                                   // 127/8
      || v4.is_private()                                // 10/8, 172.16/12, 192.168/16
      || v4.is_link_local()                             // 169.254/16 — cloud metadata
      || v4.is_broadcast()
      || v4.is_documentation()
      || v4.is_unspecified()
      || v4.is_multicast()
      || a == 0                                         // "this network"
      || (a == 100 && (64..128).contains(&b))           // 100.64/10, carrier-grade NAT
      || (a == 198 && (b == 18 || b == 19))             // 198.18/15, benchmarking
      || a >= 240) // 240/4, reserved
}

/// Whether a host name is one to refuse before even resolving it.
///
/// A resolution check alone would catch most of these, but not the ones that fail to resolve at all
/// (`.local` is mDNS, not DNS) — and a name that cannot be checked must not be fetchable.
fn is_blocked_hostname(host: &str) -> bool {
   let host = host.trim_end_matches('.').to_ascii_lowercase();

   host == "localhost"
      || host.ends_with(".localhost")
      || host.ends_with(".local")
      || host.ends_with(".internal")
      || host.ends_with(".home.arpa")
}

/// A host as an address, brackets and all: `[::1]` and `127.0.0.1` both parse.
fn host_ip(host: &str) -> Option<IpAddr> {
   host.trim_start_matches('[').trim_end_matches(']').parse().ok()
}

/// The verdict on a URL, without resolving anything.
///
/// Synchronous and cheap: it is the pre-check [`ensure_fetchable`] runs before resolving anything, so
/// a URL that cannot pass on its own terms — the scheme, a literal address, an obviously-local name —
/// is refused without a lookup.
///
/// It is deliberately not the whole check. A name it passes can still resolve to a private address,
/// and a redirect is the second way in: a harmless-looking public name only has to answer `302` with
/// one to get the request the URI itself was refused. Both are answered by resolving, which is why
/// [`get_with_cap`] follows redirects itself rather than leaving them to the client's policy — a
/// policy callback cannot await, and so can only ever see the name.
fn url_is_fetchable(url: &reqwest::Url) -> bool {
   if url.scheme() != "https" {
      return false;
   }

   match url.host_str() {
      Some(host) => match host_ip(host) {
         Some(ip) => is_public_ip(ip),
         None => !is_blocked_hostname(host),
      },
      None => false,
   }
}

/// Before the wallet is pointed at a URL: `https`, a host that is not obviously local, and — because
/// a name can mean anything — no address it resolves to may be local either.
///
/// The lookup here is a check, not a pin: the connection resolves the name again, so a name whose
/// answer changes in between is not caught. Everything that does not depend on that race is.
async fn ensure_fetchable(url: &reqwest::Url) -> Result<(), anyhow::Error> {
   if !url_is_fetchable(url) {
      return Err(anyhow!("refusing to fetch {url}"));
   }

   let Some(host) = url.host_str() else {
      return Err(anyhow!("{url} has no host"));
   };

   // A literal address has nothing left to resolve.
   if host_ip(host).is_some() {
      return Ok(());
   }

   let port = url.port_or_known_default().unwrap_or(443);
   let addrs: Vec<IpAddr> = tokio::net::lookup_host((host, port))
      .await
      .map_err(|e| anyhow!("cannot resolve {host}: {e}"))?
      .map(|addr| addr.ip())
      .collect();

   if addrs.is_empty() {
      return Err(anyhow!("{host} did not resolve"));
   }

   if let Some(local) = addrs.iter().find(|ip| !is_public_ip(**ip)) {
      return Err(anyhow!(
         "refusing to fetch {url}: {host} is {local}"
      ));
   }

   Ok(())
}

/// GET client, shared because a client per fetch would redo TLS setup every time.
///
/// Redirects are **never** followed here: [`get_with_cap`] follows them itself, one hop at a time,
/// because each hop has to be resolved and a redirect-policy callback cannot await. A client that
/// followed one would be the hole that check exists to close.
///
/// A client that cannot be built is an error rather than a fallback to `Client::new()`: that client
/// follows redirects, so the "fallback" would quietly undo the line above.
fn http_client() -> Result<&'static reqwest::Client, anyhow::Error> {
   static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

   if let Some(client) = CLIENT.get() {
      return Ok(client);
   }

   let client = reqwest::Client::builder()
      .user_agent("zeus-wallet")
      .timeout(FETCH_TIMEOUT)
      .redirect(reqwest::redirect::Policy::none())
      .build()
      .map_err(|e| anyhow!("cannot build the art http client: {e}"))?;

   Ok(CLIENT.get_or_init(|| client))
}

/// A metadata URI resolved to something we can actually fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedUri {
   /// An IPFS path — a CID, optionally with a `/…` suffix — to try on each gateway.
   Ipfs(String),
   /// An Arweave transaction id.
   Arweave(String),
   /// Inline bytes: no network at all.
   Data(Vec<u8>),
   /// A plain HTTP(S) URL.
   Http(String),
}

/// Resolve a metadata URI.
///
/// `None` means "we cannot fetch this" — a bare CID, an unknown scheme, an empty string — which the
/// caller treats as *no image* rather than as an error. Schemes are matched case-insensitively
/// because contract-provided URIs are not reliably lowercase.
pub fn resolve_uri(uri: &str) -> Option<ResolvedUri> {
   let uri = uri.trim();
   if uri.is_empty() {
      return None;
   }

   // A `data:` URI is decoded here, before [`fetch_resolved`] can apply [`MAX_BYTES`] — and a base64
   // payload allocates about three quarters of its input, while percent-encoded text allocates less than
   // it. So the *string* is bounded first: it is attacker-controlled and nothing else caps it (see
   // [`MAX_URI_LEN`]).
   if uri.len() > MAX_URI_LEN {
      return None;
   }

   if let Some(rest) = strip_prefix_ci(uri, "ipfs://") {
      // Some collections write `ipfs://ipfs/<cid>`; the gateway path must not repeat the segment.
      let path = rest.strip_prefix("ipfs/").unwrap_or(rest).trim_matches('/');
      return (!path.is_empty()).then(|| ResolvedUri::Ipfs(path.to_string()));
   }

   if let Some(rest) = strip_prefix_ci(uri, "ar://") {
      let id = rest.trim_matches('/');
      return (!id.is_empty()).then(|| ResolvedUri::Arweave(id.to_string()));
   }

   if strip_prefix_ci(uri, "data:").is_some() {
      return parse_data_uri(uri).map(ResolvedUri::Data);
   }

   // Only `https`: a contract's `http://` URI would be fetched in the clear (an observer learns which
   // token is being looked at, and a MITM chooses the art), and it is the one scheme that lets a
   // redirect reach a plaintext local server. Every gateway Zeus trusts is https.
   strip_prefix_ci(uri, "https://").map(|_| ResolvedUri::Http(uri.to_string()))
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
   let head = s.get(..prefix.len())?;
   head.eq_ignore_ascii_case(prefix).then(|| &s[prefix.len()..])
}

/// Decode a `data:` URI payload.
///
/// A `;base64` marker selects base64; anything else is read as percent-encoded text. Note that
/// clients in the wild also emit unescaped text (`data:application/json,{"name":…}`), which
/// [`percent_decode`] passes through unchanged rather than rejecting.
pub fn parse_data_uri(uri: &str) -> Option<Vec<u8>> {
   let rest = strip_prefix_ci(uri, "data:")?;
   let (meta, payload) = rest.split_once(',')?;

   if meta.split(';').any(|part| part.eq_ignore_ascii_case("base64")) {
      return base64_decode(payload);
   }

   Some(percent_decode(payload))
}

/// Decode base64, standard or URL-safe alphabet, padding optional.
///
/// Hand-rolled so Zeus does not take a dependency for its single base64 call site. Both alphabets
/// and missing padding are accepted because `data:` URIs in the wild use all three spellings, and
/// whitespace is skipped because longer payloads are often wrapped.
fn base64_decode(input: &str) -> Option<Vec<u8>> {
   fn sextet(byte: u8) -> Option<u32> {
      match byte {
         b'A'..=b'Z' => Some((byte - b'A') as u32),
         b'a'..=b'z' => Some((byte - b'a') as u32 + 26),
         b'0'..=b'9' => Some((byte - b'0') as u32 + 52),
         b'+' | b'-' => Some(62),
         b'/' | b'_' => Some(63),
         _ => None,
      }
   }

   let mut out = Vec::with_capacity(input.len() / 4 * 3);
   let mut acc: u32 = 0;
   let mut bits: u32 = 0;

   for byte in input.bytes() {
      if byte.is_ascii_whitespace() {
         continue;
      }
      if byte == b'=' {
         break;
      }

      acc = (acc << 6) | sextet(byte)?;
      bits += 6;

      if bits >= 8 {
         bits -= 8;
         out.push((acc >> bits) as u8);
      }
   }

   Some(out)
}

/// Decode `%XX` escapes, copying everything else through as bytes.
///
/// A malformed escape is left verbatim rather than rejected: it is more useful to hand the decoder
/// the payload as-is and let it fail on its own terms than to guess that the whole URI is invalid.
fn percent_decode(input: &str) -> Vec<u8> {
   let bytes = input.as_bytes();
   let mut out = Vec::with_capacity(bytes.len());
   let mut i = 0;

   while i < bytes.len() {
      if bytes[i] == b'%' && i + 2 < bytes.len() {
         let escape = std::str::from_utf8(&bytes[i + 1..i + 3])
            .ok()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());

         if let Some(byte) = escape {
            out.push(byte);
            i += 3;
            continue;
         }
      }

      out.push(bytes[i]);
      i += 1;
   }

   out
}

/// The `image` field of a metadata document, as a URI.
///
/// Only the string form is taken. Some documents carry `image` as an object of its own with a `uri`
/// inside; guessing at that shape would be worse than showing the placeholder.
pub fn image_url_from_metadata(bytes: &[u8]) -> Option<String> {
   let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
   let image = value.get("image")?.as_str()?.trim();

   (!image.is_empty()).then(|| image.to_string())
}

/// Whether a payload is a metadata document rather than a picture.
///
/// A cheap sniff on the first non-whitespace byte: every JSON document we care about is an object,
/// and no image format starts with `{`.
fn looks_like_json(bytes: &[u8]) -> bool {
   bytes
      .iter()
      .find(|byte| !byte.is_ascii_whitespace())
      .is_some_and(|byte| *byte == b'{')
}

/// Decode once and produce both renderings.
///
/// One decode for two sizes: decoding a multi-megabyte picture twice, once for the grid and once
/// for the detail view, is the expensive part.
///
/// Not public on purpose: [`prepare_image_data`] is the entry point, so nobody can bypass the
/// vector-art check and hand an SVG to a decoder that cannot read it.
fn render_two_sizes(bytes: &[u8]) -> Result<(Vec<u8>, Vec<u8>), anyhow::Error> {
   // Built by hand rather than with `image::load_from_memory`, which would take `image`'s defaults:
   // no dimension bound at all and a 512 MiB allocation budget (see [`art_limits`]).
   let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
   reader.limits(art_limits());
   let image = reader.decode()?;

   let render = |edge: u32| -> Result<Vec<u8>, anyhow::Error> {
      // Only ever shrink. Upscaling would blur a small picture without adding detail, and would
      // store a thumbnail larger than the original.
      let scaled = if image.width() > edge || image.height() > edge {
         image.resize(edge, edge, FilterType::Lanczos3)
      } else {
         image.clone()
      };

      let mut buf = Vec::new();
      scaled.write_to(
         &mut Cursor::new(&mut buf),
         image::ImageFormat::Png,
      )?;
      Ok(buf)
   };

   Ok((render(THUMB_EDGE)?, render(LARGE_EDGE)?))
}

/// Whether a document declares an XML entity — the one construct whose cost is not the document's.
///
/// usvg parses with `allow_dtd: true` and leaves roxmltree's `nodes_limit` at `u32::MAX`
/// (`usvg-0.45.1/src/parser/mod.rs`), so the only guard on entity expansion is roxmltree's loop
/// detector: at most 10 levels deep, at most 255 references *per reference*. Neither counts bytes,
/// and an entity's own definition sits in the document. A `<!ENTITY a "…7 MiB…">` with 250 `&a;`
/// references is therefore gigabytes of text out of a document of [`MAX_INFLATED_SVG`] or less — and
/// every further reference in the document gets its own 255-reference budget. Measured: a 1.2 KiB
/// download that had to be killed at 60 s.
///
/// Art has no use for entity declarations, and an *undefined* entity reference is a parse error
/// already, so a document that declares one is refused whole instead of handed to the parser. A
/// plain `<!DOCTYPE …>` prolog — what Inkscape-style art opens with — declares nothing and is left
/// alone.
fn declares_an_xml_entity(bytes: &[u8]) -> bool {
   const KEYWORD: &[u8] = b"ENTITY";

   let mut at = 0;
   while at + 2 <= bytes.len() {
      if bytes[at] == b'<' && bytes[at + 1] == b'!' {
         // `<!ENTITY name …>` and `<!ENTITY % name …>`: XML allows white space between the markup
         // introducer and the keyword, so it is skipped rather than assumed away.
         let mut keyword = at + 2;
         while keyword < bytes.len() && bytes[keyword].is_ascii_whitespace() {
            keyword += 1;
         }

         // White space (or the end of the document) after `ENTITY`, so an element or attribute name
         // that merely starts with the keyword is not one.
         if bytes[keyword..].starts_with(KEYWORD)
            && bytes.get(keyword + KEYWORD.len()).is_none_or(|byte| byte.is_ascii_whitespace())
         {
            return true;
         }
      }

      at += 1;
   }

   false
}

/// `usvg` options that cannot reach the machine, and cannot be made to allocate without a bound.
///
/// `resolve_string` is the one hook with filesystem reach: usvg's default treats a non-`data:`
/// `href` as a **path** and `std::fs::read`s it (usvg-0.45.1 `parser/image.rs`), so a collection
/// whose art says `<image href="/etc/passwd"/>` — or names a device file that never ends — would
/// have Zeus open it, and a referenced SVG is *drawn*, so the picture would appear as the token's
/// art. Refusing every string reference removes that reach.
///
/// `resolve_data` stays the default resolver for everything a nested document is allowed to carry —
/// that is what lets art with an inline image still render — behind the checks in
/// [`art_data_resolver`]. usvg parses an embedded SVG with resolvers of its own (`load_sub_svg`), so
/// a nested document cannot name a path either; what it can still do is inflate, declare entities or
/// decode without a bound, which is what the wrapper refuses.
fn svg_options() -> usvg::Options<'static> {
   let mut options = usvg::Options::default();
   options.image_href_resolver = usvg::ImageHrefResolver {
      resolve_data: art_data_resolver(),
      resolve_string: Box::new(|_, _| None),
   };
   options
}

/// Whether usvg hands a nested payload to its raster decoder rather than to its SVG parser.
///
/// Mirrors `ImageHrefResolver::default_data_resolver` (`usvg-0.45.1/src/parser/image.rs`): a raster
/// mime by name, or — for `text/plain`, which is what a mime-less `data:` becomes — a payload whose
/// bytes are one of the four formats it sniffs for through `imagesize`, the same call Zeus makes
/// below. The distinction is what decides the bound: a raster is bounded by the size it declares, a
/// document by what it may carry.
fn nested_payload_is_raster(mime: &str, data: &[u8]) -> bool {
   match mime {
      "image/jpg" | "image/jpeg" | "image/png" | "image/gif" | "image/webp" => true,
      "text/plain" => matches!(
         imagesize::image_type(data),
         Ok(imagesize::ImageType::Gif
            | imagesize::ImageType::Jpeg
            | imagesize::ImageType::Png
            | imagesize::ImageType::Webp)
      ),
      _ => false,
   }
}

/// usvg's `data:` resolver behind the checks a nested payload has to pass.
///
/// The bytes decide, never the mime. usvg reaches its own uncapped inflate for a
/// `data:image/svg+xml` payload, for a `text/plain` one, and for a mime-less `data:` — whose mime
/// defaults to `text/plain` — because `parser/image.rs` matches the mime only to fall through to
/// `load_sub_svg` for all three, and that calls `Tree::from_data`, the function that inflates. A
/// check on the mime is therefore a check on one of three spellings of the same payload; the
/// compression has to be seen in the bytes, at the one place they are handed over.
///
/// What a nested payload can cost, and what each check answers:
///
/// - **compressed** — usvg would inflate it with no cap at all, so it is refused on the magic;
/// - **raster** — decoded by the renderer at the size the payload declares: bounded per payload by
///   [`MAX_ART_EDGE`] and per document by [`MAX_NESTED_ART_PIXELS`], with an unreadable declared size
///   refused rather than handed on;
/// - **document** — parsed here with the same options as the top level (`load_sub_svg`), so it has
///   to be free of entity declarations (`declares_an_xml_entity`).
///
/// `resvg` in this build does not decode a raster at all: its `raster-images` feature is off, so
/// `render` logs and skips every raster image kind (see `Cargo.toml`). The raster checks therefore
/// bound the day that changes rather than today; the document check is live either way.
fn art_data_resolver() -> usvg::ImageHrefDataResolverFn<'static> {
   let default = usvg::ImageHrefResolver::default_data_resolver();
   // Shared by one document's nested payloads: `svg_options` is rebuilt per render, so this budget
   // is per document. An `Arc` because the resolver has to be `Send + Sync`.
   let declared_pixels = Arc::new(AtomicUsize::new(0));

   Box::new(move |mime, data, options| {
      // The exact test `Tree::from_data` inflates on, so nothing compressed can reach the
      // `read_to_end` behind it. The nested twin of [`inflate_svgz_if_needed`], which applies the
      // same rule to the document itself.
      if data.starts_with(&GZIP_MAGIC) {
         tracing::debug!("refusing a nested image: a compressed payload inflates unbounded");
         return None;
      }

      if nested_payload_is_raster(mime, &data) {
         let Ok(size) = imagesize::blob_size(&data) else {
            tracing::debug!("refusing a nested raster image whose size cannot be read");
            return None;
         };

         if size.width > MAX_ART_EDGE as usize || size.height > MAX_ART_EDGE as usize {
            tracing::debug!(
               "refusing a nested {}x{} raster image: past the {MAX_ART_EDGE}px decode edge",
               size.width,
               size.height
            );
            return None;
         }

         let pixels = size.width.saturating_mul(size.height);
         let spent = declared_pixels.fetch_add(pixels, Ordering::Relaxed).saturating_add(pixels);

         if spent > MAX_NESTED_ART_PIXELS {
            tracing::debug!(
               "refusing a nested raster image: {spent} px declared past the \
                {MAX_NESTED_ART_PIXELS}px budget for one document"
            );
            return None;
         }
      } else if declares_an_xml_entity(&data) {
         tracing::debug!("refusing a nested document that declares an XML entity");
         return None;
      }

      default(mime, data, options)
   })
}

/// The two bytes every gzip stream starts with.
///
/// One definition for both places that have to recognise one: [`inflate_svgz_if_needed`], which
/// inflates the document itself under a bound, and [`art_data_resolver`], which refuses a compressed
/// nested payload before usvg can inflate it uncapped. It is the same test usvg makes
/// (`parser/mod.rs::from_data`), so the two agree with it by construction.
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// The largest an SVG may be after inflating.
///
/// A gzip stream is the one route by which a download that passed every size check becomes an
/// arbitrarily large allocation: the compressed form is bounded by [`MAX_BYTES`], the inflated one
/// by nothing at all. usvg inflates SVGZ itself with no cap (its `parser/mod.rs` runs a `GzDecoder`
/// straight into `read_to_end`), so Zeus inflates first, under this bound, and only ever hands usvg
/// plain SVG.
const MAX_INFLATED_SVG: usize = 8 * 1024 * 1024;

/// `bytes` with a gzip stream inflated, refusing anything that inflates past [`MAX_INFLATED_SVG`].
///
/// An error rather than a truncated document on purpose: half an SVG is not something to draw, and
/// "too big" is the honest answer.
fn inflate_svgz_if_needed(bytes: &[u8]) -> Result<Cow<'_, [u8]>, anyhow::Error> {
   if !bytes.starts_with(&GZIP_MAGIC) {
      return Ok(Cow::Borrowed(bytes));
   }

   let mut out = Vec::new();
   GzDecoder::new(bytes).take(MAX_INFLATED_SVG as u64 + 1).read_to_end(&mut out)?;

   if out.len() > MAX_INFLATED_SVG {
      return Err(anyhow!(
         "SVGZ art inflates past {MAX_INFLATED_SVG} bytes, refusing to render it"
      ));
   }

   Ok(Cow::Owned(out))
}

/// Rasterise vector art into the two renderings a view asks for.
///
/// Only ever shrinks, matching [`render_two_sizes`]: a small picture is never upscaled.
///
/// The one place a document reaches the parser, which is why the check that has to happen first —
/// see [`declares_an_xml_entity`] — is here rather than at the call site.
///
/// A nested document that [`svg_options`] refuses — compressed, an entity declaration, a raster past
/// the decode edge or past the document's pixel budget — is dropped by usvg rather than failing the
/// document, so the rest of the art still renders.
fn render_svg(bytes: &[u8]) -> Result<(Vec<u8>, Vec<u8>), anyhow::Error> {
   if declares_an_xml_entity(bytes) {
      return Err(anyhow!(
         "SVG art declares an XML entity, refusing to parse it"
      ));
   }

   let tree = usvg::Tree::from_data(bytes, &svg_options())?;
   Ok((
      render_svg_edge(&tree, THUMB_EDGE)?,
      render_svg_edge(&tree, LARGE_EDGE)?,
   ))
}

/// One rendering of `tree`, scaled to fit inside `edge`.
fn render_svg_edge(tree: &usvg::Tree, edge: u32) -> Result<Vec<u8>, anyhow::Error> {
   let size = tree.size();
   // `.min(1.0)` before the multiply, so a document with no intrinsic size cannot produce a NaN
   // scale below: the pixmap is clamped to at least 1x1 either way.
   let scale = (edge as f32 / size.width()).min(edge as f32 / size.height()).min(1.0);
   let width = (size.width() * scale).round().max(1.0) as u32;
   let height = (size.height() * scale).round().max(1.0) as u32;

   let mut pixmap = Pixmap::new(width, height)
      .ok_or_else(|| anyhow!("{width}x{height} is not a renderable size"))?;
   resvg::render(
      tree,
      Transform::from_scale(scale, scale),
      &mut pixmap.as_mut(),
   );

   Ok(pixmap.encode_png()?)
}

/// Whether these bytes are vector art.
///
/// Sniffed from the content rather than from the URI or a mime type: the same collection serves SVG
/// from a `data:` URI and from gateways that label it `application/octet-stream`. Anything
/// mislabelled would otherwise be handed to a decoder that cannot read it.
///
/// A legal prolog — an XML declaration, a doctype, a comment — may precede the root element, and all
/// three may appear together. Reading the first four non-whitespace bytes instead sent a document
/// that opened with `<!DOCTYPE svg …>` to the raster decoder, which cannot read it, and the token
/// showed a placeholder.
fn is_svg(bytes: &[u8]) -> bool {
   // The prolog is small in practice; a document that hides it further in reads as a raster image.
   let head = &bytes[..bytes.len().min(4096)];
   // A UTF-8 BOM and leading whitespace are both legal ahead of the prolog.
   let head = head.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(head);
   let mut rest = head;

   loop {
      rest = rest.trim_ascii_start();

      // `<?xml … ?>`, `<!DOCTYPE … >`, `<!-- … -->`: skipped to the end of the declaration, then
      // whatever follows is checked again — a prolog may be several of these in a row.
      let skip = if rest.starts_with(b"<?") {
         rest.iter().position(|b| *b == b'>').map(|at| at + 1)
      } else if rest.starts_with(b"<!--") {
         rest[4..].windows(3).position(|w| w == b"-->").map(|at| at + 4 + 3)
      } else if rest.starts_with(b"<!") {
         rest.iter().position(|b| *b == b'>').map(|at| at + 1)
      } else {
         None
      };

      match skip.filter(|at| *at < rest.len()) {
         Some(at) => rest = &rest[at..],
         // Nothing left to skip: the root element is here, or the head ended inside a prolog.
         None => break,
      }
   }

   // The root element decides — except when the head ended *inside* a prolog, which leaves the root
   // unread and only says the document is XML: no raster format opens with `<`, and a renderer that
   // cannot read it refuses it on its own terms. Reading that as "not vector art" is how a legal
   // prolog longer than the head became a placeholder.
   opens_svg_root(rest) || rest.starts_with(b"<?") || rest.starts_with(b"<!")
}

/// Whether `rest` opens with the SVG root element.
fn opens_svg_root(rest: &[u8]) -> bool {
   rest.len() >= 4 && rest[..4].eq_ignore_ascii_case(b"<svg")
}

/// Decide what to store for a piece of art: two raster renderings, whichever way it arrived.
///
/// Vector art is rasterised here instead of being kept as source. A renderer handed SVG source
/// resolves `href` against the filesystem, which turns a collection's art into a local file read
/// and, for a referenced SVG, into a picture of a file on disk. Rendering it ourselves, under
/// [`svg_options`], keeps that reach out of the picture entirely. The two sizes are the only ones
/// the views ask for, so neither the grid nor the detail view loses anything.
pub fn prepare_image_data(bytes: &[u8]) -> Result<NftIconData, anyhow::Error> {
   // Inflated here, once, under a bound — so neither this sniff nor usvg ever inflates anything.
   let bytes = inflate_svgz_if_needed(bytes)?;
   let bytes = bytes.as_ref();

   let (x64, x250) = if is_svg(bytes) {
      render_svg(bytes)?
   } else {
      render_two_sizes(bytes)?
   };

   Ok(NftIconData {
      x64,
      x250,
      // Filled in by the caller that knows which URI the art was read from — see `fetch_nft_icon`.
      source_uri: None,
   })
}

/// The URL a redirect points at: `Location`, resolved against the URL that produced it — a gateway
/// commonly answers with a relative path.
///
/// `Ok(None)` when the response is not a redirect. A redirect we cannot make sense of is an error
/// rather than a miss: there is no body to show either way, and "cannot tell right now" is the truer
/// answer.
fn redirect_target(
   url: &reqwest::Url,
   status: reqwest::StatusCode,
   location: Option<&reqwest::header::HeaderValue>,
) -> Result<Option<reqwest::Url>, anyhow::Error> {
   if !status.is_redirection() {
      return Ok(None);
   }

   let Some(location) = location else {
      return Err(anyhow!(
         "{url} answered {status} without a Location"
      ));
   };

   let location = location
      .to_str()
      .map_err(|_| anyhow!("{url} answered {status} with a Location that is not text"))?;

   url.join(location)
      .map(Some)
      .map_err(|e| anyhow!("{url} redirected to a url we cannot follow: {e}"))
}

/// GET with a size cap, from a host the wallet is allowed to reach — the redirect chain included.
///
/// `Ok(None)` is a definitive miss (404/410 or an empty body) — the caller stops asking. `Err` is
/// "cannot tell right now" (throttled, timed out, refused, too large).
///
/// Redirects are followed here rather than by the client's policy: a policy callback cannot await, so
/// it can only judge a hop's *name* — and a harmless-looking public name that answers `302` with a
/// private address would be followed and connected to. Every hop goes back through
/// [`ensure_fetchable`], so the guarantee the first URL passes holds for all of them.
async fn get_with_cap(url: &str, max: usize) -> Result<Option<Vec<u8>>, anyhow::Error> {
   let mut url = reqwest::Url::parse(url).map_err(|e| anyhow!("{url} is not a usable url: {e}"))?;
   let mut hops = 0;

   loop {
      ensure_fetchable(&url).await?;

      let mut response = http_client()?.get(url.clone()).send().await?;

      if let Some(next) = redirect_target(
         &url,
         response.status(),
         response.headers().get(reqwest::header::LOCATION),
      )? {
         hops += 1;
         if hops > MAX_REDIRECTS {
            return Err(anyhow!(
               "{url} redirected more than {MAX_REDIRECTS} times"
            ));
         }
         url = next;
         continue;
      }

      match response.status() {
         reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::GONE => return Ok(None),
         status if !status.is_success() => return Err(anyhow!("{url} returned {status}")),
         _ => {}
      }

      // A declared length is a hint, not the cap: a chunked response declares none, and a hostile one
      // can lie. The early exit is worth taking, but [`read_body`] is what enforces the limit.
      if let Some(len) = response.content_length() {
         if len as usize > max {
            return Err(anyhow!("{url} is too large ({len} bytes)"));
         }
      }

      return read_body(&mut response, max, url.as_str()).await;
   }
}

/// Read a body chunk by chunk, refusing to hold more than `max`.
///
/// This *is* the cap. `Response::bytes` would buffer whatever the server sends — bounded only by the
/// timeout, which at line rate is hundreds of megabytes per fetch, times the number of fetches one
/// list load starts — so the body is accumulated here and refused the moment it passes the limit,
/// declared length or not. The allocation grows to `max` at the very worst, whatever the response.
///
/// `Ok(None)` is an empty body, which the callers treat as a miss.
async fn read_body(
   response: &mut reqwest::Response,
   max: usize,
   url: &str,
) -> Result<Option<Vec<u8>>, anyhow::Error> {
   let mut body = Vec::new();

   while let Some(chunk) = response.chunk().await? {
      if body.len() + chunk.len() > max {
         return Err(anyhow!("{url} is larger than {max} bytes"));
      }

      body.extend_from_slice(&chunk);
   }

   Ok((!body.is_empty()).then_some(body))
}

/// Fetch an IPFS path, trying every gateway before giving up.
///
/// `Ok(None)` only when every gateway reported a miss; a transient failure is remembered and
/// returned as an error so it is not mistaken for "this token has no image".
async fn fetch_ipfs(path: &str) -> Result<Option<Vec<u8>>, anyhow::Error> {
   let mut last_error = None;

   for gateway in ZeusUrl::IPFS_GATEWAYS {
      match get_with_cap(&format!("{}/{path}", gateway.base()), MAX_BYTES).await {
         Ok(Some(bytes)) => return Ok(Some(bytes)),
         Ok(None) => continue,
         Err(e) => last_error = Some(e),
      }
   }

   match last_error {
      Some(e) => Err(e),
      None => Ok(None),
   }
}

async fn fetch_resolved(uri: ResolvedUri) -> Result<Option<Vec<u8>>, anyhow::Error> {
   match uri {
      // A `data:` URI arrives inline, so nothing is transferred — but the cap is about what gets decoded
      // and held, not about the transfer. Without this, a `tokenURI` that is itself a `data:` document
      // would be the one URI class that skips [`MAX_BYTES`] entirely.
      ResolvedUri::Data(bytes) if bytes.len() > MAX_BYTES => Err(anyhow!(
         "inline data: URI carries {} bytes, over the {MAX_BYTES} limit",
         bytes.len()
      )),
      ResolvedUri::Data(bytes) => Ok(Some(bytes)),
      ResolvedUri::Http(url) => get_with_cap(&url, MAX_BYTES).await,
      ResolvedUri::Ipfs(path) => fetch_ipfs(&path).await,
      ResolvedUri::Arweave(id) => {
         get_with_cap(
            &format!("{}/{id}", ZeusUrl::Arweave.base()),
            MAX_BYTES,
         )
         .await
      }
   }
}

/// Download an NFT's art and prepare it for storage.
///
/// `Ok(None)` means there is nothing to show: no metadata, metadata without a usable `image`, an
/// unfetchable URI, or bytes in a format we cannot decode. That is the normal outcome for a good
/// share of tokens, and the caller shows the placeholder. `Err` means "cannot tell right now" —
/// throttling or a timeout — and is the only outcome worth retrying.
///
/// # Formats
///
/// Raster art is decoded by the `image` crate, and the formats Zeus relies on (`png`, `jpeg`,
/// `webp`, `gif`) are declared in `Cargo.toml` rather than inherited from whichever other crate in
/// the graph happens to enable them. `decodes_the_art_formats_that_appear_on_chain` fails if one
/// of them goes missing.
///
/// **SVG** is rasterised here, at the two sizes the views ask for, and never handed on as source: a
/// renderer given source resolves `href` against the filesystem (see [`svg_options`]). Art in a
/// format that is not enabled lands in `Ok(None)` with its detected format logged, so a placeholder
/// is diagnosable rather than mysterious.
pub async fn fetch_nft_icon(
   metadata_uri: &str,
   token_id: U256,
) -> Result<Option<NftIconData>, anyhow::Error> {
   // Bounded *before* the expansion below: `expand_id_placeholder` copies the whole string once per
   // placeholder form (four at most) while the only other bound applies to its result, so without this
   // the copy is what grows, not the thing that is checked.
   if metadata_uri.len() > MAX_URI_LEN {
      return Ok(None);
   }

   // The URI may still be a raw ERC-1155 template carrying `{id}`; expanding an already-expanded
   // URI is a no-op, so this is safe whichever layer expanded it. A template that cannot be expanded
   // within the bound is a **miss**: `resolve_uri` accepts the unexpanded form as a URL, so handing it
   // on means asking a gateway for a document named `{id}` — on every list load, since a miss caches
   // nothing.
   let Some(uri) = expand_id_placeholder(metadata_uri, token_id) else {
      return Ok(None);
   };

   let Some(resolved) = resolve_uri(&uri) else {
      return Ok(None);
   };

   let Some(bytes) = fetch_resolved(resolved).await? else {
      return Ok(None);
   };

   // Either the URI *is* the picture, or it is a document that points at one.
   let image_bytes = if looks_like_json(&bytes) {
      let Some(image_uri) = image_url_from_metadata(&bytes) else {
         return Ok(None);
      };

      // The same rule for the image a document points at: an expansion past the bound is a miss, not a
      // URL to request.
      let Some(image_uri) = expand_id_placeholder(&image_uri, token_id) else {
         return Ok(None);
      };

      let Some(resolved) = resolve_uri(&image_uri) else {
         return Ok(None);
      };

      match fetch_resolved(resolved).await? {
         Some(bytes) => bytes,
         None => return Ok(None),
      }
   } else {
      bytes
   };

   // Decoding, resizing and rasterising are CPU work. `fetch_nft_icon` runs on a worker of the
   // shared runtime — the same pool the RPC, proving and server tasks use — so the decode is handed
   // to the blocking pool instead: an art pipeline that saturates every worker reads as a wallet that
   // cannot talk to a node while a list loads.
   //
   // One slot at a time, though: every other bound in this module is per document and a list load
   // starts many decodes at once — see [`MAX_CONCURRENT_ART_DECODES`]. The permit covers the decode
   // alone, so the fetch above still runs as wide as before.
   let decode_slot = ART_DECODE_SLOTS
      .acquire()
      .await
      .map_err(|e| anyhow!("no art decode slot is available: {e}"))?;

   let prepared = RT
      .spawn_blocking(move || match prepare_image_data(&image_bytes) {
         Ok(data) => Ok(data),
         Err(e) => {
            // Not retryable: an undecodable picture will not become decodable. Log the format so a
            // placeholder is diagnosable instead of mysterious.
            let format = match image::guess_format(&image_bytes) {
               Ok(format) => format!("{format:?}"),
               Err(_) => "unrecognised".to_string(),
            };
            tracing::debug!("NFT image not decodable ({format}): {e}");
            Err(())
         }
      })
      .await;

   drop(decode_slot);

   match prepared {
      // The URI is stored with the art so a later change to it can be noticed at all: the cache is
      // keyed by `(collection, token id)`, which cannot tell on its own. What is recorded is the
      // *metadata* URI the fetch was asked for — the same string the caller compares against next
      // time — and a change to the document's contents behind an unchanged URI stays undetectable,
      // by nature.
      Ok(Ok(mut data)) => {
         data.source_uri = Some(uri);
         Ok(Some(data))
      }
      // Nothing to show: no metadata, no usable image, or a format we cannot decode.
      Ok(Err(())) => Ok(None),
      // The decode task itself went away (a panic in a decoder, or the runtime shutting down).
      Err(e) => Err(anyhow!("NFT image decode task failed: {e}")),
   }
}

/// Start the art downloads for `tokens`, at most [`NFT_ART_FETCH_PER_LOAD`] per load.
///
/// Call this off the frame path (a loader worker or a click): it reads `SHARED_GUI`, and the frame
/// holds that write-locked. Tokens already cached, in flight, or failed for the session are skipped,
/// and a token with no metadata URI needs no fetch — reading `tokenURI` for every row would be a
/// chain call per token.
pub fn start_nft_art_downloads<'a>(chain_id: u64, tokens: impl Iterator<Item = &'a NftToken>) {
   let icons = SHARED_GUI.read(|gui| gui.icons.clone());
   let mut started = 0;

   for token in tokens {
      if started >= NFT_ART_FETCH_PER_LOAD {
         break;
      }

      let Some(uri) = token.metadata_uri.clone() else {
         continue;
      };

      let key = (token.collection, chain_id, token.token_id);

      // The URI recorded with the art is the expanded one (`fetch_nft_icon` expands before storing
      // it as `source_uri`), so the comparison has to be against the same form: a raw `{id}` template
      // never matches itself once expanded, which had every templated collection dropping and
      // re-downloading its art on every list load.
      // A template that cannot be expanded within the bound never has art — the fetch is a miss and
      // caches nothing — so skipping it here is what stops that being one live request per list load.
      let Some(uri) = expand_id_placeholder(&uri, token.token_id) else {
         continue;
      };

      // Before deciding whether this token needs a fetch, check what is cached against the URI the
      // contract reports now: without this a reveal or an upgrade would keep showing the old picture for
      // good, because the cache is keyed by `(collection, token id)` rather than by URI.
      icons.nfts.reconcile_art(&key, Some(uri.as_str()));

      if !icons.nfts.needs_fetch(&key) {
         continue;
      }

      // Fire and forget: the row shows the placeholder until this lands.
      spawn_fetch_nft_icon(chain_id, token.collection, token.token_id, uri);
      started += 1;
   }
}

/// Fetch and cache an NFT's image in the background.
///
/// Safe to call from a repaint loop: after the first call the token is either in flight or marked
/// as failed for the session, so repeat calls are a couple of lock reads and nothing else. Nothing
/// leaves the machine unless the user opted in to fetching external images.
pub fn spawn_fetch_nft_icon(
   chain_id: u64,
   collection: Address,
   token_id: U256,
   metadata_uri: String,
) {
   let (icons, allowed) = SHARED_GUI.read(|gui| {
      (
         gui.icons.clone(),
         gui.ctx.read(|ctx| ctx.misc_config.fetch_asset_images()),
      )
   });

   if !allowed {
      return;
   }

   let key = (collection, chain_id, token_id);
   if !icons.nfts.try_begin_fetch(&key) {
      return;
   }

   RT.spawn(async move {
      match fetch_nft_icon(&metadata_uri, token_id).await {
         Ok(Some(data)) => {
            if let Err(_e) = save_nft_icon(chain_id, collection, token_id, &data) {
               #[cfg(feature = "dev")]
               tracing::warn!("Failed to save NFT image for {collection} #{token_id}: {_e}");
            }

            icons.nfts.insert_icon(key, data);
            icons.nfts.finish_fetch(&key, false);
            SHARED_GUI.write(|gui| {
               gui.request_repaint();
            });
         }
         // Nothing to show (no metadata, no usable image, or a format we cannot decode): stop
         // asking for this token until the next start.
         Ok(None) => icons.nfts.finish_fetch(&key, true),
         // Transient (throttled, timed out). Also stop for this session on purpose: retrying from
         // the repaint loop would hammer gateways that just asked us to slow down.
         Err(e) => {
            tracing::debug!("NFT image fetch failed for {collection} #{token_id}: {e}");
            icons.nfts.finish_fetch(&key, true);
         }
      }
   });
}

#[cfg(test)]
mod tests {
   use super::*;
   use std::sync::Arc;
   use zeus_eth::nft::MAX_EXPANDED_URI_LEN;

   #[test]
   fn an_svg_prolog_does_not_hide_the_root_element() {
      // Every form a legal document may open with — and that used to send it to the raster decoder.
      assert!(is_svg(
         b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"
      ));
      assert!(is_svg(b"<?xml version=\"1.0\"?>\n<svg/>"));
      assert!(is_svg(
         b"<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"x\">\n<svg/>"
      ));
      assert!(is_svg(b"<!-- drawn by hand -->\n<svg/>"));
      assert!(is_svg(
         b"\xEF\xBB\xBF  <?xml version=\"1.0\"?><!-- x --><!DOCTYPE svg><SVG/>"
      ));
      // A prolog longer than the head leaves the root unread: the document is still XML, and no
      // raster format opens with `<`.
      let long_prolog = format!(
         "<?xml version=\"1.0\"?><!--{}--><svg/>",
         "x".repeat(5000)
      );
      assert!(is_svg(long_prolog.as_bytes()));
      // And it still rejects what is not vector art.
      assert!(!is_svg(b"\x89PNG\r\n\x1a\n"));
      assert!(!is_svg(b"<html><body><svg/></body></html>"));
      assert!(!is_svg(b""));
   }

   #[test]
   fn an_ipv4_hidden_in_an_ipv6_address_is_still_checked() {
      let private = |address: &str| !is_public_ip(address.parse().unwrap());

      // The same local address, in every spelling that reaches an IPv4 destination.
      assert!(private("::127.0.0.1"));
      assert!(private("::ffff:127.0.0.1"));
      assert!(private("::ffff:0:127.0.0.1")); // IPv4-translated
      assert!(private("64:ff9b::127.0.0.1")); // NAT64
      assert!(private("64:ff9b::a00:1")); // NAT64 wrapping 10.0.0.1
      assert!(private("2002:7f00:1::")); // 6to4 wrapping 127.0.0.1
      assert!(private("2002:a9fe:a9fe::")); // 6to4 wrapping 169.254.169.254
      assert!(private("64:ff9b:1:7f00:0:1::")); // RFC 8215 local-use prefix wrapping 127.0.0.1
      assert!(private("64:ff9b:1:a00:0:1::")); // ... wrapping 10.0.0.1
      assert!(private("fec0::1")); // site-local, RFC 3879
      assert!(private("::1"));
      assert!(private("::"));

      // A 6to4 address wrapping a public one is still public, and a plain public v6 stays public.
      assert!(is_public_ip("2002:808:808::".parse().unwrap()));
      assert!(is_public_ip(
         "64:ff9b:1:808:0:808::".parse().unwrap()
      ));
      assert!(is_public_ip("2606:4700::1111".parse().unwrap()));
   }

   /// No compressed payload reaches usvg's uncapped inflate, whichever spelling of `data:` carries
   /// it — asserted where the bytes are handed over, rather than on a document that spells the mime
   /// one particular way.
   #[test]
   fn a_compressed_nested_image_is_refused_whatever_the_mime() {
      let options = svg_options();
      let resolve = &options.image_href_resolver.resolve_data;
      let compressed = gzip(RED_SVG.as_bytes());

      // `image/svg+xml`, and `text/plain` — which is what a mime-less `data:` becomes, and what
      // usvg's own fallback arm uses. Both reach `load_sub_svg` → `Tree::from_data`.
      for mime in ["image/svg+xml", "text/plain"] {
         assert!(
            resolve(mime, Arc::new(compressed.clone()), &options).is_none(),
            "{mime}: a compressed nested document must not be handed to the renderer"
         );
      }

      // The control: the same document uncompressed still resolves, so this is about the
      // compression and not about nested documents being refused wholesale.
      for mime in ["image/svg+xml", "text/plain"] {
         assert!(
            resolve(
               mime,
               Arc::new(RED_SVG.as_bytes().to_vec()),
               &options
            )
            .is_some(),
            "{mime}: an uncompressed nested document must still render"
         );
      }
   }

   /// The same refusal from the outside: a document that embeds a compressed picture renders
   /// without it, rather than allocating for it.
   #[test]
   fn svg_art_does_not_inflate_a_nested_document() {
      let nested = |mime: &str, payload: &[u8]| {
         format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><image href="data:{mime};base64,{}" x="0" y="0" width="10" height="10"/></svg>"##,
            encode_base64(payload)
         )
      };

      let prepared = prepare_image_data(nested("text/plain", &gzip(RED_SVG.as_bytes())).as_bytes())
         .expect("the document renders");
      let (x64, _) = raster_renderings(&prepared);
      assert!(
         !draws_anything(x64),
         "a compressed nested picture must not be drawn"
      );

      // The control: the same picture inline and uncompressed does draw, so the assertion above is
      // about the compression rather than about the renderer being broken.
      let prepared = prepare_image_data(nested("image/svg+xml", RED_SVG.as_bytes()).as_bytes())
         .expect("the document renders");
      let (x64, _) = raster_renderings(&prepared);
      assert!(
         draws_anything(x64),
         "an inline picture must still render"
      );
   }

   /// A nested raster is decoded by resvg at render time, where [`art_limits`] does not reach, so the
   /// resolver applies the same edge to the size the payload declares — the nested twin of
   /// `art_past_the_decode_edge_cap_is_refused`.
   #[test]
   fn a_nested_raster_past_the_decode_edge_is_refused() {
      let options = svg_options();
      let resolve = &options.image_href_resolver.resolve_data;

      assert!(
         resolve(
            "image/png",
            Arc::new(encode_png(MAX_ART_EDGE + 1, 1)),
            &options
         )
         .is_none(),
         "a nested raster past the decode edge must not be handed over"
      );
      assert!(
         resolve(
            "image/png",
            Arc::new(encode_png(MAX_ART_EDGE, 1)),
            &options
         )
         .is_some(),
         "the control: a nested raster inside the edge still resolves"
      );
   }

   #[test]
   fn a_gzip_download_is_inflated_under_a_bound() {
      // Small enough to inflate, and handed on plain.
      assert_eq!(
         inflate_svgz_if_needed(&gzip(b"<svg/>")).unwrap().as_ref(),
         b"<svg/>"
      );
      // A document that is not a gzip stream is passed through untouched.
      assert_eq!(
         inflate_svgz_if_needed(b"<svg/>").unwrap().as_ref(),
         b"<svg/>"
      );

      // Past the bound it is refused, not truncated — and the compressed form is a few kilobytes,
      // which is the whole point of capping here rather than on the download.
      let bomb = gzip(&vec![0u8; MAX_INFLATED_SVG + 1]);
      assert!(bomb.len() < 64 * 1024);
      assert!(inflate_svgz_if_needed(&bomb).is_err());
   }

   /// `bytes` as a gzip stream, for the SVGZ cases.
   fn gzip(bytes: &[u8]) -> Vec<u8> {
      use flate2::Compression;
      use flate2::write::GzEncoder;
      use std::io::Write;

      let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
      encoder.write_all(bytes).unwrap();
      encoder.finish().unwrap()
   }

   /// `bytes` as standard base64, for the `data:` URIs these tests build by hand. The decoder's
   /// own tests are `base64_decodes_every_spelling_found_in_data_uris`.
   fn encode_base64(bytes: &[u8]) -> String {
      const ALPHABET: &[u8; 64] =
         b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

      let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
      for chunk in bytes.chunks(3) {
         let high = u32::from(chunk[0]) << 16;
         let mid = u32::from(*chunk.get(1).unwrap_or(&0)) << 8;
         let low = u32::from(*chunk.get(2).unwrap_or(&0));
         let n = high | mid | low;

         out.push(ALPHABET[(n >> 18) as usize & 63] as char);
         out.push(ALPHABET[(n >> 12) as usize & 63] as char);
         out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
         } else {
            '='
         });
         out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
         } else {
            '='
         });
      }
      out
   }

   /// A real 8×8 PNG, inline. Smaller than the thumbnail edge on purpose, so the resize tests also
   /// prove we never upscale.
   const PNG_URI: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAYAAADED76LAAAAGklEQVR42mPQiFrw/wQQ46IZ8EmCaIZhYQIAW1icQe9Ao+YAAAAASUVORK5CYII=";

   /// A 10x10 SVG with a red square, at an explicit size so the rendered sizes are exact. The red
   /// square is also what the href test looks for the absence of.
   const RED_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 10 10"><rect width="10" height="10" fill="#ff0000"/></svg>"##;

   /// A metadata document in the shape OpenSea uses. Built by `format!` rather than pasted as one
   /// blob so the image URI stays reviewable.
   fn metadata_uri_with(image_uri: &str) -> String {
      format!(
         r#"data:application/json,{{"name":"Test #1","description":"fixture","image":"{image_uri}","attributes":[{{"trait_type":"Background","value":"Blue"}}]}}"#
      )
   }

   fn decoded_edge(png: &[u8]) -> (u32, u32) {
      let image = image::load_from_memory(png).expect("rendering must be a valid PNG");
      (image.width(), image.height())
   }

   /// A solid PNG of the given size: a row of one colour compresses to a few KB however wide it is,
   /// which is what makes the decode cap, not the transfer cap, the thing under test.
   fn encode_png(width: u32, height: u32) -> Vec<u8> {
      let source = image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 255]));
      let mut bytes = Vec::new();
      source
         .write_to(
            &mut Cursor::new(&mut bytes),
            image::ImageFormat::Png,
         )
         .unwrap();
      bytes
   }

   /// [`MAX_BYTES`] bounds the transfer, not the decode: a few-KB PNG that declares 8192×8192 decodes
   /// to ~256 MiB, and one list load runs [`NFT_ART_FETCH_PER_LOAD`] of those at once. The decode
   /// limits bound both edges, so a source past them is refused — by the limit, not by another
   /// failure — while one inside the cap still decodes.
   #[test]
   fn art_past_the_decode_edge_cap_is_refused() {
      let oversized = encode_png(MAX_ART_EDGE + 1, 1);
      assert!(
         oversized.len() < 16 * 1024,
         "the fixture is tiny on the wire, so only the decode limit can stop it ({} bytes)",
         oversized.len()
      );

      let err = render_two_sizes(&oversized).expect_err("4097 wide is past the edge cap");
      assert!(
         matches!(
            err.downcast_ref::<image::ImageError>(),
            Some(image::ImageError::Limits(_))
         ),
         "the decode limit must be what refused it: {err}"
      );

      assert!(
         render_two_sizes(&encode_png(MAX_ART_EDGE, 1)).is_ok(),
         "a source inside the cap must still decode, so the limit is not refusing everything"
      );
   }

   #[test]
   fn resolves_the_uri_forms_that_actually_appear_on_chain() {
      assert_eq!(
         resolve_uri("ipfs://QmeSjSinHpPnmXmspMjwiXyN6zS4E9zccariGR3jxcaWtq/1"),
         Some(ResolvedUri::Ipfs(
            "QmeSjSinHpPnmXmspMjwiXyN6zS4E9zccariGR3jxcaWtq/1".to_string()
         ))
      );
      // `ipfs://ipfs/<cid>` must not repeat the segment in the gateway path.
      assert_eq!(
         resolve_uri("ipfs://ipfs/QmX/1.json"),
         Some(ResolvedUri::Ipfs("QmX/1.json".to_string()))
      );
      // Schemes arrive in any case.
      assert_eq!(
         resolve_uri("IPFS://QmY"),
         Some(ResolvedUri::Ipfs("QmY".to_string()))
      );
      assert_eq!(
         resolve_uri("ar://abc123"),
         Some(ResolvedUri::Arweave("abc123".to_string()))
      );
      assert_eq!(
         resolve_uri("https://example.invalid/a.json"),
         Some(ResolvedUri::Http(
            "https://example.invalid/a.json".to_string()
         ))
      );

      assert_eq!(
         resolve_uri(PNG_URI),
         Some(ResolvedUri::Data(
            base64_decode(PNG_URI.trim_start_matches("data:image/png;base64,")).unwrap()
         ))
      );

      // Nothing fetchable rather than an error.
      assert_eq!(resolve_uri(""), None);
      assert_eq!(resolve_uri("   "), None);
      assert_eq!(
         resolve_uri("QmeSjSinHpPnmXmspMjwiXyN6zS4E9zccariGR3jxcaWtq"),
         None
      );
      assert_eq!(resolve_uri("ftp://example.invalid/a"), None);
      assert_eq!(resolve_uri("ipfs://"), None);
   }

   /// An inline `data:` payload is capped like every fetched URI.
   ///
   /// Nothing is transferred for a `data:` URI, but [`MAX_BYTES`] bounds what gets decoded and held —
   /// and a `tokenURI` that is itself a `data:` document is exactly the case the enclosing fetch cannot
   /// bound, because it only bounds the fetch.
   #[tokio::test]
   async fn an_oversized_data_uri_is_refused() {
      let over = ResolvedUri::Data(vec![0u8; MAX_BYTES + 1]);
      assert!(
         fetch_resolved(over).await.is_err(),
         "over the cap must be refused rather than decoded"
      );

      let under = ResolvedUri::Data(vec![0u8; 16]);
      assert_eq!(
         fetch_resolved(under).await.expect("under the cap"),
         Some(vec![0u8; 16]),
         "and an ordinary inline payload still comes through"
      );
   }

   /// A URI is fetched over `https` from a public host, or not at all.
   ///
   /// The host half is the one that matters: the URI comes from a contract, so "fetch this" means
   /// "send a GET there from the user's machine" — and a private address turns art into a probe of
   /// whatever else is listening locally. The v4 spellings that the URL spec itself normalises
   /// (`0x7f000001`, `2130706433`) are in the list because a filter that reads the text would miss
   /// them while the connection would not.
   #[test]
   fn only_public_https_hosts_are_fetchable() {
      assert_eq!(
         resolve_uri("http://example.invalid/a.json"),
         None,
         "plaintext is not fetched at all"
      );
      assert!(resolve_uri("https://example.invalid/a.json").is_some());

      let fetchable = |host: &str| {
         let url = reqwest::Url::parse(&format!("https://{host}/a.json")).expect("a url");
         url_is_fetchable(&url)
      };

      for host in [
         "127.0.0.1",
         "0x7f000001",
         "2130706433",
         "10.0.0.5",
         "172.16.9.9",
         "192.168.1.1",
         "169.254.169.254",
         "0.0.0.0",
         "100.64.0.1",
         "198.18.0.1",
         "240.0.0.1",
         "localhost",
         "foo.local",
         "box.internal",
         "service.home.arpa",
      ] {
         assert!(!fetchable(host), "{host} must not be fetchable");
      }

      // The v6 spellings, including the ones that are really v4 — both the mapped form and the
      // IPv4-compatible one, which is the same address written with a zero prefix.
      for host in [
         "[::1]",
         "[::]",
         "[fd00::1]",
         "[fe80::1]",
         "[::ffff:127.0.0.1]",
         "[::127.0.0.1]",
         "[::10.0.0.5]",
         "[::169.254.169.254]",
      ] {
         assert!(!fetchable(host), "{host} must not be fetchable");
      }

      // The ordinary internet is untouched.
      for host in [
         "1.1.1.1",
         "8.8.8.8",
         "[2606:4700:4700::1111]",
         "example.invalid",
      ] {
         assert!(fetchable(host), "{host} must stay fetchable");
      }
   }

   /// The gate `get_with_cap` actually calls, which is the same verdict — a literal address needs no
   /// lookup, so this stays offline.
   #[tokio::test]
   async fn a_private_or_plaintext_url_is_refused_before_the_request() {
      let private = reqwest::Url::parse("https://127.0.0.1:8545/a.json").expect("a url");
      assert!(ensure_fetchable(&private).await.is_err());

      let plaintext = reqwest::Url::parse("http://example.invalid/a.json").expect("a url");
      assert!(ensure_fetchable(&plaintext).await.is_err());
   }

   /// A redirect is followed by hand, and the URL it points at must pass the same check the first URL
   /// did — the part a client redirect policy cannot do, since its callback cannot resolve a name.
   ///
   /// `Location` is often relative (`/ipfs/QmX`), so it is resolved against the URL that produced it
   /// rather than used as written.
   #[test]
   fn a_redirect_is_resolved_against_its_source_and_must_still_be_fetchable() {
      let url = reqwest::Url::parse("https://gateway.example.invalid/ipfs/QmX/1.png").unwrap();
      let location = reqwest::header::HeaderValue::from_static;

      // Not a redirect: nothing to follow.
      assert_eq!(
         redirect_target(&url, reqwest::StatusCode::OK, None).unwrap(),
         None
      );

      // An absolute target.
      assert_eq!(
         redirect_target(
            &url,
            reqwest::StatusCode::FOUND,
            Some(&location("https://cdn.example.invalid/1.png")),
         )
         .unwrap(),
         Some(reqwest::Url::parse("https://cdn.example.invalid/1.png").unwrap())
      );

      // A relative one, resolved against the URL that answered.
      assert_eq!(
         redirect_target(
            &url,
            reqwest::StatusCode::FOUND,
            Some(&location("/art/1.png")),
         )
         .unwrap(),
         Some(reqwest::Url::parse("https://gateway.example.invalid/art/1.png").unwrap())
      );

      // A redirect that says nothing usable is a failure, not a miss.
      assert!(
         redirect_target(&url, reqwest::StatusCode::FOUND, None).is_err(),
         "a redirect with no Location cannot be followed"
      );

      // A hop that lowers the scheme or lands on a private address is not fetchable, so the loop
      // refuses it on the next turn — which the name-only check could not tell.
      for target in [
         "http://gateway.example.invalid/1.png",
         "https://127.0.0.1/1.png",
         "https://169.254.169.254/latest/meta-data/",
         "https://foo.internal/1.png",
      ] {
         let next = redirect_target(
            &url,
            reqwest::StatusCode::FOUND,
            Some(&location(target)),
         )
         .unwrap()
         .expect("a location to follow");

         assert!(
            !url_is_fetchable(&next),
            "{target} must not be fetchable at the next hop"
         );
      }
   }

   /// The cap has to bite *mid-stream*.
   ///
   /// With no `Content-Length` — chunked, or anything else a hostile server feels like sending — the
   /// only bound `Response::bytes` left was the timeout, so a fetch could hold hundreds of megabytes
   /// before it was refused. The server here writes well past the cap and then holds the connection
   /// open: nothing but the cap can end the read, so a buffering reader hangs until the clock below
   /// fires instead of returning.
   #[tokio::test]
   async fn a_body_that_passes_the_cap_ends_the_read() {
      use tokio::io::AsyncWriteExt;

      // A bare client, deliberately: this is `read_body`, and the guard that would refuse
      // `http://127.0.0.1` is not what is under test here.
      let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
      let addr = listener.local_addr().unwrap();

      let server = tokio::spawn(async move {
         let (mut socket, _) = listener.accept().await.unwrap();
         let head =
            b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nTransfer-Encoding: chunked\r\n\r\n";
         socket.write_all(head).await.unwrap();

         let chunk = vec![b'a'; 32 * 1024];
         for _ in 0..12 {
            let header = format!("{:x}\r\n", chunk.len());
            let wrote = async {
               socket.write_all(header.as_bytes()).await?;
               socket.write_all(&chunk).await?;
               socket.write_all(b"\r\n").await
            };

            if wrote.await.is_err() {
               return;
            }
         }

         // The point of the test: the connection stays open, so only the cap can stop the reader.
         tokio::time::sleep(Duration::from_secs(60)).await;
      });

      let mut response = reqwest::Client::new()
         .get(format!("http://{addr}/art.png"))
         .send()
         .await
         .unwrap();

      let read = tokio::time::timeout(
         Duration::from_secs(5),
         read_body(&mut response, 128 * 1024, "http://large/art.png"),
      )
      .await
      .expect("the cap has to end the read, not the clock");

      assert!(read.is_err(), "a body past the cap is an error");

      server.abort();
   }

   #[test]
   fn base64_decodes_every_spelling_found_in_data_uris() {
      assert_eq!(base64_decode("").unwrap(), b"");
      assert_eq!(base64_decode("Zg==").unwrap(), b"f");
      assert_eq!(base64_decode("Zm8=").unwrap(), b"fo");
      assert_eq!(base64_decode("Zm9v").unwrap(), b"foo");
      assert_eq!(base64_decode("Zm9vYmE=").unwrap(), b"fooba");
      assert_eq!(base64_decode("Zm9vYmFy").unwrap(), b"foobar");

      // Unpadded and URL-safe spellings both appear in the wild.
      assert_eq!(base64_decode("Zg").unwrap(), b"f");
      assert_eq!(base64_decode("Zm8").unwrap(), b"fo");
      assert_eq!(
         base64_decode("--__").unwrap(),
         base64_decode("++//").unwrap()
      );

      // Wrapped payloads: whitespace is skipped, not decoded as data.
      assert_eq!(base64_decode("Zm9v\n YmFy").unwrap(), b"foobar");

      assert_eq!(
         base64_decode("Zm9v£"),
         None,
         "a non-alphabet byte is not ignored"
      );
   }

   #[test]
   fn percent_decoding_handles_escapes_and_passes_raw_text_through() {
      assert_eq!(percent_decode("hello%20world"), b"hello world");
      assert_eq!(
         percent_decode("%89PNG"),
         vec![0x89, b'P', b'N', b'G']
      );
      // Unescaped JSON, which some contracts emit verbatim.
      assert_eq!(
         percent_decode(r#"{"a":"b"}"#),
         br#"{"a":"b"}"#.to_vec()
      );
      // A truncated escape stays verbatim rather than eating the rest.
      assert_eq!(percent_decode("abc%4"), b"abc%4");
   }

   #[test]
   fn parses_data_uri_payloads() {
      assert_eq!(
         parse_data_uri("data:text/plain,hello%20world").unwrap(),
         b"hello world"
      );
      assert_eq!(
         parse_data_uri("data:application/json;base64,eyJhIjoxfQ==").unwrap(),
         br#"{"a":1}"#.to_vec()
      );
      assert_eq!(
         parse_data_uri("data:text/plain"),
         None,
         "no comma, no payload"
      );
      assert_eq!(parse_data_uri("not-a-data-uri"), None);
   }

   #[test]
   fn reads_the_image_field_from_metadata() {
      let json = format!(r#"{{"name":"x","image":"{PNG_URI}"}}"#);
      assert_eq!(
         image_url_from_metadata(json.as_bytes()),
         Some(PNG_URI.to_string())
      );

      // A padded image URI is trimmed.
      assert_eq!(
         image_url_from_metadata(br#"{"image":"  ipfs://QmX  "}"#),
         Some("ipfs://QmX".to_string())
      );

      // Nothing usable: not JSON, no image, empty image, or a non-string image object.
      assert_eq!(image_url_from_metadata(b"not json"), None);
      assert_eq!(image_url_from_metadata(br#"{"name":"x"}"#), None);
      assert_eq!(
         image_url_from_metadata(br#"{"image":"   "}"#),
         None
      );
      assert_eq!(
         image_url_from_metadata(br#"{"image":{"uri":"ipfs://QmX"}}"#),
         None
      );

      assert!(looks_like_json(br#"  {"a":1}"#));
      assert!(!looks_like_json(&[0x89, b'P', b'N', b'G']));
   }

   /// The two renderings, which is all art has been since vector art stopped being stored as source.
   fn raster_renderings(data: &NftIconData) -> (&[u8], &[u8]) {
      (&data.x64, &data.x250)
   }

   /// The red-square document as a `data:` URI, so the fetch path needs no network.
   fn red_svg_uri() -> String {
      format!("data:image/svg+xml,{RED_SVG}")
   }

   /// One rendering's pixels.
   fn pixels(png: &[u8]) -> image::RgbaImage {
      image::load_from_memory(png).expect("a rendering is a PNG").to_rgba8()
   }

   /// Whether a rendering drew anything at all.
   fn draws_anything(png: &[u8]) -> bool {
      pixels(png).pixels().any(|pixel| pixel.0[3] != 0)
   }

   /// Formats the grid must actually be able to show. Each fixture is a real file, so this fails
   /// loudly when a decoder is missing from the dependency features instead of silently turning
   /// every JPEG/WebP/GIF token into a placeholder.
   #[test]
   fn decodes_the_art_formats_that_appear_on_chain() {
      let cases: [(&str, &[u8]); 3] = [
         ("jpeg", include_bytes!("testdata/tiny.jpg")),
         ("webp", include_bytes!("testdata/tiny.webp")),
         ("gif", include_bytes!("testdata/tiny.gif")),
      ];

      for (name, bytes) in cases {
         let data = prepare_image_data(bytes)
            .unwrap_or_else(|e| panic!("{name} art must decode, got: {e}"));
         let (x64, _) = raster_renderings(&data);

         assert_eq!(decoded_edge(x64), (8, 8), "{name} thumbnail");
      }
   }

   #[test]
   fn renders_both_sizes_and_never_upscales() {
      let data = parse_data_uri(PNG_URI).unwrap();
      let prepared = prepare_image_data(&data).unwrap();
      let (x64, x250) = raster_renderings(&prepared);

      // The source is 8×8, smaller than both edges: both renderings must keep the original size.
      assert_eq!(decoded_edge(x64), (8, 8));
      assert_eq!(decoded_edge(x250), (8, 8));
      // ...and so they are legitimately the same bytes. Nothing is upscaled, so the "thumbnail"
      // and the "detail" copy are the same picture here; they only differ when there is shrinkage.
      assert_eq!(x64, x250);
   }

   #[test]
   fn shrinks_large_art_to_fit_each_edge() {
      // A wide 800×200 source: aspect ratio is preserved, so the thumbnail is 64×16.
      let source = image::RgbaImage::from_pixel(800, 200, image::Rgba([10, 20, 30, 255]));
      let mut bytes = Vec::new();
      source
         .write_to(
            &mut Cursor::new(&mut bytes),
            image::ImageFormat::Png,
         )
         .unwrap();

      let prepared = prepare_image_data(&bytes).unwrap();
      let (x64, x250) = raster_renderings(&prepared);

      let (w64, h64) = decoded_edge(x64);
      assert_eq!(
         (w64, h64),
         (64, 16),
         "aspect ratio is preserved, not cropped"
      );

      let (w250, h250) = decoded_edge(x250);
      assert_eq!(w250, 250);
      assert!(
         h250 == 62 || h250 == 63,
         "the 250-wide rendering must keep the aspect ratio, got {h250} (200/800 × 250 = 62.5)"
      );

      // Two clearly different sizes, so the two renderings must be different pictures.
      assert_ne!(
         x64, x250,
         "a thumbnail and a detail copy must not be the same bytes when they are different sizes"
      );
   }

   /// End to end with no network: metadata URI → document → inline image → both renderings.
   #[tokio::test]
   async fn fetches_an_inline_metadata_document_end_to_end() {
      let data = fetch_nft_icon(&metadata_uri_with(PNG_URI), U256::from(1))
         .await
         .unwrap()
         .expect("an inline document with an inline image must resolve");

      let (x64, x250) = raster_renderings(&data);
      assert_eq!(decoded_edge(x64), (8, 8));
      assert_eq!(decoded_edge(x250), (8, 8));
   }

   /// A URI that already *is* the picture needs no metadata hop.
   #[tokio::test]
   async fn fetches_a_direct_image_uri() {
      let data = fetch_nft_icon(PNG_URI, U256::from(1)).await.unwrap();
      assert!(
         data.is_some(),
         "an image URI must be used as the image"
      );
   }

   #[tokio::test]
   async fn nothing_to_show_is_not_an_error() {
      // No metadata, no image field, or nothing fetchable: all "show the placeholder".
      for uri in [
         "",
         "QmBareCidWithoutAScheme",
         "ipfs://",
         &metadata_uri_with("ipfs://"),
      ] {
         let result = fetch_nft_icon(uri, U256::from(1)).await.unwrap();
         assert!(
            result.is_none(),
            "{uri} should yield no image, not an error"
         );
      }
   }

   /// SVG art is rasterised here, at the two sizes the views ask for, rather than kept as source.
   #[tokio::test]
   async fn svg_art_is_rasterised_at_the_views_sizes() {
      let data = fetch_nft_icon(&red_svg_uri(), U256::from(1))
         .await
         .unwrap()
         .expect("SVG must render, not be dropped as undecodable");

      let (x64, x250) = raster_renderings(&data);
      assert_eq!(
         decoded_edge(x64),
         (10, 10),
         "a 10x10 source is never upscaled"
      );
      assert_eq!(decoded_edge(x250), (10, 10));
      assert!(
         draws_anything(x64),
         "the red square must survive the round trip"
      );
   }

   /// A vector document is shrunk to fit each edge, exactly as raster art is.
   #[test]
   fn a_large_svg_is_shrunk_to_fit_each_edge() {
      // An 800x200 source: aspect ratio is preserved, so the thumbnail is 64x16.
      let wide = r##"<svg xmlns="http://www.w3.org/2000/svg" width="800" height="200"><rect width="800" height="200" fill="#ff0000"/></svg>"##;

      let prepared = prepare_image_data(wide.as_bytes()).unwrap();
      let (x64, x250) = raster_renderings(&prepared);

      assert_eq!(
         decoded_edge(x64),
         (64, 16),
         "aspect ratio is preserved, not cropped"
      );

      let (large_w, large_h) = decoded_edge(x250);
      assert_eq!(large_w, 250);
      assert!(
         large_h == 62 || large_h == 63,
         "the 250-wide rendering keeps the aspect ratio, got {large_h} (200/800 x 250 = 62.5)"
      );
   }

   /// Vector art cannot reach the filesystem.
   ///
   /// usvg's default href resolver treats a non-`data:` reference as a **path** and
   /// `std::fs::read`s it, and a referenced *SVG* is drawn — so without [`svg_options`] a
   /// collection's art would put a file from the user's disk on screen. The reference must be
   /// refused, and the renderer must still work for everything else.
   #[test]
   fn svg_art_cannot_reference_a_file() {
      let dir = std::env::temp_dir().join("zeus_nft_icon_href");
      std::fs::create_dir_all(&dir).unwrap();
      let target = dir.join("referenced.svg");
      std::fs::write(&target, RED_SVG).unwrap();

      let referencing = format!(
         r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><image href="{}" x="0" y="0" width="10" height="10"/></svg>"##,
         target.display()
      );

      let prepared = prepare_image_data(referencing.as_bytes()).expect("the document renders");
      let (x64, _) = raster_renderings(&prepared);
      assert!(
         !draws_anything(x64),
         "a referenced file must not be read, let alone drawn"
      );

      // The control: the same red square inline does draw, so the assertion above is about the
      // href and not about the renderer being broken.
      let prepared = prepare_image_data(RED_SVG.as_bytes()).expect("the document renders");
      let (x64, _) = raster_renderings(&prepared);
      assert!(
         draws_anything(x64),
         "an inline document must still render"
      );
   }

   /// The sniff has to survive the shapes SVG actually arrives in, and must never claim a raster
   /// picture: a false positive would hand a PNG to egui's SVG renderer.
   #[test]
   fn recognises_vector_art_from_its_content() {
      assert!(is_svg(
         b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"
      ));
      assert!(
         is_svg(b"  \n\t<svg/>"),
         "leading whitespace is legal"
      );
      assert!(is_svg(b"<?xml version=\"1.0\"?>\n<svg/>"));
      assert!(
         is_svg(&[&[0xEF, 0xBB, 0xBF][..], b"<svg/>"].concat()),
         "a UTF-8 BOM is legal"
      );

      assert!(
         !is_svg(b"\x89PNG\r\n\x1a\n"),
         "PNG must not be taken for vector art"
      );
      assert!(
         !is_svg(b"{\"image\":\"ipfs://x\"}"),
         "JSON metadata is not art"
      );
      assert!(!is_svg(b""));
   }

   /// `{id}` is expanded before resolving, because ERC-1155 `uri` bodies are templates.
   #[tokio::test]
   async fn expands_the_id_placeholder_before_resolving() {
      let template = metadata_uri_with("ipfs://QmX/{id}.png");
      // Not fetchable (BareCid), but the point is that resolution sees the expanded form: an
      // unexpanded `{id}` would be a valid CID suffix instead of a placeholder.
      let expanded = expand_id_placeholder(&template, U256::from(7)).expect("expands");
      assert!(expanded.contains(&format!("{:064x}.png", 7)));
      assert!(!expanded.contains("{id}"));
   }

   /// A template whose expansion would pass the bound is a **miss**, not a fetch. `resolve_uri` accepts
   /// any known scheme, so the unexpanded `{id}` would be a live request with a literal placeholder in
   /// its path — and one more on every list load, since a miss caches nothing.
   #[tokio::test]
   async fn a_template_past_the_expansion_bound_is_a_miss_not_a_fetch() {
      // Loopback is refused by the SSRF guard before anything connects, so this fails pre-fix with the
      // template *having been handed on*, and never touches the network either way.
      let template = format!(
         "http://127.0.0.1:9/{}{{id}}",
         "x".repeat(MAX_EXPANDED_URI_LEN)
      );
      assert!(
         expand_id_placeholder(&template, U256::from(1)).is_none(),
         "the fixture must be past the bound"
      );

      match fetch_nft_icon(&template, U256::from(1)).await {
         Ok(None) => {}
         Ok(Some(_)) => panic!("a template past the bound is not a URI, and it drew something"),
         Err(e) => panic!("the template was handed to the fetcher: {e}"),
      }
   }

   /// A document nesting `payload` as an inline image, the way a collection's art carries one.
   fn nested_document(mime: &str, payload: &[u8]) -> String {
      format!(
         r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><image href="data:{mime};base64,{}" x="0" y="0" width="10" height="10"/></svg>"##,
         encode_base64(payload)
      )
   }

   /// A document that declares an entity and references it `refs` times.
   ///
   /// One MiB of definition inside a document of just over a MiB, which is the shape whose cost is
   /// `refs × entity size` rather than the document's own.
   fn entity_bomb(refs: usize) -> String {
      format!(
         "<?xml version=\"1.0\"?><!DOCTYPE svg [<!ENTITY a \"{}\">]><svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"><text x=\"0\" y=\"5\">{}</text></svg>",
         "A".repeat(1024 * 1024),
         "&a;".repeat(refs)
      )
   }

   /// An entity declaration is the one construct whose expansion roxmltree does not bound in bytes:
   /// its guard counts references (255 per reference, 10 deep), so a document of at most
   /// [`MAX_INFLATED_SVG`] expands to gigabytes of text — measured, 250 references of a 1 MiB entity
   /// had to be killed at 60 s, off a 1.2 KiB download. Refused before the parser sees it.
   #[test]
   fn a_document_that_declares_an_entity_is_refused() {
      let err = render_svg(entity_bomb(10).as_bytes())
         .expect_err("a document that declares an entity must not reach the parser");

      assert!(
         err.to_string().contains("entity"),
         "the refusal has to say what it refused: {err}"
      );
   }

   /// The control: the prolog Inkscape-style art opens with declares no entity, so it still renders.
   #[test]
   fn a_doctype_prolog_without_an_entity_still_renders() {
      let doc = format!(
         "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">\n{RED_SVG}"
      );

      let (x64, _) = render_svg(doc.as_bytes()).expect("a plain doctype declares nothing");
      assert!(
         draws_anything(&x64),
         "the art must still be drawn"
      );
   }

   /// The same refusal one level down: usvg parses an embedded document with the same options it
   /// gives the top level, so the check has to cover the nested path too.
   #[test]
   fn a_nested_document_that_declares_an_entity_is_refused() {
      let options = svg_options();
      let resolve = &options.image_href_resolver.resolve_data;
      let bomb = entity_bomb(10).into_bytes();

      assert!(
         resolve("image/svg+xml", Arc::new(bomb.clone()), &options).is_none(),
         "a nested document that declares an entity must not be handed to the parser"
      );
      // ...and for `text/plain`, which is the spelling a mime-less `data:` gets.
      assert!(
         resolve("text/plain", Arc::new(bomb), &options).is_none(),
         "the refusal must not depend on how the mime spells the document"
      );

      let bomb = nested_document("image/svg+xml", entity_bomb(10).as_bytes());
      let prepared = prepare_image_data(bomb.as_bytes()).expect("the outer document still renders");

      let (x64, _) = raster_renderings(&prepared);
      assert!(
         !draws_anything(x64),
         "a nested document that declares an entity must not be parsed"
      );

      // The control: the same nesting with a document that declares nothing does draw, so this is
      // about the declaration and not about nested documents being refused wholesale.
      let clean = nested_document("image/svg+xml", RED_SVG.as_bytes());
      let prepared = prepare_image_data(clean.as_bytes()).expect("the outer document renders");

      let (x64, _) = raster_renderings(&prepared);
      assert!(
         draws_anything(x64),
         "an inline document must still render"
      );
   }

   /// A raster whose declared size cannot be read is refused rather than handed on: every bound in
   /// the resolver is expressed in that size, so there would be nothing left to bound it with.
   #[test]
   fn a_nested_raster_without_a_readable_size_is_refused() {
      let options = svg_options();
      let resolve = &options.image_href_resolver.resolve_data;

      assert!(
         resolve(
            "image/png",
            Arc::new(b"not a picture".to_vec()),
            &options
         )
         .is_none(),
         "a raster mime whose bytes declare no size must not be handed over"
      );
   }

   /// Nested rasters are bounded per payload by the declared edge, and per *document* by the pixel
   /// budget: ~48 payloads of `MAX_ART_EDGE`² fit a 4 MiB document as base64, and a renderer that
   /// decoded them would work through ~3 GiB. One full-size source is the budget.
   #[test]
   fn nested_raster_pixels_are_bounded_per_document() {
      let options = svg_options();
      let resolve = &options.image_href_resolver.resolve_data;
      let full_size = Arc::new(encode_png(MAX_ART_EDGE, MAX_ART_EDGE));

      assert!(
         resolve("image/png", full_size.clone(), &options).is_some(),
         "one full-size raster is exactly the budget for one document"
      );
      assert!(
         resolve("image/png", full_size, &options).is_none(),
         "the second is past it, however small the document that carries both is"
      );
   }

   /// `resvg` is built without `raster-images` (see `Cargo.toml`), so a nested raster is resolved by
   /// usvg and then never decoded — which is why the resolver's raster checks bound a decoder that
   /// is not in the build. If this fails, that feature has been enabled: the per-document pixel
   /// budget is now load-bearing and the decode it bounds now happens.
   #[test]
   fn a_nested_raster_is_not_decoded_in_this_build() {
      let png = parse_data_uri(PNG_URI).expect("the fixture is a data: URI");
      let doc = nested_document("image/png", &png);
      let prepared = prepare_image_data(doc.as_bytes()).expect("the document renders");

      let (x64, _) = raster_renderings(&prepared);
      assert!(
         !draws_anything(x64),
         "CANARY: resvg/raster-images is enabled — revisit art_data_resolver's raster bounds"
      );
   }

   /// `image` enforces the decode limits through one path (`ImageReader::decode`: `reserve` the
   /// output, then `set_limits`, whose default impl checks the dimensions) — but the *decoders* are
   /// what carry it, so each enabled format is pinned instead of PNG standing in for all four.
   #[test]
   fn the_decode_edge_cap_holds_for_every_enabled_raster_format() {
      let over = MAX_ART_EDGE + 1;
      let mut cases: Vec<(&str, Vec<u8>)> = vec![("png", encode_png(over, 1))];

      let rgb = image::RgbImage::from_pixel(over, 1, image::Rgb([10, 20, 30]));
      let mut jpeg = Vec::new();
      rgb.write_to(
         &mut Cursor::new(&mut jpeg),
         image::ImageFormat::Jpeg,
      )
      .unwrap();
      cases.push(("jpeg", jpeg));

      let rgba = image::RgbaImage::from_pixel(over, 1, image::Rgba([10, 20, 30, 255]));
      let mut gif = Vec::new();
      rgba
         .write_to(
            &mut Cursor::new(&mut gif),
            image::ImageFormat::Gif,
         )
         .unwrap();
      cases.push(("gif", gif));

      cases.push(("webp", wide_webp(over)));

      for (name, bytes) in cases {
         let err = render_two_sizes(&bytes).expect_err(&format!(
            "{name} past the decode edge must not decode"
         ));

         assert!(
            matches!(
               err.downcast_ref::<image::ImageError>(),
               Some(image::ImageError::Limits(_))
            ),
            "{name}: the decode limit has to be what refused it: {err}"
         );
      }
   }

   /// `testdata/tiny.webp` with the VP8 frame header's width rewritten.
   ///
   /// `image` has no WebP encoder to build one with, and the 14-bit field at the start of the frame
   /// header is the only place a decoder reads the width from — the top two bits of the second byte
   /// are the horizontal scale and are left as they are.
   fn wide_webp(width: u32) -> Vec<u8> {
      const WIDTH_AT: usize = 26;

      let mut bytes = include_bytes!("testdata/tiny.webp").to_vec();
      let [low, high] = (width as u16).to_le_bytes();
      bytes[WIDTH_AT] = low;
      bytes[WIDTH_AT + 1] = (bytes[WIDTH_AT + 1] & 0xC0) | (high & 0x3F);
      bytes
   }

   /// How many decodes run at once is a bound of its own — see [`MAX_CONCURRENT_ART_DECODES`] — and
   /// a decode has to wait for a slot rather than run alongside the others.
   #[tokio::test]
   async fn art_decodes_wait_for_a_slot() {
      // Other tests in this binary share the semaphore and may be holding a slot, so wait for all
      // of them rather than demanding they be free at once.
      let held = tokio::time::timeout(Duration::from_secs(5), async {
         let mut held = Vec::with_capacity(MAX_CONCURRENT_ART_DECODES);
         for _ in 0..MAX_CONCURRENT_ART_DECODES {
            held.push(
               ART_DECODE_SLOTS
                  .acquire()
                  .await
                  .expect("the art-decode semaphore is never closed"),
            );
         }
         held
      })
      .await
      .expect("shared with sibling tests: the art-decode slots never all came free");

      assert!(
         ART_DECODE_SLOTS.try_acquire().is_err(),
         "there must be no slot past MAX_CONCURRENT_ART_DECODES"
      );

      // The fetch for an inline URI is instant, so this is parked at the semaphore and nothing else.
      let waiting = tokio::time::timeout(
         Duration::from_millis(200),
         fetch_nft_icon(PNG_URI, U256::from(1)),
      )
      .await;
      assert!(
         waiting.is_err(),
         "a decode must not start while every slot is held"
      );

      drop(held);

      let decoded = fetch_nft_icon(PNG_URI, U256::from(1)).await.expect("the slots are back");
      assert!(decoded.is_some(), "and the decode completes");
   }

   /// A decode that ends in an error still gives its slot back — a permit leaked on a failure path
   /// would stall every later decode of the session.
   #[tokio::test]
   async fn a_failed_decode_gives_its_slot_back() {
      let garbage = "data:image/png;base64,bm90IGEgcGljdHVyZQ==";
      assert!(
         fetch_nft_icon(garbage, U256::from(1))
            .await
            .expect("an undecodable picture is not an error")
            .is_none()
      );

      // Other tests share this semaphore, so wait for it rather than demanding it be free at once.
      let deadline = std::time::Instant::now() + Duration::from_secs(5);
      while ART_DECODE_SLOTS.available_permits() < MAX_CONCURRENT_ART_DECODES {
         assert!(
            std::time::Instant::now() < deadline,
            "a slot was never released: {} of {MAX_CONCURRENT_ART_DECODES} free",
            ART_DECODE_SLOTS.available_permits()
         );
         tokio::time::sleep(Duration::from_millis(20)).await;
      }
   }

   /// Live end-to-end check over the real gateways: tokenURI → metadata document → image → both
   /// renderings. Ignored by default — it needs the public internet, and IPFS gateways are
   /// notoriously flaky (429s are expected), which is exactly why the gateway order is pinned.
   ///
   /// BAYC #1 is used because its chain was verified by hand when this pipeline was designed:
   /// `ipfs://QmeSj…/1` → metadata JSON → `ipfs://QmPbx…` → a 631×631 PNG.
   #[tokio::test]
   #[ignore = "needs the public internet (IPFS gateways)"]
   async fn fetches_real_ipfs_art_through_the_gateways() {
      let uri = "ipfs://QmeSjSinHpPnmXmspMjwiXyN6zS4E9zccariGR3jxcaWtq/1";
      let data = fetch_nft_icon(uri, U256::from(1))
         .await
         .unwrap()
         .expect("BAYC #1 must resolve to art on IPFS");

      let (x64, x250) = raster_renderings(&data);
      let (thumb_w, thumb_h) = decoded_edge(x64);
      let (large_w, large_h) = decoded_edge(x250);

      eprintln!("BAYC #1: thumbnail {thumb_w}x{thumb_h}, detail {large_w}x{large_h}");

      assert!(
         thumb_w <= THUMB_EDGE && thumb_h <= THUMB_EDGE,
         "thumbnail must fit inside {THUMB_EDGE}px, got {thumb_w}x{thumb_h}"
      );
      assert!(
         large_w <= LARGE_EDGE && large_h <= LARGE_EDGE,
         "detail copy must fit inside {LARGE_EDGE}px, got {large_w}x{large_h}"
      );
      assert!(thumb_w > 0 && large_w > 0);
   }
}
