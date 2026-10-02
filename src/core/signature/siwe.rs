//! EIP-4361 "Sign-In With Ethereum" message detection.
//!
//! A SIWE message is signed over EIP-191 exactly like any other `personal_sign`
//! payload, so the only way to tell it apart is the message itself: a
//! `<domain> wants you to sign in with your Ethereum account:` header, the
//! account on the next line, and the required `URI` / `Version` / `Chain ID` /
//! `Nonce` / `Issued At` fields. Detection is strict about that skeleton and
//! lenient about everything else (statement, optional fields, field order), so
//! a non-conforming message degrades to a plain sign-message prompt rather than
//! being silently labelled a sign-in.

use std::str::FromStr;
use zeus_eth::alloy_primitives::Address;

/// The fixed suffix that introduces the `<domain>` on the first line.
const ACCOUNT_SUFFIX: &str = " wants you to sign in with your Ethereum account:";

/// Fields EIP-4361 requires after the account line.
const REQUIRED_FIELDS: [&str; 5] = ["URI: ", "Version: ", "Chain ID: ", "Nonce: ", "Issued At: "];

/// Whether `text` is an EIP-4361 Sign-In With Ethereum message.
pub fn is_siwe(text: &str) -> bool {
   let mut lines = text.split('\n').map(|line| line.trim_end_matches('\r'));

   let Some(header) = lines.next() else {
      return false;
   };

   let Some(domain) = header.strip_suffix(ACCOUNT_SUFFIX) else {
      return false;
   };

   if domain.is_empty() {
      return false;
   }

   // The account the message is signed for.
   let Some(address) = lines.next() else {
      return false;
   };

   if Address::from_str(address.trim()).is_err() {
      return false;
   }

   // A blank line, then the optional statement, then the fields.
   if lines.next() != Some("") {
      return false;
   }

   let mut seen = [false; REQUIRED_FIELDS.len()];

   for line in lines {
      for (index, field) in REQUIRED_FIELDS.iter().enumerate() {
         if line.starts_with(field) {
            seen[index] = true;
         }
      }
   }

   seen.iter().all(|found| *found)
}

#[cfg(test)]
mod tests {
   use super::is_siwe;

   const SIWE: &str = "https://example.com wants you to sign in with your Ethereum account:\n\
                       0x6fF5693b99212Da76ad316178A184AB56D299b43\n\n\
                       Sign in to authenticate your wallet.\n\n\
                       URI: https://example.com/\n\
                       Version: 1\n\
                       Chain ID: 1\n\
                       Nonce: 32891756\n\
                       Issued At: 2021-09-30T16:25:24Z";

   #[test]
   fn accepts_lowercase_account() {
      // `Address::from_str` checks length and hex, not the EIP-55 checksum, so
      // a dapp that lowercases the account line still reads as SIWE.
      let lower = SIWE.replace(
         "0x6fF5693b99212Da76ad316178A184AB56D299b43",
         "0x6ff5693b99212da76ad316178a184ab56d299b43",
      );
      assert!(is_siwe(&lower));
   }

   #[test]
   fn detects_siwe() {
      assert!(is_siwe(SIWE));
   }

   #[test]
   fn accepts_crlf() {
      assert!(is_siwe(&SIWE.replace('\n', "\r\n")));
   }

   #[test]
   fn rejects_plain_message() {
      assert!(!is_siwe("Sign in to prove you own this wallet."));
      assert!(!is_siwe(""));
   }

   #[test]
   fn rejects_siwe_without_required_fields() {
      let text = "https://example.com wants you to sign in with your Ethereum account:\n\
                  0x6fF5693b99212Da76ad316178A184AB56D299b43\n\n\
                  Sign in.\n\n\
                  URI: https://example.com/\nNonce: 32891756\n";
      assert!(!is_siwe(text));
   }

   #[test]
   fn rejects_header_without_account() {
      let text = "https://example.com wants you to sign in with your Ethereum account:\n\n\
                  URI: https://example.com/\nVersion: 1\nChain ID: 1\nNonce: 1\nIssued At: now";
      assert!(!is_siwe(text));
   }
}
