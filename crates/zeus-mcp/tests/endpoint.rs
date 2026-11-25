//! The endpoint from the outside: real HTTP over loopback, one request at a time.
//!
//! Every test drives the same public surface a harness does — bind, advertise, authenticate, answer —
//! with a hand-rolled client rather than a client library, so the assertions are about the bytes on the
//! wire: the status code, the headers, and the JSON body. Nothing here reaches into the crate's
//! internals, which is the point: this is the contract a harness sees.

use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::OnceLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zeus_core::agent::{AgentProfile, AgentProfileId, AgentProfileStore, ScreenGrant, WalletGrant};
use zeus_core::{TimeStamp, TransactionRich, WalletInfo, ZeusCtx};
use zeus_eth::alloy_primitives::{Address, TxHash};
use zeus_eth::utils::NumericValue;
use zeus_mcp::protocol::{LEGACY_VERSION, MODERN_VERSION, meta};
use zeus_mcp::{NoOps, ServerConfig, ServerHandle, ServerStatus};

/// The one working directory this test binary uses.
///
/// The endpoint writes its rendezvous file into `data/` beside the process's working directory, as the
/// app does, so the suite moves into a temporary one once. Every test calls this before it touches the
/// filesystem, which is what makes the single move safe even though the tests share a process.
fn workspace() -> &'static PathBuf {
   static DIR: OnceLock<PathBuf> = OnceLock::new();

   DIR.get_or_init(|| {
      let dir = std::env::temp_dir().join(format!("zeus-mcp-tests-{}", std::process::id()));
      std::fs::create_dir_all(&dir).expect("a temporary working directory");
      std::env::set_current_dir(&dir).expect("move into the temporary working directory");
      dir
   })
}

/// One test's running endpoint and the credential that reaches it.
struct Fixture {
   ctx: ZeusCtx,
   server: ServerHandle,
   port: u16,
   token: String,
   profile: AgentProfileId,
}

impl Fixture {
   /// A server on an ephemeral port, with one credential that may or may not read the wallet list.
   fn start(read_granted: bool) -> Self {
      if read_granted {
         Self::with(&[], &["settings.wallets.read"])
      } else {
         Self::with(&[], &[])
      }
   }

   /// A server whose one profile holds exactly `caps` on its wallet and `screens` on the app.
   fn with(caps: &[&str], screens: &[&str]) -> Self {
      workspace();

      let ctx = ZeusCtx::new(tokio::runtime::Handle::current());

      // The policy reads exactly these two flags for "the wallet is open", and an unlocked vault is what
      // a harness only ever meets in a running app.
      ctx.write(|ctx| ctx.vault_unlocked = true);

      let address = "0x1111111111111111111111111111111111111111".parse().expect("an address");
      let mut info = WalletInfo::default();
      info.address = address;

      ctx.write(|ctx| {
         ctx.wallet_info_cache.map.insert(address, info.clone());
         ctx.wallet_info_cache.ordered_vec.push(info);
      });

      let (token, hash) = zeus_core::agent::generate_token();
      let mut profile = AgentProfile::new("test harness", hash, 0);

      for screen in screens {
         profile.screens.insert(
            (*screen).to_owned(),
            ScreenGrant {
               read: true,
               write: false,
            },
         );
      }

      let mut grant = WalletGrant {
         enabled: true,
         ..Default::default()
      };

      for capability in caps {
         grant.caps.insert((*capability).to_owned());
      }

      profile.wallets.insert(address, grant);

      let id = profile.id.clone();
      ctx.agent_profiles().insert(profile);

      let mut config = ServerConfig::new(
         ctx.clone(),
         std::sync::Arc::new(NoOps),
         tokio::runtime::Handle::current(),
      );
      config.preferred_port = 0;

      let server = ServerHandle::start(config).expect("the endpoint binds");
      let port = server.port();

      Self {
         ctx,
         server,
         port,
         token: token.as_str().to_owned(),
         profile: id,
      }
   }

   fn store(&self) -> AgentProfileStore {
      self.ctx.agent_profiles()
   }
}

impl Drop for Fixture {
   fn drop(&mut self) {
      self.server.stop();
   }
}

/// One answer, as the wire carried it.
struct Reply {
   status: u16,
   headers: Vec<(String, String)>,
   body: Value,
}

impl Reply {
   fn header(&self, name: &str) -> Option<&str> {
      self
         .headers
         .iter()
         .find(|(have, _)| have == name)
         .map(|(_, value)| value.as_str())
   }

   fn error_code(&self) -> Option<i64> {
      self.body.pointer("/error/code").and_then(Value::as_i64)
   }
}

/// One request, written by hand so the headers under test are the ones on the wire.
async fn request(
   port: u16,
   method: &str,
   path: &str,
   headers: &[(&str, &str)],
   body: &str,
   declared_length: Option<u64>,
) -> Reply {
   let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
      .await
      .expect("the endpoint accepts a connection");

   let length = declared_length.unwrap_or(body.len() as u64);

   let custom_host = headers.iter().any(|(name, _)| name.eq_ignore_ascii_case("host"));

   let mut head = format!(
      "{method} {path} HTTP/1.1\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {length}\r\n"
   );

   if !custom_host {
      head.push_str(&format!("Host: 127.0.0.1:{port}\r\n"));
   }

   for (name, value) in headers {
      head.push_str(&format!("{name}: {value}\r\n"));
   }

   head.push_str("\r\n");

   let _ = stream.write_all(head.as_bytes()).await;
   let _ = stream.write_all(body.as_bytes()).await;

   let mut raw = Vec::new();
   let _ = stream.read_to_end(&mut raw).await;

   parse(&raw)
}

fn parse(raw: &[u8]) -> Reply {
   let text = String::from_utf8_lossy(raw);
   let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));

   let mut lines = head.split("\r\n");
   let status = lines
      .next()
      .and_then(|line| line.split_whitespace().nth(1))
      .and_then(|code| code.parse().ok())
      .unwrap_or(0);

   let headers = lines
      .filter_map(|line| line.split_once(':'))
      .map(|(name, value)| {
         (
            name.trim().to_ascii_lowercase(),
            value.trim().to_owned(),
         )
      })
      .collect();

   Reply {
      status,
      headers,
      body: serde_json::from_str(body).unwrap_or(Value::Null),
   }
}

/// A JSON-RPC request body.
fn call(id: u64, method: &str, params: Value) -> String {
   json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string()
}

/// The `_meta` the stateless revision requires on every request.
fn modern_meta() -> Value {
   json!({
      meta::PROTOCOL_VERSION: MODERN_VERSION,
      meta::CLIENT_CAPABILITIES: {},
      meta::CLIENT_INFO: {"name": "tests", "version": "0.1"},
   })
}

#[tokio::test]
async fn a_request_without_a_credential_is_unauthorized() {
   let fixture = Fixture::start(true);

   let reply = request(
      fixture.port,
      "POST",
      "/mcp",
      &[("Authorization", "")],
      &call(1, "tools/list", json!({})),
      None,
   )
   .await;

   assert_eq!(reply.status, 401);
   assert!(reply.header("www-authenticate").is_some_and(|value| value.contains("Bearer")));
   assert_eq!(
      reply.body.pointer("/error/code").and_then(Value::as_str),
      Some("AUTH_MISSING")
   );
}

#[tokio::test]
async fn a_wrong_token_is_unauthorized() {
   let fixture = Fixture::start(true);

   let reply = request(
      fixture.port,
      "POST",
      "/mcp",
      &[("Authorization", "Bearer zk_0000")],
      &call(1, "tools/list", json!({})),
      None,
   )
   .await;

   assert_eq!(reply.status, 401);
   assert_eq!(
      reply.body.pointer("/error/code").and_then(Value::as_str),
      Some("AUTH_UNKNOWN_TOKEN")
   );
}

#[tokio::test]
async fn a_browser_origin_is_forbidden() {
   let fixture = Fixture::start(true);

   let reply = request(
      fixture.port,
      "POST",
      "/mcp",
      &[
         (
            "Authorization",
            &format!("Bearer {}", fixture.token),
         ),
         ("Origin", "https://evil.example"),
      ],
      &call(1, "tools/list", json!({})),
      None,
   )
   .await;

   assert_eq!(reply.status, 403);
   assert_eq!(
      reply.body.pointer("/error/code").and_then(Value::as_str),
      Some("BROWSER_REQUEST")
   );
}

#[tokio::test]
async fn a_rebound_host_is_forbidden() {
   let fixture = Fixture::start(true);

   let reply = request(
      fixture.port,
      "POST",
      "/mcp",
      &[
         (
            "Authorization",
            &format!("Bearer {}", fixture.token),
         ),
         ("Host", "zeus.example:65502"),
      ],
      &call(1, "tools/list", json!({})),
      None,
   )
   .await;

   assert_eq!(reply.status, 403);
   assert_eq!(
      reply.body.pointer("/error/code").and_then(Value::as_str),
      Some("UNEXPECTED_HOST")
   );
}

#[tokio::test]
async fn a_body_over_the_ceiling_is_refused_before_it_is_read() {
   let fixture = Fixture::start(true);

   let reply = request(
      fixture.port,
      "POST",
      "/mcp",
      &[(
         "Authorization",
         &format!("Bearer {}", fixture.token),
      )],
      "{}",
      Some(2 * 1024 * 1024),
   )
   .await;

   assert_eq!(reply.status, 413);
   assert_eq!(reply.error_code(), Some(-32600));
}

#[tokio::test]
async fn an_unknown_path_is_a_json_rpc_404() {
   let fixture = Fixture::start(true);

   let reply = request(
      fixture.port,
      "POST",
      "/api",
      &[(
         "Authorization",
         &format!("Bearer {}", fixture.token),
      )],
      &call(1, "tools/list", json!({})),
      None,
   )
   .await;

   assert_eq!(reply.status, 404);
   assert_eq!(reply.error_code(), Some(-32600));
   assert_eq!(
      reply.body.pointer("/id").and_then(Value::as_i64),
      None
   );
}

#[tokio::test]
async fn a_get_on_the_endpoint_is_method_not_allowed() {
   let fixture = Fixture::start(true);

   let reply = request(fixture.port, "GET", "/mcp", &[], "", None).await;

   assert_eq!(reply.status, 405);
   assert_eq!(reply.header("allow"), Some("POST"));
}

#[tokio::test]
async fn the_legacy_handshake_then_a_tool_call_reads_the_wallets() {
   let fixture = Fixture::start(true);
   let auth = format!("Bearer {}", fixture.token);

   let initialize = request(
      fixture.port,
      "POST",
      "/mcp",
      &[("Authorization", &auth)],
      &call(1, "initialize", json!({"protocolVersion": LEGACY_VERSION, "capabilities": {}, "clientInfo": {"name": "tests"}})),
      None,
   )
   .await;

   assert_eq!(initialize.status, 200);
   assert_eq!(
      initialize.body.pointer("/result/protocolVersion").and_then(Value::as_str),
      Some(LEGACY_VERSION)
   );
   // A handshake-era client is not sent the fields its revision never had.
   assert!(initialize.body.pointer("/result/resultType").is_none());

   let list = request(
      fixture.port,
      "POST",
      "/mcp",
      &[("Authorization", &auth)],
      &call(2, "tools/list", json!({})),
      None,
   )
   .await;

   let tools = list
      .body
      .pointer("/result/tools")
      .and_then(Value::as_array)
      .expect("a tool list");
   let names: Vec<&str> = tools.iter().filter_map(|tool| tool["name"].as_str()).collect();
   assert_eq!(
      names,
      [
         "delegate_status",
         "settings_get",
         "settings_list",
         "wallet_get",
         "wallet_list",
         "zeus_help",
         "zeus_status"
      ]
   );

   let status = request(
      fixture.port,
      "POST",
      "/mcp",
      &[("Authorization", &auth)],
      &call(
         3,
         "tools/call",
         json!({"name": "zeus_status", "arguments": {}}),
      ),
      None,
   )
   .await;

   assert_eq!(status.status, 200);
   assert_eq!(
      status.body.pointer("/result/isError").and_then(Value::as_bool),
      None
   );
   assert_eq!(
      status
         .body
         .pointer("/result/structuredContent/data/server/era")
         .and_then(Value::as_str),
      Some("legacy")
   );
   assert_eq!(
      status
         .body
         .pointer("/result/structuredContent/data/vault/unlocked")
         .and_then(Value::as_bool),
      Some(true)
   );

   let wallets = request(
      fixture.port,
      "POST",
      "/mcp",
      &[("Authorization", &auth)],
      &call(
         4,
         "tools/call",
         json!({"name": "wallet_list", "arguments": {}}),
      ),
      None,
   )
   .await;

   let rows = wallets
      .body
      .pointer("/result/structuredContent/data/wallets")
      .and_then(Value::as_array)
      .expect("a wallet list");

   assert_eq!(rows.len(), 1);
   assert_eq!(
      rows[0].get("address").and_then(Value::as_str),
      Some("0x1111111111111111111111111111111111111111")
   );
}

#[tokio::test]
async fn the_modern_era_discovers_and_calls_with_per_request_metadata() {
   let fixture = Fixture::start(true);
   let auth = format!("Bearer {}", fixture.token);

   let discover = request(
      fixture.port,
      "POST",
      "/mcp",
      &[
         ("Authorization", &auth),
         ("MCP-Protocol-Version", MODERN_VERSION),
         ("Mcp-Method", "server/discover"),
      ],
      &call(
         1,
         "server/discover",
         json!({"_meta": modern_meta()}),
      ),
      None,
   )
   .await;

   assert_eq!(discover.status, 200);
   assert_eq!(
      discover.body.pointer("/result/supportedVersions/0").and_then(Value::as_str),
      Some(MODERN_VERSION)
   );
   assert_eq!(
      discover.body.pointer("/result/resultType").and_then(Value::as_str),
      Some("complete")
   );
   assert!(
      discover
         .body
         .pointer("/result/_meta")
         .and_then(|meta| meta.get("io.modelcontextprotocol/serverInfo"))
         .is_some()
   );

   let status = request(
      fixture.port,
      "POST",
      "/mcp",
      &[
         ("Authorization", &auth),
         ("MCP-Protocol-Version", MODERN_VERSION),
         ("Mcp-Method", "tools/call"),
         ("Mcp-Name", "zeus_status"),
      ],
      &call(
         2,
         "tools/call",
         json!({"name": "zeus_status", "arguments": {}, "_meta": modern_meta()}),
      ),
      None,
   )
   .await;

   assert_eq!(status.status, 200);
   assert_eq!(
      status
         .body
         .pointer("/result/structuredContent/data/server/era")
         .and_then(Value::as_str),
      Some("modern")
   );
   assert_eq!(
      status.body.pointer("/result/resultType").and_then(Value::as_str),
      Some("complete")
   );
}

#[tokio::test]
async fn an_unknown_revision_is_a_400_that_names_what_is_supported() {
   let fixture = Fixture::start(true);

   let reply = request(
      fixture.port,
      "POST",
      "/mcp",
      &[
         (
            "Authorization",
            &format!("Bearer {}", fixture.token),
         ),
         ("MCP-Protocol-Version", "2020-01-01"),
         ("Mcp-Method", "tools/list"),
      ],
      &call(
         1,
         "tools/list",
         json!({"_meta": {meta::PROTOCOL_VERSION: "2020-01-01", meta::CLIENT_CAPABILITIES: {}}}),
      ),
      None,
   )
   .await;

   assert_eq!(reply.status, 400);
   assert_eq!(reply.error_code(), Some(-32022));
   assert_eq!(
      reply.body.pointer("/error/data/requested").and_then(Value::as_str),
      Some("2020-01-01")
   );
}

#[tokio::test]
async fn a_header_that_disagrees_with_the_body_is_a_400() {
   let fixture = Fixture::start(true);

   let reply = request(
      fixture.port,
      "POST",
      "/mcp",
      &[
         (
            "Authorization",
            &format!("Bearer {}", fixture.token),
         ),
         ("MCP-Protocol-Version", MODERN_VERSION),
         ("Mcp-Method", "tools/list"),
      ],
      &call(
         1,
         "tools/call",
         json!({"name": "zeus_status", "arguments": {}, "_meta": modern_meta()}),
      ),
      None,
   )
   .await;

   assert_eq!(reply.status, 400);
   assert_eq!(reply.error_code(), Some(-32020));
}

#[tokio::test]
async fn a_disabled_profile_is_refused_on_the_next_request() {
   let fixture = Fixture::start(true);
   let auth = format!("Bearer {}", fixture.token);

   let before = request(
      fixture.port,
      "POST",
      "/mcp",
      &[("Authorization", &auth)],
      &call(1, "tools/list", json!({})),
      None,
   )
   .await;

   assert_eq!(before.status, 200);

   assert!(fixture.store().set_enabled(&fixture.profile, false));

   let after = request(
      fixture.port,
      "POST",
      "/mcp",
      &[("Authorization", &auth)],
      &call(2, "tools/list", json!({})),
      None,
   )
   .await;

   assert_eq!(after.status, 401);
   assert_eq!(
      after.body.pointer("/error/code").and_then(Value::as_str),
      Some("AUTH_PROFILE_DISABLED")
   );
}

#[tokio::test]
async fn the_grant_decides_which_tools_exist_and_which_calls_run() {
   let fixture = Fixture::start(false);
   let auth = format!("Bearer {}", fixture.token);

   let list = request(
      fixture.port,
      "POST",
      "/mcp",
      &[("Authorization", &auth)],
      &call(1, "tools/list", json!({})),
      None,
   )
   .await;

   let tools = list
      .body
      .pointer("/result/tools")
      .and_then(Value::as_array)
      .expect("a tool list");
   let names: Vec<&str> = tools.iter().filter_map(|tool| tool["name"].as_str()).collect();
   assert_eq!(
      names,
      ["settings_get", "settings_list", "zeus_help", "zeus_status"]
   );

   let refused = request(
      fixture.port,
      "POST",
      "/mcp",
      &[("Authorization", &auth)],
      &call(
         2,
         "tools/call",
         json!({"name": "wallet_list", "arguments": {}}),
      ),
      None,
   )
   .await;

   assert_eq!(refused.status, 200);
   assert_eq!(
      refused.body.pointer("/result/isError").and_then(Value::as_bool),
      Some(true)
   );
   assert_eq!(
      refused
         .body
         .pointer("/result/structuredContent/error/code")
         .and_then(Value::as_str),
      Some("NOT_GRANTED")
   );
   assert!(
      refused
         .body
         .pointer("/result/structuredContent/error/hint")
         .and_then(Value::as_str)
         .is_some_and(|hint| hint.contains("Settings"))
   );

   // The refusal is the line a user needs to see, so it is in the ring.
   let entries = fixture.ctx.agent_audit().tail(10);
   let entry = entries.last().expect("a recorded refusal");

   assert_eq!(entry.tool, "wallet_list");
   assert_eq!(entry.capability, "settings.wallets.read");
   assert_eq!(entry.denial.as_deref(), Some("NOT_GRANTED"));
}

#[tokio::test]
async fn an_unknown_tool_is_a_protocol_error_rather_than_a_tool_error() {
   let fixture = Fixture::start(true);

   let reply = request(
      fixture.port,
      "POST",
      "/mcp",
      &[(
         "Authorization",
         &format!("Bearer {}", fixture.token),
      )],
      &call(
         1,
         "tools/call",
         json!({"name": "tx_send", "arguments": {}}),
      ),
      None,
   )
   .await;

   assert_eq!(reply.status, 200);
   assert!(reply.body.pointer("/result").is_none());
   assert_eq!(reply.error_code(), Some(-32602));
}

#[tokio::test]
async fn a_notification_is_accepted_without_an_answer() {
   let fixture = Fixture::start(true);

   let reply = request(
      fixture.port,
      "POST",
      "/mcp",
      &[(
         "Authorization",
         &format!("Bearer {}", fixture.token),
      )],
      &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}).to_string(),
      None,
   )
   .await;

   assert_eq!(reply.status, 202);
   assert_eq!(reply.body, Value::Null);
}

#[tokio::test]
async fn the_resources_follow_the_grant() {
   let fixture = Fixture::start(true);
   let auth = format!("Bearer {}", fixture.token);

   let list = request(
      fixture.port,
      "POST",
      "/mcp",
      &[("Authorization", &auth)],
      &call(1, "resources/list", json!({})),
      None,
   )
   .await;

   let uris: Vec<&str> = list
      .body
      .pointer("/result/resources")
      .and_then(Value::as_array)
      .expect("a resource list")
      .iter()
      .filter_map(|resource| resource["uri"].as_str())
      .collect();

   assert_eq!(
      uris,
      ["zeus://profile", "zeus://permissions", "zeus://wallets"]
   );

   let wallets = request(
      fixture.port,
      "POST",
      "/mcp",
      &[("Authorization", &auth)],
      &call(
         2,
         "resources/read",
         json!({"uri": "zeus://wallets"}),
      ),
      None,
   )
   .await;

   assert!(
      wallets
         .body
         .pointer("/result/contents/0/text")
         .and_then(Value::as_str)
         .is_some_and(|text| text.contains("0x1111111111111111111111111111111111111111"))
   );

   let unknown = request(
      fixture.port,
      "POST",
      "/mcp",
      &[("Authorization", &auth)],
      &call(
         3,
         "resources/read",
         json!({"uri": "zeus://secrets"}),
      ),
      None,
   )
   .await;

   assert_eq!(unknown.error_code(), Some(-32602));
}

#[tokio::test]
async fn a_legacy_ping_answers_and_a_modern_one_does_not() {
   let fixture = Fixture::start(true);
   let auth = format!("Bearer {}", fixture.token);

   let legacy = request(
      fixture.port,
      "POST",
      "/mcp",
      &[("Authorization", &auth)],
      &call(1, "ping", json!({})),
      None,
   )
   .await;

   assert_eq!(legacy.status, 200);
   assert_eq!(legacy.body.pointer("/result"), Some(&json!({})));

   let modern = request(
      fixture.port,
      "POST",
      "/mcp",
      &[
         ("Authorization", &auth),
         ("MCP-Protocol-Version", MODERN_VERSION),
         ("Mcp-Method", "ping"),
      ],
      &call(2, "ping", json!({"_meta": modern_meta()})),
      None,
   )
   .await;

   assert_eq!(modern.error_code(), Some(-32601));
}

#[tokio::test]
async fn a_stopped_server_refuses_on_a_connection_it_already_had() {
   let fixture = Fixture::start(true);
   let auth = format!("Bearer {}", fixture.token);

   // A harness that keeps its connection alive is the one case a stop cannot answer by closing the
   // listener, so it is the case worth pinning: the first request proves the connection works, the
   // second proves the switch reaches it.
   let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", fixture.port))
      .await
      .expect("a connection");

   let first = exchange(
      &mut stream,
      fixture.port,
      &auth,
      &call(1, "tools/list", json!({})),
   )
   .await;

   assert_eq!(first.status, 200);

   fixture.server.stop();

   let second = exchange(
      &mut stream,
      fixture.port,
      &auth,
      &call(2, "tools/list", json!({})),
   )
   .await;

   assert_eq!(second.status, 503);
   assert_eq!(
      second.body.pointer("/error/code").and_then(Value::as_str),
      Some("AGENT_SERVER_STOPPED")
   );
}

/// One request on a connection that stays open, and exactly one answer read back.
async fn exchange(
   stream: &mut tokio::net::TcpStream,
   port: u16,
   authorization: &str,
   body: &str,
) -> Reply {
   let head = format!(
      "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: {authorization}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
      body.len()
   );

   let _ = stream.write_all(head.as_bytes()).await;
   let _ = stream.write_all(body.as_bytes()).await;

   read_one(stream).await
}

async fn read_one(stream: &mut tokio::net::TcpStream) -> Reply {
   let mut buffer = Vec::new();
   let mut chunk = [0u8; 4096];

   loop {
      if let Some(end) = head_end(&buffer) {
         let head = String::from_utf8_lossy(&buffer[..end]).to_string();

         let length = head
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.trim().parse::<usize>().ok())
            .unwrap_or(0);

         if buffer.len() >= end + 4 + length {
            return parse(&buffer[..end + 4 + length]);
         }
      }

      match stream.read(&mut chunk).await {
         Ok(0) | Err(_) => return parse(&buffer),
         Ok(read) => buffer.extend_from_slice(&chunk[..read]),
      }
   }
}

fn head_end(buffer: &[u8]) -> Option<usize> {
   buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

#[tokio::test]
async fn a_malformed_body_is_a_parse_error() {
   let fixture = Fixture::start(true);

   let reply = request(
      fixture.port,
      "POST",
      "/mcp",
      &[(
         "Authorization",
         &format!("Bearer {}", fixture.token),
      )],
      "{not json",
      None,
   )
   .await;

   assert_eq!(reply.status, 400);
   assert_eq!(reply.error_code(), Some(-32700));
}

#[tokio::test]
async fn the_rate_limit_answers_with_a_wait() {
   let fixture = Fixture::start(true);
   let auth = format!("Bearer {}", fixture.token);

   let mut refused = None;

   for id in 0..(zeus_mcp::limit::CALLS_PER_WINDOW + 5) {
      let reply = request(
         fixture.port,
         "POST",
         "/mcp",
         &[("Authorization", &auth)],
         &call(u64::from(id), "tools/list", json!({})),
         None,
      )
      .await;

      if reply.status == 429 {
         refused = Some(reply);
         break;
      }
   }

   let refused = refused.expect("the budget runs out");
   assert_eq!(refused.header("retry-after"), Some("1"));
   assert!(refused.body.pointer("/error/retry_after_ms").and_then(Value::as_u64).is_some());

   // The budget is per credential, so a fresh profile still has its own.
   let (token, hash) = zeus_core::agent::generate_token();
   let other = AgentProfile::new("second harness", hash, 0);
   fixture.store().insert(other);

   let reply = request(
      fixture.port,
      "POST",
      "/mcp",
      &[(
         "Authorization",
         &format!("Bearer {}", token.as_str()),
      )],
      &call(1, "tools/list", json!({})),
      None,
   )
   .await;

   assert_eq!(reply.status, 200);
}

#[tokio::test]
async fn two_servers_do_not_fight_over_the_port_and_a_stop_releases_it() {
   workspace();

   let first = Fixture::start(true);
   let port = first.port;

   // The same preferred port, while the first listener holds it, walks to the next one down.
   let ctx = first.ctx.clone();
   let mut config = ServerConfig::new(
      ctx,
      std::sync::Arc::new(NoOps),
      tokio::runtime::Handle::current(),
   );
   config.preferred_port = port;

   let second = ServerHandle::start(config).expect("the walk finds a free port");
   assert_ne!(second.port(), port);

   // The walk goes down from the port it was asked for, and any other listener of this suite may hold
   // one of the ports in between, so the guarantee is "below, within the walk", not "exactly one less".
   assert!(
      second.port() < port,
      "{} must be below {port}",
      second.port()
   );
   assert!(
      port - second.port() <= 8,
      "{} must be in the walk",
      second.port()
   );

   let released = second.port();
   second.stop();
   assert_eq!(second.status(), ServerStatus::Stopped);

   // Closing the listener is the aborted task's last act, and an abort lands on the runtime's next
   // pass, so the port can take a tick to come back — but nothing else may hold it.
   let mut third = None;

   for _ in 0..50 {
      let mut config = ServerConfig::new(
         first.ctx.clone(),
         std::sync::Arc::new(NoOps),
         tokio::runtime::Handle::current(),
      );
      config.preferred_port = released;

      let handle = ServerHandle::start(config).expect("a server starts");

      if handle.port() == released {
         third = Some(handle);
         break;
      }

      handle.stop();
      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
   }

   let third = third.expect("the port comes back once the stopped task is dropped");
   third.stop();
}

/// Test connection is the same handshake a harness performs, exercised against a real endpoint
/// rather than a mock: the probe must see the server's name and the tools the grant exposes.
#[tokio::test]
async fn the_probe_performs_the_handshake_the_connect_panel_offers() {
   let fixture = Fixture::start(true);
   let port = fixture.port;
   let token = fixture.token.clone();

   let report = tokio::task::spawn_blocking(move || zeus_mcp::probe::probe(port, &token))
      .await
      .expect("the probe task must finish")
      .expect("the probe must complete initialize and tools/list");

   assert_eq!(report.server, "zeus");
   assert!(
      !report.version.is_empty(),
      "the report must carry the server's version"
   );
   assert!(
      report.tools >= 2,
      "the grant lists at least the two meta tools: {report:?}"
   );

   let port = fixture.port;

   let refused = tokio::task::spawn_blocking(move || zeus_mcp::probe::probe(port, "zk_wrong"))
      .await
      .expect("the probe task must finish")
      .expect_err("a token that matches no profile must be refused");

   assert!(refused.contains("refused the token"), "{refused}");
}

/// The read capabilities the fixture's one wallet can hold.
const READ_CAPS: &[&str] = &[
   "wallet.read.portfolio",
   "wallet.read.history",
   "wallet.read.approvals",
   "wallet.read.private",
];

/// The global screens the read surface answers for.
const READ_SCREENS: &[&str] = &[
   "settings.wallets.read",
   "settings.general.read",
   "settings.security.read",
   "settings.networks.read",
   "settings.contacts.read",
   "settings.railgun.read",
   "settings.pools.read",
   "settings.dapps.read",
];

/// The fixture's wallet, as every request names it.
const WALLET: &str = "0x1111111111111111111111111111111111111111";

/// A tool call, as a harness writes it.
async fn call_tool(fixture: &Fixture, name: &str, arguments: Value) -> Reply {
   request(
      fixture.port,
      "POST",
      "/mcp",
      &[(
         "Authorization",
         &format!("Bearer {}", fixture.token),
      )],
      &call(
         1,
         "tools/call",
         json!({"name": name, "arguments": arguments}),
      ),
      None,
   )
   .await
}

/// The tool names one list answer carries.
fn tool_names(reply: &Reply) -> Vec<String> {
   reply
      .body
      .pointer("/result/tools")
      .and_then(Value::as_array)
      .map(|tools| {
         tools
            .iter()
            .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
            .collect()
      })
      .unwrap_or_default()
}

/// The data of a successful tool answer.
fn data(reply: &Reply) -> &Value {
   reply
      .body
      .pointer("/result/structuredContent/data")
      .expect("a successful tool answer carries data")
}

/// The error code of a refused tool answer.
fn error_code(reply: &Reply) -> Option<&str> {
   reply
      .body
      .pointer("/result/structuredContent/error/code")
      .and_then(Value::as_str)
}

#[tokio::test]
async fn the_read_surface_is_scoped_to_the_grant() {
   // A profile with nothing granted sees the two meta tools and the list of what it could be granted —
   // no wallet tool, because the wallet read is itself a grant.
   let bare = Fixture::with(&[], &[]);
   let list = request(
      bare.port,
      "POST",
      "/mcp",
      &[("Authorization", &format!("Bearer {}", bare.token))],
      &call(1, "tools/list", json!({})),
      None,
   )
   .await;

   assert_eq!(
      tool_names(&list),
      ["settings_get", "settings_list", "zeus_help", "zeus_status"]
   );

   // Granted everything the read surface needs, it is the whole surface and nothing that writes.
   let fixture = Fixture::with(READ_CAPS, READ_SCREENS);
   let list = request(
      fixture.port,
      "POST",
      "/mcp",
      &[(
         "Authorization",
         &format!("Bearer {}", fixture.token),
      )],
      &call(1, "tools/list", json!({})),
      None,
   )
   .await;

   assert_eq!(
      tool_names(&list),
      [
         "approvals_list",
         "contacts_list",
         "dapps_list",
         "delegate_status",
         "networks_list",
         "pools_list",
         "portfolio_get",
         "railgun_status",
         "settings_get",
         "settings_list",
         "tx_get",
         "tx_history",
         "wallet_get",
         "wallet_list",
         "zeus_help",
         "zeus_status"
      ]
   );
}

#[tokio::test]
async fn every_read_tool_answers_for_a_granted_profile() {
   let fixture = Fixture::with(READ_CAPS, READ_SCREENS);
   let address: Address = WALLET.parse().expect("the fixture's address");

   // One recorded transaction, so the history tools answer a row rather than an empty shape.
   let transaction = TransactionRich {
      chain: 1,
      block: 42,
      hash: TxHash::repeat_byte(9),
      timestamp: TimeStamp::Seconds(1_700_000_000),
      value_sent: NumericValue::parse_to_wei("1.5", 6),
      ..Default::default()
   };
   fixture
      .ctx
      .tx_db()
      .add_tx(1, address, transaction)
      .expect("the history records the transaction");

   // No node answers in this fixture, so the delegation check is told it ran a moment ago.
   fixture.ctx.write(|ctx| {
      ctx.delegated_wallets.last_check.insert((1, address), u64::MAX);
   });

   let hash = format!("{:#x}", TxHash::repeat_byte(9));

   let calls: Vec<(&str, Value)> = vec![
      ("wallet_list", json!({})),
      ("wallet_get", json!({"wallet": WALLET})),
      ("portfolio_get", json!({"wallet": WALLET})),
      (
         "tx_history",
         json!({"wallet": WALLET, "chain": 1}),
      ),
      ("tx_get", json!({"wallet": WALLET, "hash": hash})),
      ("approvals_list", json!({"wallet": WALLET})),
      ("railgun_status", json!({"wallet": WALLET})),
      (
         "delegate_status",
         json!({"wallet": WALLET, "chain": 1}),
      ),
      ("contacts_list", json!({})),
      ("networks_list", json!({})),
      ("pools_list", json!({})),
      ("dapps_list", json!({})),
      ("settings_list", json!({})),
      ("settings_get", json!({"screen": "general"})),
      ("settings_get", json!({"screen": "security"})),
      ("settings_get", json!({"screen": "networks"})),
      ("settings_get", json!({"screen": "railgun"})),
      ("settings_get", json!({"screen": "contacts"})),
      ("settings_get", json!({"screen": "pools"})),
      ("settings_get", json!({"screen": "dapps"})),
      ("settings_get", json!({"screen": "wallets"})),
   ];

   for (name, arguments) in calls {
      let reply = call_tool(&fixture, name, arguments).await;

      assert_eq!(reply.status, 200, "{name}");
      assert_eq!(
         reply.body.pointer("/result/isError"),
         None,
         "{name} refused: {}",
         reply.body
      );
      assert!(
         data(&reply).is_object(),
         "{name} did not answer an object"
      );
   }

   // The row the history tools answer names the recorded block and carries an exact amount: the raw
   // integer as a string, the readable form beside it, and the token's own decimals.
   let history = call_tool(
      &fixture,
      "tx_history",
      json!({"wallet": WALLET, "chain": 1}),
   )
   .await;
   let rows = data(&history)["transactions"].as_array().expect("a transaction list");

   assert_eq!(rows.len(), 1);
   assert_eq!(rows[0]["block"], 42);
   assert_eq!(rows[0]["value"]["raw"], "1500000");
   assert!(rows[0]["value"]["amount"].is_string());
   assert_eq!(
      rows[0]["value"]["decimals"], 18,
      "a row whose event names no token falls back to the native decimals"
   );
   assert!(rows[0]["value"]["usd"]["known"].is_boolean());
   assert_eq!(rows[0]["fee"]["decimals"], 18);
   assert!(
      rows[0]["timestamp"].as_u64().is_some(),
      "a row carries the instant it landed"
   );
}

#[tokio::test]
async fn a_read_tool_without_its_capability_names_the_switch() {
   let fixture = Fixture::with(&[], &["settings.wallets.read"]);

   let reply = call_tool(
      &fixture,
      "portfolio_get",
      json!({"wallet": WALLET}),
   )
   .await;

   assert_eq!(
      reply.body.pointer("/result/isError").and_then(Value::as_bool),
      Some(true)
   );
   assert_eq!(error_code(&reply), Some("NOT_GRANTED"));

   let message = reply
      .body
      .pointer("/result/structuredContent/error/message")
      .and_then(Value::as_str)
      .unwrap_or_default();

   assert!(
      message.contains("wallet.read.portfolio"),
      "{message}"
   );
}

#[tokio::test]
async fn an_ungranted_wallet_is_not_found_on_the_global_tools() {
   let fixture = Fixture::with(&[], &["settings.wallets.read"]);

   let reply = call_tool(
      &fixture,
      "delegate_status",
      json!({"wallet": "0x2222222222222222222222222222222222222222"}),
   )
   .await;

   assert_eq!(error_code(&reply), Some("NOT_FOUND"));

   let message = reply
      .body
      .pointer("/result/structuredContent/error/message")
      .and_then(Value::as_str)
      .unwrap_or_default();

   assert!(message.contains("0x2222"), "{message}");
}

#[tokio::test]
async fn a_locked_vault_refuses_every_read_tool() {
   let fixture = Fixture::with(READ_CAPS, READ_SCREENS);

   // The auto-lock timer fired: the server is still up, and every wallet read is refused.
   fixture.ctx.write(|ctx| ctx.locked = true);

   let calls: Vec<(&str, Value)> = vec![
      ("wallet_list", json!({})),
      ("wallet_get", json!({"wallet": WALLET})),
      ("portfolio_get", json!({"wallet": WALLET})),
      ("tx_history", json!({"wallet": WALLET})),
      (
         "tx_get",
         json!({"wallet": WALLET, "hash": format!("{:#x}", TxHash::repeat_byte(1))}),
      ),
      ("approvals_list", json!({"wallet": WALLET})),
      ("railgun_status", json!({"wallet": WALLET})),
      ("delegate_status", json!({"wallet": WALLET})),
      ("contacts_list", json!({})),
      ("networks_list", json!({})),
      ("pools_list", json!({})),
      ("dapps_list", json!({})),
      ("settings_list", json!({})),
      ("settings_get", json!({"screen": "general"})),
   ];

   for (name, arguments) in calls {
      let reply = call_tool(&fixture, name, arguments).await;
      assert_eq!(error_code(&reply), Some("VAULT_LOCKED"), "{name}");
   }

   // Status is the one answer that still works: it is how a harness learns *why* everything else fails.
   let status = call_tool(&fixture, "zeus_status", json!({})).await;

   assert_eq!(status.body.pointer("/result/isError"), None);
   assert_eq!(
      data(&status)["vault"]["unlocked"],
      false,
      "status is still answered while locked, and says the wallet is not open"
   );
}

#[tokio::test]
async fn a_settings_screen_needs_its_own_switch() {
   let fixture = Fixture::with(&[], &["settings.wallets.read"]);

   let refused = call_tool(
      &fixture,
      "settings_get",
      json!({"screen": "general"}),
   )
   .await;
   assert_eq!(error_code(&refused), Some("NOT_GRANTED"));

   // An unknown screen is not a grant refusal at all: this build has no values for it, so it is not
   // found.
   let missing = call_tool(
      &fixture,
      "settings_get",
      json!({"screen": "nowhere"}),
   )
   .await;
   assert_eq!(error_code(&missing), Some("NOT_FOUND"));
}

#[tokio::test]
async fn a_resource_follows_the_grant_and_the_templates_are_published() {
   let uri = format!("zeus://wallet/{WALLET}/portfolio");

   let bare = Fixture::with(&[], &[]);
   let refused = request(
      bare.port,
      "POST",
      "/mcp",
      &[("Authorization", &format!("Bearer {}", bare.token))],
      &call(1, "resources/read", json!({"uri": uri})),
      None,
   )
   .await;

   assert_eq!(refused.error_code(), Some(-32602));

   let templates = request(
      bare.port,
      "POST",
      "/mcp",
      &[("Authorization", &format!("Bearer {}", bare.token))],
      &call(2, "resources/templates/list", json!({})),
      None,
   )
   .await;

   let templates = templates
      .body
      .pointer("/result/resourceTemplates")
      .and_then(Value::as_array)
      .expect("a template list");

   assert_eq!(templates.len(), 4);

   // Granted, the same URI answers the same document portfolio_get would.
   let fixture = Fixture::with(READ_CAPS, READ_SCREENS);
   let granted = request(
      fixture.port,
      "POST",
      "/mcp",
      &[(
         "Authorization",
         &format!("Bearer {}", fixture.token),
      )],
      &call(1, "resources/read", json!({"uri": uri})),
      None,
   )
   .await;

   assert_eq!(granted.status, 200);

   let text = granted
      .body
      .pointer("/result/contents/0/text")
      .and_then(Value::as_str)
      .expect("the document as text");

   let document: Value = serde_json::from_str(text).expect("the document is JSON");
   assert_eq!(document["wallet"], WALLET);

   let screen = request(
      fixture.port,
      "POST",
      "/mcp",
      &[(
         "Authorization",
         &format!("Bearer {}", fixture.token),
      )],
      &call(
         2,
         "resources/read",
         json!({"uri": "zeus://settings/general"}),
      ),
      None,
   )
   .await;

   assert_eq!(screen.status, 200);
}
