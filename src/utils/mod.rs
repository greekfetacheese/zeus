pub mod misc;
pub mod self_update;
pub mod simulate;
pub mod state;
pub mod swap_quoter;
pub mod universal_router_v2;

pub use misc::*;

#[track_caller]
pub fn malloc_trim() {
   unsafe {
      if libc::malloc_trim(0) == 1 {
         tracing::info!("Released free memory");
      } else {
         tracing::warn!("Failed to release free memory");
      }
   }
}
