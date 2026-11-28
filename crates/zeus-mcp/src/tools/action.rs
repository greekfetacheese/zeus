//! One tool call, one action: durable enough to replay, never a second send.
//!
//! An agent that loses its connection mid-send cannot know whether the transaction was broadcast, so it
//! retries — and a retry that sends again is the worst failure this surface can have. A caller therefore
//! names its action with `action_id`: the same id with the same arguments replays the first answer, the same
//! id with other arguments is refused, and no id runs the call and says it is not idempotent.
//!
//! The action runs on its own task. The HTTP request only reads the result, so a client that gives up does
//! not cancel a transaction in flight — and does not drop the confirmation the user is looking at either,
//! which would leave a modal nobody listens to.
//!
//! Only a completed action is kept. A refusal is dropped as soon as it is answered: nothing was sent, so a
//! retry may legitimately succeed — and after a broadcast whose receipt timed out the honest answer is
//! "check the history", never "it failed".

use super::{Call, ToolOutput, Warning};
use crate::error::ToolError;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::future::Future;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// How long a finished action can be replayed for.
const KEEP: Duration = Duration::from_secs(600);

/// What one action came to.
type Outcome = Result<ToolOutput, ToolError>;

/// One action this server has run, or is running.
struct Entry {
   /// What the request asked for, so the same id with another payload cannot be mistaken for a retry.
   fingerprint: String,
   started: Instant,
   outcome: watch::Receiver<Option<Outcome>>,
}

static ACTIONS: LazyLock<Mutex<HashMap<(String, String), Entry>>> = LazyLock::new(Default::default);

/// Run one action, once, and answer what it came to.
///
/// `body` is built by the caller with everything it needs moved into it: it outlives the request that
/// started it.
pub async fn run<F>(call: &Call, arguments: &Value, body: F) -> Result<ToolOutput, ToolError>
where
   F: Future<Output = Outcome> + Send + 'static,
{
   let Some(id) = arguments.get("action_id").and_then(Value::as_str) else {
      // Without an id there is nothing to recognise a retry by, and guessing one would be worse than
      // saying so: the call runs once, and the answer says a second call is a second action.
      let output = body.await?;

      return Ok(output.stamped(json!({"id": null, "replayed": false, "idempotent": false})));
   };

   let key = (call.profile.id.to_string(), id.to_owned());
   let fingerprint = fingerprint(arguments);

   let replay = {
      let mut actions = ACTIONS.lock().unwrap();
      actions.retain(|_, entry| entry.started.elapsed() < KEEP);

      match actions.get(&key) {
         Some(entry) if entry.fingerprint != fingerprint => {
            return Err(ToolError::validation(format!(
               "action_id {id} was already used for a different request"
            ))
            .with_hint(
               "Use a new id for a new action, or repeat the arguments of the one it was used for.",
            ));
         }
         Some(entry) => Some(entry.outcome.clone()),
         None => None,
      }
   };

   let replayed = replay.is_some();

   let outcome = match replay {
      Some(outcome) => wait(outcome).await?,
      None => {
         let (sender, receiver) = watch::channel(None);

         ACTIONS.lock().unwrap().insert(
            key.clone(),
            Entry {
               fingerprint,
               started: Instant::now(),
               outcome: receiver.clone(),
            },
         );

         // Detached on purpose: the request reads the answer, it does not drive the action.
         tokio::spawn(async move {
            let outcome = body.await;

            // A refusal is not an outcome to replay — see the module docs.
            if outcome.is_err() {
               ACTIONS.lock().unwrap().remove(&key);
            }

            let _ = sender.send(Some(outcome));
         });

         wait(receiver).await?
      }
   };

   outcome.map(|output| output.stamped(json!({"id": id, "replayed": replayed, "idempotent": true})))
}

/// Stamp a preview's answer: the id it carried, and the fact that a preview never consumed it.
///
/// A dry run answers before run is reached, and it must not register its id: registering one would make
/// the real send that repeats it replay the preview and never send at all. The answer says so rather than
/// leaving the caller to notice that its id went nowhere.
pub fn preview(data: Value, arguments: &Value) -> ToolOutput {
   let id = arguments.get("action_id").and_then(Value::as_str).map(str::to_owned);

   let mut output = ToolOutput::new(data)
      .stamped(json!({"id": id, "replayed": false, "idempotent": false, "preview": true}));

   // A preview runs no flow, so it has no promise to read and decides on the envelope alone. Saying so is
   // the difference between "this will be allowed" and "the envelope allows it": a transfer tax or a bad
   // route is only visible when the promise and the simulation are compared, and that happens at broadcast.
   output.warnings.push(Warning {
      code: "PREVIEW_SKIPS_REVIEW",
      message: PREVIEW_SKIPS_REVIEW.to_owned(),
      effect: "an allowed preview is not a promise the broadcast will be allowed",
   });

   output
}

/// What a preview does not do: the review needs a promise, and only a flow builds one.
const PREVIEW_SKIPS_REVIEW: &str = "This preview decides on the envelope alone: the price-impact and shortfall review runs at broadcast.";

/// Wait for an action's answer, whether this call started it or is replaying one still in flight.
async fn wait(mut receiver: watch::Receiver<Option<Outcome>>) -> Result<Outcome, ToolError> {
   loop {
      if let Some(outcome) = receiver.borrow().clone() {
         return Ok(outcome);
      }

      if receiver.changed().await.is_err() {
         return Err(ToolError::internal(
            "the action ended before it answered anything",
         ));
      }
   }
}

/// The request's own bytes, minus the id, so two calls can be compared without trusting key order.
fn fingerprint(arguments: &Value) -> String {
   let mut arguments = arguments.clone();

   if let Value::Object(map) = &mut arguments {
      map.remove("action_id");
   }

   arguments.to_string()
}

#[cfg(test)]
mod tests {
   use super::*;

   /// A preview answers before the ring is reached, so its id is not consumed — and the answer says so
   /// rather than leaving the caller to find out when the real send runs as though it were new.
   #[test]
   fn a_preview_says_the_action_id_it_did_not_consume() {
      let output = preview(
         json!({"preview": true}),
         &json!({"action_id": "order-1"}),
      );

      assert_eq!(
         output.data["action"],
         json!({"id": "order-1", "replayed": false, "idempotent": false, "preview": true})
      );
   }

   #[test]
   fn a_preview_without_an_id_says_that_too() {
      let output = preview(json!({}), &json!({}));

      assert_eq!(output.data["action"]["id"], Value::Null);
      assert_eq!(output.data["action"]["idempotent"], false);
   }
}
