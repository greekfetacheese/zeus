pub mod simulate_position;
pub mod swap_quoter;
pub mod zeus_delegate;



/// UNIX time in X days from now
pub fn get_unix_time_from_days(days: u64) -> Result<u64, anyhow::Error> {
   let now = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)?
      .as_secs();

   Ok(now + 86400 * days)
}

/// UNIX time in X minutes from now
pub fn get_unix_time_from_minutes(minutes: u64) -> Result<u64, anyhow::Error> {
   let now = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)?
      .as_secs();

   Ok(now + 60 * minutes)
}