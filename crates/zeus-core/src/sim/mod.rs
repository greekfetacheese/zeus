pub mod approval_diff;
pub mod balance_diff;
pub mod diff;

mod prefetch;
mod run;

pub use prefetch::*;
pub use run::*;

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn railgun_slots() {
      let slots = railgun_smart_wallet_known_slots();
      println!("{}", slots.len());
   }

   /// The send path is spawned, so callers hold a `ForkSim` across awaits. If this
   /// ever stops compiling, the EVM has leaked back out of `simulate_on_fork_with`.
   #[test]
   fn fork_sim_is_send() {
      fn assert_send<T: Send>() {}
      assert_send::<ForkSim>();
   }
}
