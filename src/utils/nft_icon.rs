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

use crate::assets::icons::{NftIconData, save_nft_icon};
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use anyhow::anyhow;
use image::imageops::FilterType;
use std::io::Cursor;
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

/// IPFS gateways, tried in order.
///
/// The order is load-bearing. `ipfs.io` and `dweb.link` answer 429 ("service worker gateway only")
/// to non-browser clients, and `cloudflare-ipfs.com` no longer resolves, so the two that answered
/// reliably are tried first and the throttled pair is a last resort. Never trust one gateway: they
/// pin different content, so a miss on one is not a miss overall.
const IPFS_GATEWAYS: &[&str] = &[
   "https://gateway.pinata.cloud/ipfs",
   "https://4everland.io/ipfs",
   "https://ipfs.io/ipfs",
   "https://dweb.link/ipfs",
];

const ARWEAVE_GATEWAY: &str = "https://arweave.net";

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

   let http =
      strip_prefix_ci(uri, "https://").is_some() || strip_prefix_ci(uri, "http://").is_some();

   http.then(|| ResolvedUri::Http(uri.to_string()))
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

/// Decide what to store for a piece of art.
///
/// Vector art keeps its source: egui rasterises SVG at whatever size a view asks for, so
/// pre-rendering it would only lose detail and cost disk. Everything else is decoded once here and
/// stored as two renderings.
pub fn prepare_image_data(bytes: &[u8]) -> Result<NftIconData, anyhow::Error> {
   if is_svg(bytes) {
      return Ok(NftIconData::Svg(bytes.to_vec()));
   }

   let (x64, x250) = render_two_sizes(bytes)?;
   Ok(NftIconData::Raster { x64, x250 })
}

/// GET with a size cap.
///
/// `Ok(None)` is a definitive miss (404/410 or an empty body) — the caller stops asking. `Err` is
/// "cannot tell right now" (throttled, timed out, too large).
async fn get_with_cap(url: &str, max: usize) -> Result<Option<Vec<u8>>, anyhow::Error> {
   let response = http_client().get(url).send().await?;

   match response.status() {
      reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::GONE => return Ok(None),
      status if !status.is_success() => return Err(anyhow!("{url} returned {status}")),
      _ => {}
   }

   if let Some(len) = response.content_length() {
      if len as usize > max {
         return Err(anyhow!("{url} is too large ({len} bytes)"));
      }
   }

   let bytes = response.bytes().await?;

   if bytes.is_empty() {
      return Ok(None);
   }

   if bytes.len() > max {
      return Err(anyhow!(
         "{url} is too large ({} bytes)",
         bytes.len()
      ));
   }

   Ok(Some(bytes.to_vec()))
}

/// Fetch an IPFS path, trying every gateway before giving up.
///
/// `Ok(None)` only when every gateway reported a miss; a transient failure is remembered and
/// returned as an error so it is not mistaken for "this token has no image".
async fn fetch_ipfs(path: &str) -> Result<Option<Vec<u8>>, anyhow::Error> {
   let mut last_error = None;

   for gateway in IPFS_GATEWAYS {
      match get_with_cap(&format!("{gateway}/{path}"), MAX_BYTES).await {
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
      ResolvedUri::Data(bytes) => Ok(Some(bytes)),
      ResolvedUri::Http(url) => get_with_cap(&url, MAX_BYTES).await,
      ResolvedUri::Ipfs(path) => fetch_ipfs(&path).await,
      ResolvedUri::Arweave(id) => get_with_cap(&format!("{ARWEAVE_GATEWAY}/{id}"), MAX_BYTES).await,
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
/// **SVG** is not decoded at all: it is kept as vector art (see [`prepare_image_data`]), which is
/// both faithful and cheaper. Art in a format that is not enabled lands in `Ok(None)` with its
/// detected format logged, so a placeholder is diagnosable rather than mysterious.
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
      Ok(data) => Ok(Some(data)),
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

   const SVG_URI: &str = "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAxMCAxMCIvPg==";

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

   /// The two raster renderings, for the tests that are about raster art specifically.
   fn raster_renderings(data: &NftIconData) -> (&[u8], &[u8]) {
      match data {
         NftIconData::Raster { x64, x250 } => (x64, x250),
         NftIconData::Svg(_) => panic!("expected raster art, got vector"),
      }
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

   /// SVG art is kept as its source rather than rasterised: egui renders it at whatever size a view
   /// asks for, so it stays crisp in both the grid and the detail view.
   #[tokio::test]
   async fn svg_art_is_kept_as_vector() {
      let data = fetch_nft_icon(SVG_URI, U256::from(1))
         .await
         .unwrap()
         .expect("SVG must be kept, not dropped as undecodable");

      match data {
         NftIconData::Svg(svg) => assert!(is_svg(&svg)),
         NftIconData::Raster { .. } => panic!("SVG must not be rasterised"),
      }
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
