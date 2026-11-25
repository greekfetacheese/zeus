pub mod fs;
#[cfg(test)]
pub mod runtime;
pub mod time;

pub use fs::{
   restrict_dir_to_owner, restrict_file_to_owner, write_private, write_private_atomic,
   write_private_from_reader,
};
pub use time::*;

/// Shorten an address for display: six characters from each end, joined by an ellipsis.
///
/// Anything too short to shorten comes back whole. The offsets are counted in *characters*, not
/// bytes: the input is whatever the caller has — a decoded event value, a name — and a byte offset
/// would split a multi-byte character and panic instead of shortening the label. Addresses render as
/// 42 ASCII characters, where the two counts agree, so their output is unchanged.
pub fn truncate_address(address: String) -> String {
   let chars = address.chars().count();
   if chars <= 12 {
      return address;
   }

   let head: String = address.chars().take(6).collect();
   let tail: String = address.chars().skip(chars - 6).collect();

   format!("{head}...{tail}")
}

/// The host part of a URL: what sits between `://` and the first `/` or `?`.
///
/// This is the part of an endpoint that a caller may echo. A provider key usually rides in the path or
/// the query, so a spoken sentence, an accessibility label or an answer to an agent names the host and
/// nothing else. `None` means the string is not shaped like a URL at all.
pub fn url_host(url: &str) -> Option<&str> {
   let (_, rest) = url.split_once("://")?;
   let host = rest.split(['/', '?']).next()?;

   if host.is_empty() { None } else { Some(host) }
}

/// Check that a string is a URL a client can be pointed at: a scheme, a host, and no whitespace.
///
/// A shape check, not a resolution: the client parses the URL again when it dials it, so an endpoint
/// that is down is a failure of that call rather than a reason to refuse the setting. `field` is the
/// patch field the sentence is about, so a refusal reads the same whichever screen asked.
pub fn check_url(field: &str, url: &str) -> Result<(), String> {
   let url = url.trim();

   if url.is_empty() {
      return Err(format!(
         "{field} cannot be empty: pass the URL to use, or leave the field out"
      ));
   }

   if !(url.starts_with("http://") || url.starts_with("https://")) {
      return Err(format!(
         "{field} must be an http:// or https:// URL"
      ));
   }

   if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
      return Err(format!("{field} cannot contain whitespace"));
   }

   if url_host(url).is_none() {
      return Err(format!("{field} names no host"));
   }

   Ok(())
}

pub fn truncate_hash(hash: String) -> String {
   if hash.len() > 38 {
      format!("{}...{}", &hash[..6], &hash[hash.len() - 4..])
   } else {
      hash
   }
}

#[track_caller]
pub fn malloc_trim() {
   #[cfg(target_os = "linux")]
   // SAFETY: `libc::malloc_trim` is glibc's allocator-reclaim entry point. A zero request only
   // asks it to release free heap pages; it dereferences no caller pointer and imposes no
   // aliasing or lifetime precondition on the caller. Its `int` result is compared and logged
   // below and never used as a pointer. glibc serialises the call against its own allocator
   // state, which is the same libc allocator Rust uses.
   unsafe {
      if libc::malloc_trim(0) == 1 {
         tracing::info!("Released free memory");
      } else {
         tracing::warn!("Failed to release free memory");
      }
   }
}
