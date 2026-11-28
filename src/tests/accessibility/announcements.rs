//! What the app says, and when: the queue an agent's sentences wait in.
//!
//! A reader hears one sentence at a time, so the app has to say one at a time. A live region whose
//! text changes twice in a frame is one interruption, and the second sentence is the one lost — which
//! is exactly the shape two agent actions finishing together have.

use crate::gui::Announcements;

/// One pass of the app's channel: the sentence a frame would say, and the moment it said it.
fn say(queue: &mut Announcements, now: f64) -> Option<String> {
   let said = queue.next(now)?;

   queue.spoken(&said, now);

   Some(said)
}

/// Two actions finishing between two frames are two sentences, not one.
#[test]
fn two_sentences_between_two_frames_are_both_said() {
   let mut queue = Announcements::default();

   assert!(queue.queue("Agent sent 12 USDC"));
   assert!(queue.queue("Agent was denied: that token is not in your allowlist"));

   assert_eq!(
      say(&mut queue, 0.0).as_deref(),
      Some("Agent sent 12 USDC")
   );

   assert_eq!(
      say(&mut queue, 0.0),
      None,
      "the second must wait for the first to be heard"
   );

   assert!(
      queue.waiting(0.0).is_some(),
      "and the window must ask for the frame that says it"
   );

   assert_eq!(
      say(&mut queue, 2.0).as_deref(),
      Some("Agent was denied: that token is not in your allowlist")
   );
}

/// A harness retrying the same call is one thing to hear, not a burst of the same one.
#[test]
fn the_sentence_already_waiting_is_not_queued_twice() {
   let mut queue = Announcements::default();

   assert!(queue.queue("Agent was denied: no"));
   assert!(!queue.queue("Agent was denied: no"));

   assert_eq!(
      say(&mut queue, 0.0).as_deref(),
      Some("Agent was denied: no")
   );

   assert_eq!(
      say(&mut queue, 10.0),
      None,
      "nothing else was queued"
   );
}

/// The same words after they were said are a second announcement: two identical sends are two.
#[test]
fn the_same_sentence_after_it_was_said_is_said_again() {
   let mut queue = Announcements::default();

   queue.queue("Agent sent 1 USDC");
   assert_eq!(
      say(&mut queue, 0.0).as_deref(),
      Some("Agent sent 1 USDC")
   );

   queue.queue("Agent sent 1 USDC");
   assert_eq!(
      say(&mut queue, 10.0).as_deref(),
      Some("Agent sent 1 USDC"),
      "the queue's own gap must not swallow a second action"
   );
}

/// The queue is bounded: a run of attempts keeps the newest sentences, not the oldest.
#[test]
fn a_burst_keeps_the_newest_sentences() {
   let mut queue = Announcements::default();

   for n in 0..40 {
      queue.queue(format!("sentence {n}"));
   }

   let mut drained = Vec::new();

   for step in 0..40 {
      if let Some(said) = say(&mut queue, step as f64 * 10.0) {
         drained.push(said);
      }
   }

   assert_eq!(
      drained.len(),
      32,
      "the queue holds thirty-two sentences"
   );
   assert_eq!(
      drained[0], "sentence 8",
      "the oldest were dropped"
   );
   assert_eq!(drained[31], "sentence 39");
}

/// A sentence a page raised holds the queue back too: the two never talk over each other, whichever
/// of them was raised first.
#[test]
fn a_page_s_sentence_holds_the_queue_back() {
   let mut queue = Announcements::default();

   queue.queue("Agent sent 12 USDC");
   queue.spoken("Transaction confirmed", 5.0);

   assert_eq!(queue.next(5.0), None);

   let until = queue.waiting(5.0).expect("the queue says when it may speak");

   assert!(
      (until - 5.8).abs() < 1e-9,
      "a two-word sentence is given the floor of 0.8 seconds, not its own third: {until}"
   );

   assert_eq!(
      say(&mut queue, 7.0).as_deref(),
      Some("Agent sent 12 USDC")
   );
}
