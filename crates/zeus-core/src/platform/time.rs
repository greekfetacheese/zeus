use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeStamp {
   Seconds(u64),
   Millis(u64),
}

impl Default for TimeStamp {
   fn default() -> Self {
      TimeStamp::Seconds(0)
   }
}

impl TimeStamp {
   pub fn now_as_secs() -> Result<Self, anyhow::Error> {
      let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
      Ok(TimeStamp::Seconds(now))
   }

   pub fn now_as_millis() -> Result<Self, anyhow::Error> {
      let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
      Ok(TimeStamp::Millis(now))
   }

   pub fn saturating_add_secs(self, seconds: u64) -> Self {
      match self {
         TimeStamp::Seconds(s) => TimeStamp::Seconds(s.saturating_add(seconds)),
         TimeStamp::Millis(m) => TimeStamp::Millis(m.saturating_add(seconds * 1000)),
      }
   }

   pub fn saturating_sub_secs(self, seconds: u64) -> Self {
      match self {
         TimeStamp::Seconds(s) => TimeStamp::Seconds(s.saturating_sub(seconds)),
         TimeStamp::Millis(m) => TimeStamp::Millis(m.saturating_sub(seconds * 1000)),
      }
   }

   pub fn saturating_sub_millis(self, millis: u64) -> Self {
      match self {
         TimeStamp::Seconds(s) => TimeStamp::Seconds(s.saturating_sub(millis / 1000)),
         TimeStamp::Millis(m) => TimeStamp::Millis(m.saturating_sub(millis)),
      }
   }

   pub fn timestamp(&self) -> u64 {
      match self {
         TimeStamp::Seconds(seconds) => *seconds,
         TimeStamp::Millis(millis) => *millis,
      }
   }

   /// Exact instant expressed in milliseconds. `u128` because scaling a `u64` second count by
   /// 1000 can exceed `u64`.
   fn as_millis(&self) -> u128 {
      match self {
         TimeStamp::Seconds(seconds) => u128::from(*seconds) * 1000,
         TimeStamp::Millis(millis) => u128::from(*millis),
      }
   }

   /// Tie-breaker for instants that normalize to the same millisecond while living in different
   /// variants. The derived `PartialEq` compares variants, so ordering has to as well.
   fn rank(&self) -> u8 {
      match self {
         TimeStamp::Seconds(_) => 0,
         TimeStamp::Millis(_) => 1,
      }
   }

   pub fn to_relative(&self) -> String {
      timestamp_to_relative_time(self)
   }

   pub fn to_date_string(&self) -> String {
      let dt_opt = match self {
         TimeStamp::Seconds(seconds) => DateTime::<Utc>::from_timestamp_secs(*seconds as i64),
         TimeStamp::Millis(millis) => DateTime::<Utc>::from_timestamp_millis(*millis as i64),
      };

      if let Some(dt) = dt_opt {
         dt.format("%Y-%m-%d %H:%M:%S %Z").to_string()
      } else {
         format!("Invalid timestamp: {}", self.timestamp())
      }
   }

   pub fn to_date(&self) -> DateTime<Utc> {
      let opt = match self {
         TimeStamp::Seconds(seconds) => DateTime::<Utc>::from_timestamp_secs(*seconds as i64),
         TimeStamp::Millis(millis) => DateTime::<Utc>::from_timestamp_millis(*millis as i64),
      };

      match opt {
         Some(dt) => dt,
         None => DateTime::<Utc>::default(),
      }
   }
}

impl PartialOrd for TimeStamp {
   fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
      Some(self.cmp(other))
   }
}

impl Ord for TimeStamp {
   fn cmp(&self, other: &Self) -> Ordering {
      (self.as_millis(), self.rank()).cmp(&(other.as_millis(), other.rank()))
   }
}

/// Convert a timestamp to relative time
///
/// Eg. X time ago, or in X time
fn timestamp_to_relative_time(timestamp: &TimeStamp) -> String {
   let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();

   let (now, timestamp) = match timestamp {
      TimeStamp::Seconds(seconds) => (now.as_secs(), *seconds),
      TimeStamp::Millis(millis) => (now.as_millis() as u64, *millis),
   };

   let elapsed_opt = if now > timestamp {
      Some(now - timestamp)
   } else {
      None
   };

   let future_time_opt = if timestamp > now {
      Some(timestamp - now)
   } else {
      None
   };

   if let Some(elapsed) = elapsed_opt {
      if elapsed < 60 {
         return format!("{} second(s) ago", elapsed);
      } else if elapsed < 3600 {
         return format!("{} minute(s) ago", elapsed / 60);
      } else if elapsed < 86400 {
         return format!("{} hour(s) ago", elapsed / 3600);
      } else if elapsed < 604800 {
         return format!("{} day(s) ago", elapsed / 86400);
      } else if elapsed < 2419200 {
         return format!("{} week(s) ago", elapsed / 604800);
      } else if elapsed < 29030400 {
         return format!("{} month(s) ago", elapsed / 2419200);
      } else {
         return format!("{} year(s) ago", elapsed / 31536000);
      }
   }

   if let Some(future_time) = future_time_opt {
      if future_time < 60 {
         return format!("in {} second(s)", future_time);
      } else if future_time < 3600 {
         return format!("in {} minute(s)", future_time / 60);
      } else if future_time < 86400 {
         return format!("in {} hour(s)", future_time / 3600);
      } else if future_time < 604800 {
         return format!("in {} day(s)", future_time / 86400);
      } else if future_time < 2419200 {
         return format!("in {} week(s)", future_time / 604800);
      } else if future_time < 29030400 {
         return format!("in {} month(s)", future_time / 2419200);
      } else {
         return format!("in {} year(s)", future_time / 31536000);
      }
   }

   format!("Invalid timestamp")
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn a_mixed_variant_pair_is_not_equal() {
      let secs = TimeStamp::Seconds(1);
      let millis = TimeStamp::Millis(1_500);
      assert_ne!(secs.cmp(&millis), std::cmp::Ordering::Equal);
   }

   #[test]
   fn ordering_agrees_with_equality_for_mixed_units() {
      assert_eq!(
         TimeStamp::Seconds(1).cmp(&TimeStamp::Millis(1000)),
         std::cmp::Ordering::Less
      );
      assert_ne!(TimeStamp::Seconds(1), TimeStamp::Millis(1000));
      assert_eq!(
         TimeStamp::Millis(1000).cmp(&TimeStamp::Seconds(1)),
         std::cmp::Ordering::Greater
      );
      assert_eq!(
         TimeStamp::Seconds(1).cmp(&TimeStamp::Millis(1500)),
         std::cmp::Ordering::Less
      );
   }
}
