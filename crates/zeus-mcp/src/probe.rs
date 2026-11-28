//! Test connection: the handshake the Connect panel offers, performed for real.
//!
//! A user who has pasted a snippet needs one control that answers "is it working" without leaving
//! Zeus, and the answer has to be the answer a harness gets: a real HTTP request to the endpoint,
//! with the real credential, over the socket. So this is a small blocking client, not a mock: two
//! requests on two connections (the transport is stateless, so each stands alone) and a sentence
//! the page can print.

use crate::protocol::LEGACY_VERSION;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

/// How long the probe waits for the socket, and for each read.
const TIMEOUT: Duration = Duration::from_secs(3);

/// What a successful probe learned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
   /// The name the endpoint gives itself (`serverInfo.name`).
   pub server: String,
   /// Its version.
   pub version: String,
   /// How many tools it lists for this credential.
   pub tools: usize,
}

/// Perform the handshake a harness performs, and say what came back.
pub fn probe(port: u16, token: &str) -> Result<Report, String> {
   let initialize = json!({
      "jsonrpc": "2.0",
      "id": 1,
      "method": "initialize",
      "params": {
         "protocolVersion": LEGACY_VERSION,
         "capabilities": {},
         "clientInfo": { "name": "zeus-connect", "version": env!("CARGO_PKG_VERSION") },
      }
   });

   let answer = exchange(port, token, &initialize)?;
   let info = answer.pointer("/result/serverInfo");

   let server = info
      .and_then(|value| value.get("name"))
      .and_then(Value::as_str)
      .unwrap_or("zeus")
      .to_owned();

   let version = info
      .and_then(|value| value.get("version"))
      .and_then(Value::as_str)
      .unwrap_or("unknown")
      .to_owned();

   let list = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} });
   let answer = exchange(port, token, &list)?;

   let tools = answer
      .pointer("/result/tools")
      .and_then(Value::as_array)
      .map(|tools| tools.len())
      .ok_or_else(|| format!("the endpoint answered tools/list without a tool list: {answer}"))?;

   Ok(Report {
      server,
      version,
      tools,
   })
}

/// One request, one answer: the whole of the transport the probe needs.
fn exchange(port: u16, token: &str, body: &Value) -> Result<Value, String> {
   let address = SocketAddr::from(([127, 0, 0, 1], port));
   let mut stream = TcpStream::connect_timeout(&address, TIMEOUT)
      .map_err(|e| format!("nothing is listening on 127.0.0.1:{port}: {e}"))?;

   stream
      .set_read_timeout(Some(TIMEOUT))
      .map_err(|e| format!("could not set a read timeout: {e}"))?;

   stream
      .set_write_timeout(Some(TIMEOUT))
      .map_err(|e| format!("could not set a write timeout: {e}"))?;

   let text =
      serde_json::to_string(body).map_err(|e| format!("could not encode the request: {e}"))?;

   // The Host header is what the endpoint validates, and it has to carry the port the listener
   // bound: a Host without one is refused before the method is even read.
   let request = format!(
      "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\n\
       Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
      text.len()
   );

   stream
      .write_all(request.as_bytes())
      .map_err(|e| format!("could not send the request: {e}"))?;

   let (status, body) = read_response(&mut stream)?;

   if status == 401 {
      return Err("the endpoint refused the token".to_owned());
   }

   let answer: Value =
      serde_json::from_str(&body).map_err(|e| format!("the answer was not JSON: {e}"))?;

   if let Some(error) = answer.get("error") {
      let message = error.get("message").and_then(Value::as_str).unwrap_or("unknown error");
      return Err(format!(
         "the endpoint answered {status} with an error: {message}"
      ));
   }

   if status != 200 {
      return Err(format!("the endpoint answered {status}: {body}"));
   }

   Ok(answer)
}

/// Read one HTTP/1.1 response: its status code and its body.
///
/// The body comes from `Content-Length` rather than from the end of the stream, because the
/// endpoint keeps a connection alive for the next request by default.
fn read_response(stream: &mut TcpStream) -> Result<(u16, String), String> {
   let mut buffer = Vec::new();
   let mut chunk = [0u8; 1024];

   loop {
      let read = stream.read(&mut chunk).map_err(|e| format!("could not read the answer: {e}"))?;

      if read == 0 {
         return Err("the connection closed before the answer was complete".to_owned());
      }

      buffer.extend_from_slice(&chunk[..read]);

      let Some(head_end) = find_head_end(&buffer) else {
         continue;
      };

      let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
      let status = parse_status(&head)?;
      let length = parse_content_length(&head)?;

      if buffer.len() >= head_end + 4 + length {
         let body = &buffer[head_end + 4..head_end + 4 + length];
         return Ok((status, String::from_utf8_lossy(body).into_owned()));
      }
   }
}

/// Where the headers end: the index of the first byte of the blank line's CR LF.
fn find_head_end(buffer: &[u8]) -> Option<usize> {
   buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

/// The status code of a response's first line.
fn parse_status(head: &str) -> Result<u16, String> {
   head
      .lines()
      .next()
      .and_then(|line| line.split_whitespace().nth(1))
      .and_then(|code| code.parse().ok())
      .ok_or_else(|| format!("the answer had no status line: {head}"))
}

/// The body's declared length, from a header whose name is case-insensitive.
fn parse_content_length(head: &str) -> Result<usize, String> {
   head
      .lines()
      .filter_map(|line| line.split_once(':'))
      .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
      .and_then(|(_, value)| value.trim().parse().ok())
      .ok_or_else(|| "the answer declared no content-length".to_owned())
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn the_status_line_is_read_from_the_first_line() {
      assert_eq!(parse_status("HTTP/1.1 200 OK\r\n").unwrap(), 200);
      assert_eq!(
         parse_status("HTTP/1.1 401 Unauthorized\r\n").unwrap(),
         401
      );
      assert!(parse_status("").is_err());
   }

   #[test]
   fn the_content_length_header_is_case_insensitive() {
      let head = "HTTP/1.1 200 OK\r\ncontent-length: 12\r\nContent-Type: application/json\r\n";
      assert_eq!(parse_content_length(head).unwrap(), 12);
      assert!(parse_content_length("HTTP/1.1 200 OK\r\n").is_err());
   }

   #[test]
   fn the_head_ends_at_the_blank_line() {
      let buffer = b"HTTP/1.1 200 OK\r\n\r\n{}";
      assert_eq!(find_head_end(buffer), Some(15));
   }

   #[test]
   fn a_port_nobody_listens_on_is_reported_as_such() {
      let error = probe(1, "zk_test").unwrap_err();
      assert!(
         error.starts_with("nothing is listening on 127.0.0.1:1"),
         "{error}"
      );
   }
}
