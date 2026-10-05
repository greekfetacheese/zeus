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
//! non-public address. A metadata URI comes from a contract, so "fetch this" is really "make the
//! user's machine send a GET there", and that is the whole of the attack.

use crate::assets::icons::{NftIconData, save_nft_icon};
use crate::core::urls::ZeusUrl;
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use anyhow::anyhow;
use image::imageops::FilterType;
use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg;
use std::io::Cursor;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::OnceLock;
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

/// How many art downloads one list load starts.
///
/// The cap is what keeps a wallet tracking hundreds of ids from firing hundreds of concurrent
/// gateway requests at once (the throttle that follows marks tokens failed for the session, so the
/// art would never appear). Tokens already tried are skipped, so successive loads work through a long
/// list instead of retrying the first few forever.
const NFT_ART_FETCH_PER_LOAD: usize = 24;

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
      IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
         // `::ffff:127.0.0.1` is the same address wearing a different hat.
         Some(v4) => is_public_v4(v4),
         None => {
            !(v6.is_loopback()
               || v6.is_unspecified()
               || v6.is_multicast()
               || v6.is_unique_local()
               || v6.is_unicast_link_local())
         }
      },
   }
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
/// Synchronous because the redirect policy cannot await, and a redirect is the second way in: a
/// harmless-looking public name only has to answer `302` with a private address to get the request
/// the URI itself was refused.
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
/// Redirects are still followed — a gateway may answer a CID with a `Location` — but every hop is
/// put through [`url_is_fetchable`], which is the check a URI cannot dodge by answering `302`.
fn http_client() -> &'static reqwest::Client {
   static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
   CLIENT.get_or_init(|| {
      reqwest::Client::builder()
         .user_agent("zeus-wallet")
         .timeout(FETCH_TIMEOUT)
         .redirect(reqwest::redirect::Policy::custom(|attempt| {
            // Ten is reqwest's own default; the scheme and address checks are the addition.
            if attempt.previous().len() >= 10 {
               return attempt.error(anyhow!("too many redirects"));
            }

            let url = attempt.url().clone();

            if !url_is_fetchable(&url) {
               return attempt.error(anyhow!("refusing to follow {url}"));
            }

            attempt.follow()
         }))
         .build()
         .unwrap_or_else(|_| reqwest::Client::new())
   })
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
   // it. So the *string* is bounded first: it is attacker-controlled (any contract the wallet reads can
   // return a multi-megabyte `tokenURI`) and nothing else caps it. The bound is generous enough for any
   // `data:` URI that decodes within `MAX_BYTES`.
   const MAX_URI_LEN: usize = MAX_BYTES * 4;
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
   let image = image::load_from_memory(bytes)?;

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

/// `usvg` options that cannot reach the machine.
///
/// `resolve_string` is the one hook with filesystem reach: usvg's default treats a non-`data:`
/// `href` as a **path** and `std::fs::read`s it (usvg-0.45.1 `parser/image.rs`), so a collection
/// whose art says `<image href="/etc/passwd"/>` — or names a device file that never ends — would
/// have Zeus open it, and a referenced SVG is *drawn*, so the picture would appear as the token's
/// art. Refusing every string reference removes that reach.
///
/// The `data:` half stays at the default because it is already closed: usvg parses a nested
/// document with `None` resolvers of its own (`load_sub_svg`), so an embedded SVG cannot name a
/// path either — and keeping it lets art that embeds an image inline still render.
fn svg_options() -> usvg::Options<'static> {
   let mut options = usvg::Options::default();
   options.image_href_resolver = usvg::ImageHrefResolver {
      resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
      resolve_string: Box::new(|_, _| None),
   };
   options
}

/// Rasterise vector art into the two renderings a view asks for.
///
/// Only ever shrinks, matching [`render_two_sizes`]: a small picture is never upscaled.
fn render_svg(bytes: &[u8]) -> Result<(Vec<u8>, Vec<u8>), anyhow::Error> {
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
fn is_svg(bytes: &[u8]) -> bool {
   // A UTF-8 BOM and leading whitespace are both legal ahead of the root element.
   let head = &bytes[..bytes.len().min(1024)];
   let head = head.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(head);
   let start: Vec<u8> =
      head.iter().copied().skip_while(|b| b.is_ascii_whitespace()).take(4).collect();

   start.starts_with(b"<svg") || start.starts_with(b"<?xm")
}

/// Decide what to store for a piece of art: two raster renderings, whichever way it arrived.
///
/// Vector art is rasterised here instead of being kept as source. A renderer handed SVG source
/// resolves `href` against the filesystem, which turns a collection's art into a local file read
/// and, for a referenced SVG, into a picture of a file on disk. Rendering it ourselves, under
/// [`svg_options`], keeps that reach out of the picture entirely. The two sizes are the only ones
/// the views ask for, so neither the grid nor the detail view loses anything.
pub fn prepare_image_data(bytes: &[u8]) -> Result<NftIconData, anyhow::Error> {
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

/// GET with a size cap, from a host the wallet is allowed to reach.
///
/// `Ok(None)` is a definitive miss (404/410 or an empty body) — the caller stops asking. `Err` is
/// "cannot tell right now" (throttled, timed out, refused, too large).
async fn get_with_cap(url: &str, max: usize) -> Result<Option<Vec<u8>>, anyhow::Error> {
   let url = reqwest::Url::parse(url).map_err(|e| anyhow!("{url} is not a usable url: {e}"))?;
   ensure_fetchable(&url).await?;

   let mut response = http_client().get(url.clone()).send().await?;

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

   read_body(&mut response, max, url.as_str()).await
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
   // The URI may still be a raw ERC-1155 template carrying `{id}`; expanding an already-expanded
   // URI is a no-op, so this is safe whichever layer expanded it.
   let uri = expand_id_placeholder(metadata_uri, token_id);

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

      let image_uri = expand_id_placeholder(&image_uri, token_id);
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

   match prepare_image_data(&image_bytes) {
      // The URI is stored with the art so a later change to it can be noticed at all: the cache is keyed
      // by `(collection, token id)`, which cannot tell on its own. What is recorded is the *metadata* URI
      // the fetch was asked for — the same string the caller compares against next time — and a change to
      // the document's contents behind an unchanged URI stays undetectable, by nature.
      Ok(mut data) => {
         data.source_uri = Some(uri);
         Ok(Some(data))
      }
      Err(e) => {
         // Not retryable: an undecodable picture will not become decodable. Log the format so a
         // placeholder is diagnosable instead of mysterious.
         let format = match image::guess_format(&image_bytes) {
            Ok(format) => format!("{format:?}"),
            Err(_) => "unrecognised".to_string(),
         };
         tracing::debug!("NFT image not decodable ({format}): {e}");
         Ok(None)
      }
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

      // The v6 spellings, including one that is really v4.
      for host in [
         "[::1]",
         "[::]",
         "[fd00::1]",
         "[fe80::1]",
         "[::ffff:127.0.0.1]",
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
      let expanded = expand_id_placeholder(&template, U256::from(7));
      assert!(expanded.contains(&format!("{:064x}.png", 7)));
      assert!(!expanded.contains("{id}"));
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
