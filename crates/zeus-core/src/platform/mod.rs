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
