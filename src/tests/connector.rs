#[cfg(test)]
mod tests {
   use crate::connector::{
      ConnectorSession, decode_native_frame, encode_native_frame, generate_pairing_token,
      parse_dapp_origin, token_matches, write_connector_session,
   };
   use std::fs;

   #[test]
   fn pairing_token_is_64_hex_chars_and_unique() {
      let a = generate_pairing_token();
      let b = generate_pairing_token();
      assert_eq!(a.len(), 64);
      assert_eq!(b.len(), 64);
      assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
      assert_ne!(a, b);
   }

   #[test]
   fn token_matches_accepts_exact_value() {
      let token = "ab".repeat(32);
      assert!(token_matches(&token, &token));
   }

   #[test]
   fn token_matches_rejects_wrong_or_empty() {
      let token = "ab".repeat(32);
      assert!(!token_matches(&token, &"cd".repeat(32)));
      assert!(!token_matches(&token, ""));
      assert!(!token_matches(&token, "ab"));
   }

   #[test]
   fn write_connector_session_is_user_readable_only() {
      let dir = tempfile::tempdir().unwrap();
      let path = dir.path().join("connector.json");
      let session = ConnectorSession {
         token: "ab".repeat(32),
         port: 65534,
      };
      write_connector_session(&path, &session).unwrap();

      let raw = fs::read_to_string(&path).unwrap();
      let loaded: ConnectorSession = serde_json::from_str(&raw).unwrap();
      assert_eq!(loaded, session);

      #[cfg(unix)]
      {
         use std::os::unix::fs::PermissionsExt;
         let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
         assert_eq!(mode, 0o600);
      }
   }

   #[test]
   fn parse_dapp_origin_accepts_http_origins() {
      assert_eq!(
         parse_dapp_origin("https://app.uniswap.org").unwrap(),
         "https://app.uniswap.org"
      );
      assert_eq!(
         parse_dapp_origin("https://app.uniswap.org/").unwrap(),
         "https://app.uniswap.org"
      );
      assert_eq!(
         parse_dapp_origin("http://localhost:3000").unwrap(),
         "http://localhost:3000"
      );
   }

   #[test]
   fn parse_dapp_origin_rejects_spoofable_or_empty() {
      assert!(parse_dapp_origin("").is_err());
      assert!(parse_dapp_origin("null").is_err());
      assert!(parse_dapp_origin("https://app.uniswap.org/swap").is_err());
      assert!(parse_dapp_origin("javascript:alert(1)").is_err());
      assert!(parse_dapp_origin("file:///etc/passwd").is_err());
      assert!(parse_dapp_origin("chrome-extension://abc").is_err());
   }

   #[test]
   fn native_frame_roundtrip() {
      let payload = br#"{"token":"aa","port":1}"#;
      let framed = encode_native_frame(payload);
      assert_eq!(
         u32::from_le_bytes(framed[..4].try_into().unwrap()) as usize,
         payload.len()
      );
      assert_eq!(decode_native_frame(&framed).unwrap(), payload);
   }
}
