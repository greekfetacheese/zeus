//! The endpoint from the outside: real HTTP over loopback, one request at a time.
//!
//! Every test drives the same public surface a harness does — bind, advertise, authenticate, answer —
//! with a hand-rolled client rather than a client library, so the assertions are about the bytes on the
//! wire: the status code, the headers, and the JSON body. Nothing here reaches into the crate's
//! internals, which is the point: this is the contract a harness sees.

use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zeus_core::agent::settings::{Applied, Closed, SettingsError};
use zeus_core::agent::{
   AgentProfile, AgentProfileId, AgentProfileStore, ProfileMode, ScreenGrant, WalletGrant,
};
use zeus_core::{TimeStamp, TransactionRich, WalletInfo, ZeusCtx};
use zeus_eth::alloy_primitives::{Address, TxHash};
use zeus_eth::utils::NumericValue;
use zeus_mcp::protocol::{LEGACY_VERSION, MODERN_VERSION, meta};
use zeus_mcp::{AgentOps, AppInfo, BinScreen, NoOps, ServerConfig, ServerHandle, ServerStatus};

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
      Self::with_writes(caps, screens, &[])
   }

   /// A server whose profile also holds these screen writes.
   ///
   /// A screen named in both lists is read and written; one named only in `writes` is written and not
   /// read, which is the grant a write-only profile holds.
   fn with_writes(caps: &[&str], screens: &[&str], writes: &[&str]) -> Self {
      Self::with_port(caps, screens, writes, Arc::new(NoOps))
   }

   /// A server over the port it is given, so the binary's own screens can be tested.
   ///
   /// The real three are typed in the binary crate, which this one cannot name; the port is the seam, and
   /// the double below is the other side of it.
   fn with_port(caps: &[&str], screens: &[&str], writes: &[&str], ops: Arc<dyn AgentOps>) -> Self {
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

      for screen in writes {
         profile.screens.entry((*screen).to_owned()).or_default().write = true;
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
         ops,
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

   /// Give the context the key a sealed store is written with.
   ///
   /// The real app has one from the first unlock; a test that writes a sealed file has to make one, or the
   /// save is refused for a reason that has nothing to do with what the test is about.
   fn with_wallet_state_key(&self) {
      self
         .ctx
         .write_vault(|vault| vault.ensure_wallet_state_key())
         .expect("a wallet state key");
   }

   /// Put the profile in the cautious mode, where every state change is asked about.
   fn ask_before_every_change(&self) {
      let mut profile = self.store().profile(&self.profile).expect("the fixture's profile");

      profile.mode = ProfileMode::ConfirmEach;
      self.store().insert(profile);
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
         "settings_describe",
         "settings_get",
         "settings_list",
         "settings_set",
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
      [
         "settings_describe",
         "settings_get",
         "settings_list",
         "settings_set",
         "zeus_help",
         "zeus_status"
      ]
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
   "settings.unshield.read",
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
   // A profile with nothing granted sees the two meta tools and the four settings tools — no wallet
   // tool, because the wallet read is itself a grant. The settings surface is listed anyway: which
   // screen a call names is the argument's answer, so it cannot be scoped by a static capability.
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
      [
         "settings_describe",
         "settings_get",
         "settings_list",
         "settings_set",
         "zeus_help",
         "zeus_status"
      ]
   );

   // Granted everything the read surface needs, it is the whole surface. `settings_set` is in it
   // because a settings call names its screen as an argument, and it still refuses per screen.
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
         "settings_describe",
         "settings_get",
         "settings_list",
         "settings_set",
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
async fn a_locked_vault_refuses_every_tool_but_status() {
   let fixture = Fixture::with(READ_CAPS, READ_SCREENS);

   // The auto-lock timer fired: the server is still up, and every read and every write is refused.
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
      ("settings_describe", json!({"screen": "general"})),
      (
         "settings_set",
         json!({"screen": "general", "patch": {"check_for_updates": true}}),
      ),
      (
         "networks_set_rpc",
         json!({"chain": 1, "url": "https://example.invalid", "action": "add"}),
      ),
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
async fn a_settings_write_lands_and_is_audited() {
   let fixture = Fixture::with_writes(
      &[],
      &["settings.general.read"],
      &["settings.general.write"],
   );

   let reply = call_tool(
      &fixture,
      "settings_set",
      json!({"screen": "general", "patch": {"check_for_updates": true}}),
   )
   .await;

   assert_eq!(
      reply.body.pointer("/result/isError"),
      None,
      "{}",
      reply.body
   );
   assert_eq!(data(&reply)["screen"], "general");
   assert_eq!(
      data(&reply)["changed"],
      json!(["check_for_updates"])
   );
   assert_eq!(
      data(&reply)["values"]["check_for_updates"],
      true,
      "the answer carries the screen as it stands now"
   );

   // An allowed write is on the ring, not only a refused one.
   let tail = fixture.ctx.agent_audit().tail(10);
   let entry = tail.last().expect("an audit entry");

   assert_eq!(entry.tool, "settings_set");
   assert_eq!(entry.capability, "settings.general.write");
   assert_eq!(
      entry.decision,
      zeus_core::agent::AuditDecision::Allowed
   );
}

#[tokio::test]
async fn a_settings_write_needs_its_own_switch() {
   let fixture = Fixture::with(&[], &["settings.general.read"]);

   let reply = call_tool(
      &fixture,
      "settings_set",
      json!({"screen": "general", "patch": {"check_for_updates": true}}),
   )
   .await;

   assert_eq!(error_code(&reply), Some("NOT_GRANTED"));

   let message = reply
      .body
      .pointer("/result/structuredContent/error/message")
      .and_then(Value::as_str)
      .unwrap_or_default();

   assert!(
      message.contains("settings.general.write"),
      "{message}"
   );
}

#[tokio::test]
async fn a_patch_field_the_screen_does_not_have_is_a_validation_error() {
   let fixture = Fixture::with_writes(
      &[],
      &["settings.general.read"],
      &["settings.general.write"],
   );

   let reply = call_tool(
      &fixture,
      "settings_set",
      json!({"screen": "general", "patch": {"fetch_prices": true}}),
   )
   .await;

   assert_eq!(error_code(&reply), Some("VALIDATION"));

   let message = reply
      .body
      .pointer("/result/structuredContent/error/message")
      .and_then(Value::as_str)
      .unwrap_or_default();

   assert!(message.contains("fetch_prices"), "{message}");
}

#[tokio::test]
async fn a_runtime_field_names_the_call_it_belongs_to() {
   let fixture = Fixture::with_writes(
      &[],
      &["settings.security.read"],
      &["settings.security.write"],
   );

   let reply = call_tool(
      &fixture,
      "settings_set",
      json!({"screen": "security", "patch": {"privacy_mode": true}}),
   )
   .await;

   assert_eq!(error_code(&reply), Some("VALIDATION"));

   let message = reply
      .body
      .pointer("/result/structuredContent/error/message")
      .and_then(Value::as_str)
      .unwrap_or_default();

   assert!(message.contains("per call"), "{message}");
}

#[tokio::test]
async fn a_screen_with_no_writer_is_refused_as_read_only() {
   let fixture = Fixture::with_writes(
      &[],
      &["settings.pools.read"],
      &["settings.pools.write"],
   );

   let reply = call_tool(
      &fixture,
      "settings_set",
      json!({"screen": "pools", "patch": {}}),
   )
   .await;

   assert_eq!(error_code(&reply), Some("NOT_FOUND"));

   let message = reply
      .body
      .pointer("/result/structuredContent/error/message")
      .and_then(Value::as_str)
      .unwrap_or_default();

   assert!(message.contains("read-only"), "{message}");
}

#[tokio::test]
async fn a_settings_description_names_the_fields_an_agent_may_not_write() {
   let fixture = Fixture::with(&[], &["settings.general.read"]);

   let reply = call_tool(
      &fixture,
      "settings_describe",
      json!({"screen": "general"}),
   )
   .await;

   assert_eq!(
      reply.body.pointer("/result/isError"),
      None,
      "{}",
      reply.body
   );

   let described = data(&reply);

   assert_eq!(described["writable"], true);
   assert_eq!(
      described["schema"]["properties"]["fetch_asset_images"]["type"],
      "boolean"
   );
   assert!(
      described["schema"]["properties"]["agent_server"].is_null(),
      "the server's own switch is not a field of the patch"
   );
   assert!(
      described["closed"]
         .as_array()
         .expect("a closed list")
         .iter()
         .any(|entry| entry["field"] == "agent_server")
   );
}

#[tokio::test]
async fn settings_list_says_which_screens_this_build_can_write() {
   let fixture = Fixture::with(&[], &["settings.general.read"]);

   let reply = call_tool(&fixture, "settings_list", json!({})).await;
   let screens = data(&reply)["screens"].as_array().expect("a screen list");

   let row = |name: &str| {
      screens
         .iter()
         .find(|screen| screen["screen"] == name)
         .cloned()
         .unwrap_or_else(|| panic!("{name} is missing from the list"))
   };

   let general = row("general");
   assert_eq!(
      general["writable"], true,
      "this build writes general"
   );
   assert_eq!(general["write"], false, "the grant does not");

   for name in ["railgun", "unshield"] {
      assert_eq!(
         row(name)["writable"],
         true,
         "this build writes {name}"
      );
   }

   let pools = row("pools");
   assert_eq!(
      pools["writable"], false,
      "no writer for pools: adding one is an action, not a patch"
   );
   assert_eq!(
      pools["read"], false,
      "and its read switch is not granted either"
   );
}

#[tokio::test]
async fn a_confirm_each_profile_is_asked_before_a_settings_write() {
   let fixture = Fixture::with_writes(
      &[],
      &["settings.general.read"],
      &["settings.general.write"],
   );
   fixture.ask_before_every_change();

   let reply = call_tool(
      &fixture,
      "settings_set",
      json!({"screen": "general", "patch": {"check_for_updates": true}}),
   )
   .await;

   // No window is attached in this test, so the prompt cannot be drawn. The write must not happen on an
   // assumed answer: the profile asked to be asked, and nothing answered.
   assert_eq!(error_code(&reply), Some("USER_ACTION_REQUIRED"));
   assert!(!fixture.ctx.read(|ctx| ctx.misc_config.check_for_updates()));

   let tail = fixture.ctx.agent_audit().tail(10);
   let entry = tail.last().expect("an audit entry");

   assert_eq!(entry.denial.as_deref(), Some("NO_UI"));
}

#[tokio::test]
async fn a_networks_patch_saves_through_the_client_settings() {
   let fixture = Fixture::with_writes(
      &[],
      &["settings.networks.read"],
      &["settings.networks.write"],
   );

   let reply = call_tool(
      &fixture,
      "settings_set",
      json!({"screen": "networks", "patch": {"concurrency": 5}}),
   )
   .await;

   assert_eq!(
      reply.body.pointer("/result/isError"),
      None,
      "{}",
      reply.body
   );
   assert_eq!(data(&reply)["changed"], json!(["concurrency"]));
   assert_eq!(fixture.ctx.get_client_manager().concurrency(), 5);
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

#[tokio::test]
async fn a_railgun_patch_is_saved_for_the_next_launch() {
   let fixture = Fixture::with_writes(
      &[],
      &["settings.railgun.read"],
      &["settings.railgun.write"],
   );
   fixture.with_wallet_state_key();

   let reply = call_tool(
      &fixture,
      "settings_set",
      json!({"screen": "railgun", "patch": {"syncer_concurrency": 3}}),
   )
   .await;

   assert_eq!(
      reply.body.pointer("/result/isError"),
      None,
      "{}",
      reply.body
   );
   assert_eq!(
      data(&reply)["changed"],
      json!(["syncer_concurrency"])
   );
   assert_eq!(
      data(&reply)["values"]["syncer_concurrency"],
      3,
      "the answer carries the screen as it stands now"
   );
   assert_eq!(
      fixture.ctx.read(|ctx| ctx.railgun_config.rpc_syncer_concurrency()),
      3
   );

   // The file a restart reads, not only the copy in memory.
   let saved = std::fs::read_to_string(workspace().join("data").join("railgun_config.json"))
      .expect("the saved config");

   assert!(
      saved.contains("\"rpc_syncer_concurrency\":3"),
      "{saved}"
   );
}

#[tokio::test]
async fn an_unshield_bundler_url_is_saved_and_answered_as_a_host() {
   let fixture = Fixture::with_writes(
      &[],
      &["settings.unshield.read"],
      &["settings.unshield.write"],
   );
   fixture.with_wallet_state_key();

   let url = "https://bundler.example/v1/1/rpc?apikey=secret";

   let reply = call_tool(
      &fixture,
      "settings_set",
      json!({"screen": "unshield", "patch": {"bundler_url": url}}),
   )
   .await;

   assert_eq!(
      reply.body.pointer("/result/isError"),
      None,
      "{}",
      reply.body
   );
   assert_eq!(data(&reply)["changed"], json!(["bundler_url"]));
   assert_eq!(
      data(&reply)["values"]["bundler"]["host"],
      "bundler.example",
      "the answer names the host, which is the part without the key"
   );

   let answer = data(&reply).to_string();
   assert!(!answer.contains("apikey"), "{answer}");

   assert_eq!(fixture.ctx.bundler_url().url, url);
   assert!(workspace().join("data").join("bundler_url.data").exists());
}

#[tokio::test]
async fn an_endpoint_can_be_disabled_mev_protected_and_removed() {
   let fixture = Fixture::with_writes(
      &[],
      &["settings.networks.read"],
      &["settings.networks.write"],
   );
   fixture.with_wallet_state_key();

   // Zeus ships endpoints for Ethereum; acting on one of them keeps this test off the network.
   let url = fixture
      .ctx
      .get_client_manager()
      .get_rpcs(1)
      .keys()
      .next()
      .expect("a built-in endpoint")
      .to_string();

   let reply = call_tool(
      &fixture,
      "networks_set_rpc",
      json!({"chain": 1, "url": url, "action": "disable"}),
   )
   .await;

   assert_eq!(
      reply.body.pointer("/result/isError"),
      None,
      "{}",
      reply.body
   );
   assert_eq!(data(&reply)["endpoint"]["enabled"], false);

   let reply = call_tool(
      &fixture,
      "networks_set_rpc",
      json!({"chain": 1, "url": url, "action": "mev_protect", "value": true}),
   )
   .await;

   assert_eq!(data(&reply)["endpoint"]["mev_protect"], true);

   let reply = call_tool(
      &fixture,
      "networks_set_rpc",
      json!({"chain": 1, "url": url, "action": "remove"}),
   )
   .await;

   assert_eq!(
      reply.body.pointer("/result/isError"),
      None,
      "{}",
      reply.body
   );
   assert!(
      !fixture.ctx.get_client_manager().get_rpcs(1).contains_key(url.as_str()),
      "the endpoint is gone from the live set"
   );

   // A write that landed is on the ring, whatever it changed.
   let entry = fixture.ctx.agent_audit().tail(10).last().cloned().expect("an audit entry");

   assert_eq!(entry.tool, "networks_set_rpc");
   assert_eq!(entry.capability, "settings.networks.write");
   assert_eq!(
      entry.decision,
      zeus_core::agent::AuditDecision::Allowed
   );
}

#[tokio::test]
async fn an_endpoint_change_needs_the_networks_write_switch() {
   let fixture = Fixture::with(&[], &["settings.networks.read"]);

   let reply = call_tool(
      &fixture,
      "networks_set_rpc",
      json!({"chain": 1, "url": "https://example.invalid", "action": "add"}),
   )
   .await;

   assert_eq!(error_code(&reply), Some("NOT_GRANTED"));

   let message = reply
      .body
      .pointer("/result/structuredContent/error/message")
      .and_then(Value::as_str)
      .unwrap_or_default();

   assert!(
      message.contains("settings.networks.write"),
      "{message}"
   );
}

#[tokio::test]
async fn adding_an_endpoint_that_is_already_listed_is_refused_first() {
   let fixture = Fixture::with_writes(&[], &[], &["settings.networks.write"]);

   let url = fixture
      .ctx
      .get_client_manager()
      .get_rpcs(1)
      .keys()
      .next()
      .expect("a built-in endpoint")
      .to_string();

   let before = fixture.ctx.get_client_manager().get_rpcs(1).len();

   let reply = call_tool(
      &fixture,
      "networks_set_rpc",
      json!({"chain": 1, "url": url, "action": "add"}),
   )
   .await;

   // Refused before a connection is attempted: re-adding would replace a record the user set up,
   // health readings included.
   assert_eq!(error_code(&reply), Some("VALIDATION"));
   assert_eq!(
      fixture.ctx.get_client_manager().get_rpcs(1).len(),
      before
   );
}

// --- the binary's own screens, through the port --------------------------------------------------

/// The three screens whose types live in the binary, as a double.
///
/// The real implementation is `crate::agent_screens`, which this crate cannot name. This double answers
/// the same records and keeps the keyboard screen's values in memory, so a test can watch a patch go
/// through the port and read the screen back afterwards.
#[derive(Clone)]
struct FakeOps {
   state: Arc<Mutex<FakeState>>,
}

#[derive(Default)]
struct FakeState {
   keyboard: Value,
   /// Every patch the port was asked to apply, in order.
   applied: Vec<(String, Value)>,
   /// Every application-menu change the port was asked for, in order.
   desktop: Vec<bool>,
   /// When set, the installer has nothing to install, so a test can prove the flag is not stored anyway.
   desktop_fails: bool,
}

impl FakeOps {
   fn new() -> Self {
      let state = FakeState {
         keyboard: json!({
             "bindings": [
                 {"action": "next_region", "binding": "F6", "key": "F6", "chord": "plain", "default": true},
             ],
         }),
         ..Default::default()
      };

      Self {
         state: Arc::new(Mutex::new(state)),
      }
   }

   fn applied(&self) -> Vec<(String, Value)> {
      self.state.lock().expect("the double").applied.clone()
   }

   fn desktop_calls(&self) -> Vec<bool> {
      self.state.lock().expect("the double").desktop.clone()
   }

   fn refuse_desktop(&self) {
      self.state.lock().expect("the double").desktop_fails = true;
   }
}

impl AgentOps for FakeOps {
   fn app_info(&self) -> AppInfo {
      AppInfo {
         version: "0.0.0-fake".to_owned(),
      }
   }

   fn bin_screens(&self) -> Vec<BinScreen> {
      let state = self.state.lock().expect("the double");

      vec![
         BinScreen {
            name: "appearance",
            read: "settings.appearance.read",
            write: "settings.appearance.write",
            values: json!({"hold_to_confirm": false}),
            schema: json!({"type": "object", "additionalProperties": false}),
            closed: Vec::new(),
         },
         BinScreen {
            name: "bridge",
            read: "settings.bridge.read",
            write: "settings.bridge.write",
            // The URL never leaves the process: an answer carries its host.
            values: json!({"api": {"host": "across.to", "default": true, "use_api": true}}),
            schema: json!({"type": "object", "additionalProperties": false}),
            closed: Vec::new(),
         },
         BinScreen {
            name: "keyboard",
            read: "settings.keyboard.read",
            write: "settings.keyboard.write",
            values: state.keyboard.clone(),
            schema: json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {"bindings": {"type": "array"}},
            }),
            closed: vec![Closed {
               field: "capture",
               runtime: true,
               why: "the page hands the keyboard over while it records one, so a write names the chord \
                     it wants instead",
            }],
         },
      ]
   }

   fn apply_bin_screen(&self, name: &str, patch: &Value) -> Result<Applied, SettingsError> {
      if name != "keyboard" {
         return Err(SettingsError::refused(format!(
            "the {name} screen is not written by this double"
         )));
      }

      let bindings = patch.get("bindings").and_then(Value::as_array).ok_or_else(|| {
         SettingsError::invalid(
            "the patch is not one the keyboard screen takes: bindings is required",
         )
      })?;

      if bindings.iter().any(|entry| entry.get("key").is_none()) {
         return Err(SettingsError::invalid("a binding needs a key"));
      }

      // An empty patch changes nothing, and the real screen says so too.
      if bindings.is_empty() {
         return Ok(Applied::none());
      }

      let mut state = self.state.lock().expect("the double");
      state.keyboard = json!({"bindings": bindings});
      state.applied.push((name.to_owned(), patch.clone()));

      Ok(Applied {
         changed: vec!["bindings"],
      })
   }

   fn set_desktop_integration(&self, on: bool) -> Result<(), String> {
      let mut state = self.state.lock().expect("the double");
      state.desktop.push(on);

      if state.desktop_fails {
         return Err("the double cannot install a menu entry".to_owned());
      }

      Ok(())
   }
}

#[tokio::test]
async fn a_binary_screen_is_listed_described_and_read_through_the_port() {
   let fixture = Fixture::with_port(
      &[],
      &["settings.keyboard.read"],
      &[],
      Arc::new(FakeOps::new()),
   );

   let list = call_tool(&fixture, "settings_list", json!({})).await;
   let rows = data(&list)["screens"].as_array().cloned().expect("the list");

   let keyboard = rows
      .iter()
      .find(|row| row["screen"] == "keyboard")
      .expect("the port's keyboard screen is listed beside the core ones");

   assert_eq!(keyboard["read"], true);
   assert_eq!(keyboard["write"], false);
   assert_eq!(
      keyboard["writable"], true,
      "the port said it writes this screen, so the list says so too"
   );

   // A screen the grant does not read is still listed: its values are simply not reachable.
   let bridge = rows
      .iter()
      .find(|row| row["screen"] == "bridge")
      .expect("the port's bridge screen");

   assert_eq!(bridge["read"], false);
   assert_eq!(bridge["writable"], true);

   let get = call_tool(
      &fixture,
      "settings_get",
      json!({"screen": "keyboard"}),
   )
   .await;
   assert_eq!(data(&get)["screen"], "keyboard");
   assert_eq!(
      data(&get)["values"]["bindings"][0]["binding"],
      "F6"
   );

   let describe = call_tool(
      &fixture,
      "settings_describe",
      json!({"screen": "keyboard"}),
   )
   .await;

   assert_eq!(data(&describe)["writable"], true);
   assert_eq!(
      data(&describe)["capability"],
      "settings.keyboard.write"
   );
   assert_eq!(data(&describe)["closed"][0]["field"], "capture");
   assert_eq!(data(&describe)["closed"][0]["runtime"], true);

   // A read the grant does not hold names the switch, the same way a core screen does.
   let refused = call_tool(
      &fixture,
      "settings_get",
      json!({"screen": "bridge"}),
   )
   .await;
   assert_eq!(error_code(&refused), Some("NOT_GRANTED"));
}

#[tokio::test]
async fn a_binary_screen_is_written_through_the_port_and_audited() {
   let ops = Arc::new(FakeOps::new());

   let fixture = Fixture::with_port(
      &[],
      &["settings.keyboard.read"],
      &["settings.keyboard.write"],
      ops.clone(),
   );

   let reply = call_tool(
      &fixture,
      "settings_set",
      json!({
          "screen": "keyboard",
          "patch": {"bindings": [{"action": "next_region", "key": "F7", "chord": "plain"}]},
      }),
   )
   .await;

   assert_eq!(
      reply.body.pointer("/result/isError"),
      None,
      "{}",
      reply.body
   );
   assert_eq!(data(&reply)["changed"], json!(["bindings"]));
   assert_eq!(
      data(&reply)["values"]["bindings"][0]["key"],
      "F7",
      "the answer carries the screen as the port reads it now"
   );

   let applied = ops.applied();
   assert_eq!(applied.len(), 1, "one call, one write");
   assert_eq!(applied[0].0, "keyboard");

   let tail = fixture.ctx.agent_audit().tail(10);
   let entry = tail.last().expect("an audit entry");

   assert_eq!(entry.tool, "settings_set");
   assert_eq!(entry.capability, "settings.keyboard.write");
   assert_eq!(
      entry.decision,
      zeus_core::agent::AuditDecision::Allowed
   );
}

#[tokio::test]
async fn a_binary_screen_write_needs_its_own_switch() {
   let ops = Arc::new(FakeOps::new());

   let fixture = Fixture::with_port(&[], &["settings.keyboard.read"], &[], ops.clone());

   let reply = call_tool(
      &fixture,
      "settings_set",
      json!({"screen": "keyboard", "patch": {"bindings": []}}),
   )
   .await;

   assert_eq!(error_code(&reply), Some("NOT_GRANTED"));
   assert!(
      ops.applied().is_empty(),
      "nothing reached the port"
   );
}

#[tokio::test]
async fn a_binary_screen_refusal_is_the_ports_own_sentence() {
   let fixture = Fixture::with_port(
      &[],
      &["settings.keyboard.read"],
      &["settings.keyboard.write"],
      Arc::new(FakeOps::new()),
   );

   let reply = call_tool(
      &fixture,
      "settings_set",
      json!({"screen": "keyboard", "patch": {"bindings": [{"action": "next_region"}]}}),
   )
   .await;

   assert_eq!(error_code(&reply), Some("VALIDATION"));

   let message = reply
      .body
      .pointer("/result/structuredContent/error/message")
      .and_then(Value::as_str)
      .unwrap_or_default();

   assert!(
      message.contains("a binding needs a key"),
      "{message}"
   );
}

#[tokio::test]
async fn a_binary_screen_write_is_asked_about_when_every_change_is() {
   let ops = Arc::new(FakeOps::new());

   let fixture = Fixture::with_port(
      &[],
      &["settings.keyboard.read"],
      &["settings.keyboard.write"],
      ops.clone(),
   );

   fixture.ask_before_every_change();

   let reply = call_tool(
      &fixture,
      "settings_set",
      json!({"screen": "keyboard", "patch": {"bindings": []}}),
   )
   .await;

   assert_eq!(error_code(&reply), Some("USER_ACTION_REQUIRED"));
   assert!(
      ops.applied().is_empty(),
      "a prompt nobody can answer does not write"
   );
}

#[tokio::test]
async fn the_desktop_entry_is_installed_before_the_flag_is_stored() {
   let ops = Arc::new(FakeOps::new());

   let fixture = Fixture::with_port(
      &[],
      &["settings.general.read"],
      &["settings.general.write"],
      ops.clone(),
   );

   let reply = call_tool(
      &fixture,
      "settings_set",
      json!({"screen": "general", "patch": {"desktop_integration": true}}),
   )
   .await;

   assert_eq!(
      reply.body.pointer("/result/isError"),
      None,
      "{}",
      reply.body
   );
   assert_eq!(ops.desktop_calls(), vec![true]);
   assert_eq!(
      data(&reply)["values"]["desktop_integration"],
      true
   );

   // A host that cannot install refuses the write, and the file keeps saying what it said before.
   ops.refuse_desktop();

   let refused = call_tool(
      &fixture,
      "settings_set",
      json!({"screen": "general", "patch": {"desktop_integration": false}}),
   )
   .await;

   assert_eq!(error_code(&refused), Some("INTERNAL"));

   let values = call_tool(
      &fixture,
      "settings_get",
      json!({"screen": "general"}),
   )
   .await;

   assert_eq!(
      data(&values)["values"]["desktop_integration"],
      true,
      "the flag the earlier write stored is untouched"
   );
}
