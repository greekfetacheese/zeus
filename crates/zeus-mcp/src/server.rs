//! The listener: one loopback endpoint, one task, and a stop that actually stops.
//!
//! The transport is the stateless one: a single `POST` path, JSON in and JSON out, no session, no stream,
//! no server-initiated request. Everything the revision asks of a local listener is here — a loopback
//! bind, an `Origin` that must be absent, a `Host` that must be loopback, a body ceiling, and a credential
//! checked on every request. Binding to loopback is not an authentication boundary, so there is no path
//! through this file that reaches a tool without a profile.
//!
//! The stop path is real, which the app's own connector does not have: the switch takes the listener down
//! and removes the rendezvous file, and a request that arrives on a connection the client already had is
//! answered "the agent server was stopped" rather than served.

use crate::auth::{AuthError, Caller, authenticate};
use crate::error::ProtocolError;
use crate::limit::RateLimiter;
use crate::now_secs;
use crate::ops::AgentOps;
use crate::protocol::era::{self, Headers};
use crate::protocol::jsonrpc::{self, Incoming};
use crate::protocol::types;
use crate::resources;
use crate::tools::{self, Call};
use serde::Serialize;
use serde_json::{Value, json};
use std::convert::Infallible;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::process;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use tokio::task::JoinHandle;
use warp::http::StatusCode;
use warp::http::header::{HeaderValue, WWW_AUTHENTICATE};
use warp::reply::Response;
use warp::{Filter, Rejection, Reply};
use zeus_core::ZeusCtx;
use zeus_core::persisted::{PersistedFile, ensure_data_dir, file_path};
use zeus_core::write_private_atomic;

/// The port the server asks for first.
///
/// Chosen outside the dapp connector's band, which owns 65534 down to 65503 and walks it: a second
/// listener inside that range would make the connector's own walk ambiguous.
pub const PREFERRED_PORT: u16 = 65502;

/// How far down the walk goes before giving up.
const PORT_ATTEMPTS: u16 = 8;

/// The largest request body the endpoint accepts.
const MAX_BODY_BYTES: u64 = 1024 * 1024;

/// What a 401 says it wants.
const CHALLENGE: &str = "Bearer realm=\"zeus\", error=\"invalid_token\"";

/// What the GUI shows and what a test waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerStatus {
   Stopped,
   Running { port: u16 },
}

/// Everything one listener needs to exist.
pub struct ServerConfig {
   pub ctx: ZeusCtx,
   pub ops: Arc<dyn AgentOps>,
   /// The process runtime the listener and its blocking work are spawned on.
   pub rt: tokio::runtime::Handle,
   /// Where the port walk starts. Zero asks the OS for any free port, which is what a test wants.
   pub preferred_port: u16,
}

impl ServerConfig {
   pub fn new(ctx: ZeusCtx, ops: Arc<dyn AgentOps>, rt: tokio::runtime::Handle) -> Self {
      Self {
         ctx,
         ops,
         rt,
         preferred_port: PREFERRED_PORT,
      }
   }
}

/// A running listener, owned by whoever started it.
///
/// Dropping the handle stops the server, so the GUI can hold one in a field and let the value's lifetime
/// be the switch.
pub struct ServerHandle {
   port: u16,
   stopping: Arc<AtomicBool>,
   task: Mutex<Option<JoinHandle<()>>>,
}

impl ServerHandle {
   /// Bind, advertise the port, and start serving.
   pub fn start(config: ServerConfig) -> Result<Self, anyhow::Error> {
      let listener = bind(config.preferred_port)?;
      let port = listener.local_addr()?.port();
      let started_at = now_secs();

      let card = SessionCard {
         port,
         pid: process::id(),
         version: config.ops.app_info().version,
         started_at,
      };

      if let Err(e) = write_card(&card) {
         // A missing rendezvous file costs a client nothing when the GUI shows the port anyway, so it is
         // not a reason to leave the owner without an agent surface.
         tracing::warn!("Could not write the agent server's session file: {e:?}");
      }

      let stopping = Arc::new(AtomicBool::new(false));
      let server = Arc::new(Server {
         ctx: config.ctx,
         ops: config.ops,
         port,
         started_at,
         rate: RateLimiter::standard(),
         stopping: stopping.clone(),
      });

      let route = routes(server);

      let listener = {
         // The listener has to register with the runtime's reactor, which is what entering the handle
         // does; the caller is the frame loop, not a worker of that runtime.
         let _entered = config.rt.enter();
         tokio::net::TcpListener::from_std(listener)?
      };

      let task = config.rt.spawn(async move {
         warp::serve(route).incoming(listener).run().await;
      });

      tracing::info!("Zeus agent server listening on 127.0.0.1:{port}");

      Ok(Self {
         port,
         stopping,
         task: Mutex::new(Some(task)),
      })
   }

   /// The port the listener actually holds.
   pub fn port(&self) -> u16 {
      self.port
   }

   pub fn status(&self) -> ServerStatus {
      if self.stopping.load(Ordering::SeqCst) {
         ServerStatus::Stopped
      } else {
         ServerStatus::Running { port: self.port }
      }
   }

   /// Stop serving: no new connection is accepted, the file that advertises the port is gone, and a
   /// request that arrives on a connection the client already had is told why.
   pub fn stop(&self) {
      if self.stopping.swap(true, Ordering::SeqCst) {
         return;
      }

      let task = self.task.lock().map(|mut task| task.take()).ok().flatten();

      if let Some(task) = task {
         task.abort();
      }

      remove_card();

      tracing::info!(
         "Zeus agent server stopped (was on 127.0.0.1:{})",
         self.port
      );
   }
}

impl Drop for ServerHandle {
   fn drop(&mut self) {
      self.stop();
   }
}

/// The state one listener shares with its requests.
struct Server {
   ctx: ZeusCtx,
   ops: Arc<dyn AgentOps>,
   port: u16,
   started_at: u64,
   rate: RateLimiter,
   stopping: Arc<AtomicBool>,
}

impl Server {
   /// The checks that come before the protocol: is the server running, is this a browser, is the host
   /// the one we bound.
   fn admit(&self, origin: Option<&str>, host: Option<&str>) -> Result<(), Rejection> {
      if self.stopping.load(Ordering::SeqCst) {
         return Err(refuse(Refusal::Stopped));
      }

      // No browser has any business here: the endpoint has no session, no cookies and no CORS headers,
      // and every request it accepts carries a token no page can read. A request that states an origin
      // is a page, and the answer is no.
      if origin.is_some() {
         return Err(refuse(Refusal::Browser));
      }

      match host {
         Some(host) if loopback_host(host, self.port) => Ok(()),
         _ => Err(refuse(Refusal::Host)),
      }
   }

   /// Authenticate one request, spend its rate budget and remember that the profile was used.
   fn welcome(&self, header: Option<&str>) -> Result<Caller, Rejection> {
      let now = now_secs();

      let caller = authenticate(&self.ctx.agent_profiles(), header, now)
         .map_err(|error| refuse(Refusal::Anonymous(error)))?;

      self
         .rate
         .check(&caller.profile.id, Instant::now())
         .map_err(|retry| refuse(Refusal::TooFast(retry.as_millis() as u64)))?;

      self.touch(&caller, now);

      Ok(caller)
   }

   /// Record the use in memory, and only in memory.
   ///
   /// The stamp feeds a column on the agents page. Writing the sealed profile store behind a request
   /// would let a request flood become a disk flood, and a flood is exactly what a rate-limited
   /// endpoint should expect; the store is written where it is written for everything else — when the
   /// owner changes it.
   fn touch(&self, caller: &Caller, now: u64) {
      self.ctx.agent_profiles().mark_used(&caller.profile.id, now);
   }

   /// Answer one message, from authentication to the body on the wire.
   async fn serve(&self, caller: Caller, headers: Headers, body: Value) -> Response {
      let incoming = match Incoming::parse(body) {
         Ok(incoming) => incoming,
         Err(error) => return error_reply(None, &error),
      };

      // A notification is owed no answer at all, and the only one either era sends is the handshake's
      // `initialized`. An unknown one is accepted too: a client that sent it believed it mattered, and
      // there is nothing useful to say about a message that expects no reply.
      let Some(id) = incoming.id.clone() else {
         return empty_reply(StatusCode::ACCEPTED);
      };

      let era = match era::detect(&incoming.method, incoming.params.as_ref()) {
         Ok(era) => era,
         Err(error) => return error_reply(Some(&id), &error),
      };

      if let Err(error) = era::check_headers(
         &era,
         &headers,
         &incoming.method,
         object_name(&incoming).as_deref(),
      ) {
         return error_reply(Some(&id), &error);
      }

      let modern = era.is_modern();

      let call = Call {
         ctx: self.ctx.clone(),
         profile: caller.profile,
         ops: self.ops.clone(),
         era,
         port: self.port,
         started_at: self.started_at,
      };

      let outcome = match incoming.method.as_str() {
         "initialize" => Ok(types::initialize(
            self.ops.as_ref(),
            incoming
               .params
               .as_ref()
               .and_then(|params| params.get("protocolVersion"))
               .and_then(Value::as_str),
         )),
         "server/discover" => Ok(types::discover(self.ops.as_ref())),
         // The handshake era's keep-alive. The stateless revision removed it outright, so a modern
         // client asking for it is asking for a method this server does not have.
         "ping" if !modern => Ok(json!({})),
         "tools/list" => Ok(tools::list(&call.profile, modern)),
         "tools/call" => tools::call_request(&call, incoming.params.as_ref(), modern).await,
         "resources/list" => Ok(resources::list(&call, modern)),
         "resources/templates/list" => Ok(resources::templates(&call.profile, modern)),
         "resources/read" => read_resource(&call, incoming.params.as_ref(), modern),
         other => Err(ProtocolError::unknown_method(other)),
      };

      match outcome {
         Ok(result) => json_reply(StatusCode::OK, &jsonrpc::result(id, result)),
         Err(error) => error_reply(Some(&id), &error),
      }
   }
}

/// A request the transport answered before the protocol saw it.
#[derive(Debug)]
enum Refusal {
   /// The switch was turned off between the connection and the request.
   Stopped,
   /// A request that carries an `Origin`: a browser.
   Browser,
   /// A `Host` that is not this loopback endpoint.
   Host,
   /// No credential, or one that does not resolve.
   Anonymous(AuthError),
   /// The profile's budget is spent; the value is the wait in milliseconds.
   TooFast(u64),
}

impl warp::reject::Reject for Refusal {}

fn refuse(refusal: Refusal) -> Rejection {
   warp::reject::custom(refusal)
}

fn routes(server: Arc<Server>) -> impl Filter<Extract = (Response,), Error = Infallible> + Clone {
   let mcp = warp::path("mcp")
      .and(warp::path::end())
      .and(warp::post())
      .and(with_admission(server.clone()))
      .and(with_caller(server.clone()))
      .and(warp::header::optional::<String>(
         "mcp-protocol-version",
      ))
      .and(warp::header::optional::<String>("mcp-method"))
      .and(warp::header::optional::<String>("mcp-name"))
      .and(warp::body::content_length_limit(MAX_BODY_BYTES))
      .and(warp::body::json::<Value>())
      .and(with_server(server))
      .and_then(handle);

   mcp.with(warp::trace::request()).recover(handle_rejection).map(normalise)
}

/// Collapse the two shapes a reply can take in this module — what the routes produced, and what
/// recovery produced for a refusal — into the one type the caller hands to `serve`.
fn normalise<T: Reply>(reply: T) -> Response {
   reply.into_response()
}

fn with_admission(server: Arc<Server>) -> impl Filter<Extract = (), Error = Rejection> + Clone {
   warp::header::optional::<String>("origin")
      .and(warp::header::optional::<String>("host"))
      .and_then(
         move |origin: Option<String>, host: Option<String>| {
            let server = server.clone();
            async move { server.admit(origin.as_deref(), host.as_deref()) }
         },
      )
      .untuple_one()
}

fn with_caller(server: Arc<Server>) -> impl Filter<Extract = (Caller,), Error = Rejection> + Clone {
   warp::header::optional::<String>("authorization").and_then(move |header: Option<String>| {
      let server = server.clone();
      async move { server.welcome(header.as_deref()) }
   })
}

fn with_server(
   server: Arc<Server>,
) -> impl Filter<Extract = (Arc<Server>,), Error = Infallible> + Clone {
   warp::any().map(move || server.clone())
}

async fn handle(
   caller: Caller,
   protocol_version: Option<String>,
   method: Option<String>,
   name: Option<String>,
   body: Value,
   server: Arc<Server>,
) -> Result<Response, Rejection> {
   let headers = Headers {
      protocol_version,
      method,
      name,
   };

   Ok(server.serve(caller, headers, body).await)
}

/// The one resource a request can name, read by the URI in its params.
fn read_resource(
   call: &Call,
   params: Option<&Value>,
   modern: bool,
) -> Result<Value, ProtocolError> {
   let uri = params
      .and_then(|params| params.get("uri"))
      .and_then(Value::as_str)
      .ok_or_else(|| ProtocolError::invalid_params("resources/read needs a uri"))?;

   resources::read(call, uri, modern)
}

/// The object a request names, for the header the modern era repeats it in.
fn object_name(incoming: &Incoming) -> Option<String> {
   if !era::names_an_object(&incoming.method) {
      return None;
   }

   let field = if incoming.method == "resources/read" {
      "uri"
   } else {
      "name"
   };

   incoming.params.as_ref()?.get(field)?.as_str().map(str::to_owned)
}

async fn handle_rejection(error: Rejection) -> Result<Response, Infallible> {
   if let Some(refusal) = error.find::<Refusal>() {
      return Ok(refusal_reply(refusal));
   }

   // A refusal from this module's own filters is the most specific thing that can be said about a
   // request, so it wins over the generic method and path rejections warp collects beside it.
   let refused = error.find::<Refusal>().is_some();

   if !refused && error.find::<warp::reject::MethodNotAllowed>().is_some() {
      return Ok(method_not_allowed());
   }

   if !refused && error.is_not_found() {
      return Ok(no_such_endpoint());
   }

   if error.find::<warp::reject::PayloadTooLarge>().is_some() {
      let error = ProtocolError::new(
         ProtocolError::INVALID_REQUEST,
         format!("the request body is larger than {MAX_BODY_BYTES} bytes"),
      );

      return Ok(json_reply(
         StatusCode::PAYLOAD_TOO_LARGE,
         &jsonrpc::error(None, &error),
      ));
   }

   if error.find::<warp::reject::UnsupportedMediaType>().is_some() {
      let error = ProtocolError::new(
         ProtocolError::INVALID_REQUEST,
         "the request body must be application/json",
      );

      return Ok(json_reply(
         StatusCode::UNSUPPORTED_MEDIA_TYPE,
         &jsonrpc::error(None, &error),
      ));
   }

   if error.find::<warp::filters::body::BodyDeserializeError>().is_some() {
      let error = ProtocolError::new(
         ProtocolError::PARSE_ERROR,
         "the request body is not valid JSON",
      );

      return Ok(json_reply(
         StatusCode::BAD_REQUEST,
         &jsonrpc::error(None, &error),
      ));
   }

   tracing::warn!("Unhandled rejection on the agent endpoint: {error:?}");

   let error = ProtocolError::internal("the agent server could not handle this request");

   Ok(json_reply(
      StatusCode::INTERNAL_SERVER_ERROR,
      &jsonrpc::error(None, &error),
   ))
}

/// The answer a transport-level refusal gets.
fn refusal_reply(refusal: &Refusal) -> Response {
   match refusal {
      Refusal::Stopped => transport(
         StatusCode::SERVICE_UNAVAILABLE,
         "AGENT_SERVER_STOPPED",
         "The agent server was stopped in Zeus",
         "Start it again in Zeus, then retry.",
      ),
      Refusal::Browser => transport(
         StatusCode::FORBIDDEN,
         "BROWSER_REQUEST",
         "This endpoint does not serve browser requests",
         "Call it from the harness, not from a page.",
      ),
      Refusal::Host => transport(
         StatusCode::FORBIDDEN,
         "UNEXPECTED_HOST",
         "The Host header is not this loopback endpoint",
         "Call it as http://127.0.0.1:<port>/mcp.",
      ),
      Refusal::Anonymous(error) => {
         let mut reply = transport(
            StatusCode::UNAUTHORIZED,
            error.code(),
            error.message(),
            error.hint(),
         );

         reply.headers_mut().insert(
            WWW_AUTHENTICATE,
            HeaderValue::from_static(CHALLENGE),
         );

         reply
      }
      // The wait is in both the header HTTP defines for it and the body, because a harness that only
      // reads JSON would otherwise have no idea how long to wait.
      Refusal::TooFast(wait) => json_reply(
         StatusCode::TOO_MANY_REQUESTS,
         &json!({"error": {
            "code": "RATE_LIMITED",
            "message": "This credential has used its calls for the minute",
            "hint": "Wait the time in retry_after_ms, then retry.",
            "retry_after_ms": wait,
         }}),
      )
      .with_retry_after(*wait),
   }
}

/// The 405 a request with the wrong method gets, with the one method the endpoint answers.
fn method_not_allowed() -> Response {
   let mut reply = transport(
      StatusCode::METHOD_NOT_ALLOWED,
      "METHOD_NOT_ALLOWED",
      "This endpoint answers POST only",
      "Send the request as POST /mcp.",
   );

   reply.headers_mut().insert(
      warp::http::header::ALLOW,
      HeaderValue::from_static("POST"),
   );

   reply
}

/// The 404 an unknown path gets: a JSON-RPC body, so a client that reads the body learns what the
/// status said.
fn no_such_endpoint() -> Response {
   json_reply(
      StatusCode::NOT_FOUND,
      &jsonrpc::error(
         None,
         &ProtocolError::invalid_request("no such endpoint: POST /mcp is the only one"),
      ),
   )
}

/// A JSON body for a transport-level refusal, in the shape every other error uses.
fn transport(status: StatusCode, code: &str, message: &str, hint: &str) -> Response {
   json_reply(
      status,
      &json!({"error": {"code": code, "message": message, "hint": hint}}),
   )
}

/// The reply an error in the protocol itself gets: `400` for the cases the protocol names, `200`
/// otherwise, because a JSON-RPC error is a successful exchange.
fn error_reply(id: Option<&jsonrpc::Id>, error: &ProtocolError) -> Response {
   let status = if error.rejected {
      StatusCode::BAD_REQUEST
   } else {
      StatusCode::OK
   };

   json_reply(status, &jsonrpc::error(id, error))
}

fn json_reply(status: StatusCode, body: &Value) -> Response {
   warp::reply::with_status(warp::reply::json(body), status).into_response()
}

fn empty_reply(status: StatusCode) -> Response {
   warp::reply::with_status(warp::reply::reply(), status).into_response()
}

/// A retry hint on a 429, in the header HTTP already defines for it.
trait RetryAfter {
   fn with_retry_after(self, milliseconds: u64) -> Self;
}

impl RetryAfter for Response {
   fn with_retry_after(mut self, milliseconds: u64) -> Self {
      let seconds = milliseconds.div_ceil(1000).max(1);

      if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
         self.headers_mut().insert(warp::http::header::RETRY_AFTER, value);
      }

      self
   }
}

/// Whether a `Host` header names this loopback endpoint.
///
/// The transport asks for this: without it a name that resolves to loopback reaches the listener, and the
/// only thing standing between that name and the wallet would be the token.
fn loopback_host(host: &str, port: u16) -> bool {
   let (name, declared) = match host.rsplit_once(':') {
      Some((name, declared)) => (name, Some(declared)),
      None => (host, None),
   };

   let name = name.trim_start_matches('[').trim_end_matches(']');

   let named = name.eq_ignore_ascii_case("localhost") || name == "127.0.0.1" || name == "::1";
   let ported = declared.is_none_or(|declared| declared.parse::<u16>().ok() == Some(port));

   named && ported
}

/// Bind the listener, walking down from `preferred` while the port is taken.
fn bind(preferred: u16) -> Result<std::net::TcpListener, anyhow::Error> {
   let attempts = if preferred == 0 { 1 } else { PORT_ATTEMPTS };
   let mut taken = None;

   for step in 0..attempts {
      let port = preferred.saturating_sub(step);

      match std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
         Ok(listener) => {
            listener.set_nonblocking(true)?;
            return Ok(listener);
         }
         Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => taken = Some(e),
         Err(e) => {
            return Err(anyhow::anyhow!(
               "cannot listen on 127.0.0.1:{port}: {e}"
            ));
         }
      }
   }

   let lowest = preferred.saturating_sub(attempts.saturating_sub(1));

   Err(anyhow::anyhow!(
      "no free port on 127.0.0.1 between {lowest} and {preferred}: {}",
      taken.map(|e| e.to_string()).unwrap_or_default()
   ))
}

/// What the rendezvous file holds.
#[derive(Serialize)]
struct SessionCard {
   port: u16,
   pid: u32,
   version: String,
   started_at: u64,
}

fn card_path() -> Result<PathBuf, anyhow::Error> {
   file_path(PersistedFile::Mcp)
}

fn write_card(card: &SessionCard) -> Result<(), anyhow::Error> {
   ensure_data_dir()?;
   let bytes = serde_json::to_vec_pretty(card)?;
   write_private_atomic(&card_path()?, &bytes)
}

fn remove_card() {
   let Ok(path) = card_path() else {
      return;
   };

   if let Err(e) = std::fs::remove_file(&path)
      && e.kind() != std::io::ErrorKind::NotFound
   {
      tracing::warn!("Could not remove {}: {e}", path.display());
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn a_loopback_host_is_accepted_with_or_without_the_port() {
      assert!(loopback_host("127.0.0.1:65502", 65502));
      assert!(loopback_host("127.0.0.1", 65502));
      assert!(loopback_host("localhost:65502", 65502));
      assert!(loopback_host("LOCALHOST:65502", 65502));
      assert!(loopback_host("[::1]:65502", 65502));
   }

   #[test]
   fn a_rebinding_host_or_the_wrong_port_is_refused() {
      assert!(!loopback_host("zeus.example:65502", 65502));
      assert!(!loopback_host("127.0.0.1:80", 65502));
      assert!(!loopback_host("", 65502));
   }
}
