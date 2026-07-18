use alloy_sol_types::sol;

sol! {
    contract RailgunLegacy {
        event Nullifiers(
            uint256 treeNumber,
            uint256[] nullifier
        );

        event Transact(
            uint256 treeNumber,
            uint256 startPosition,
            bytes32[] hash,
            CommitmentCiphertext[] ciphertext
        );

          event Shield(
           uint256 treeNumber,
           uint256 startPosition,
           CommitmentPreimage[] commitments,
           ShieldCiphertext[] shieldCiphertext
         );

          event Unshield(address to, TokenDataLegacy token, uint256 amount, uint256 fee);

    // This is the same as railgun::CommitmentCiphertext, but sol! doesnt allow imprting it from outside
     struct CommitmentCiphertext {
           bytes32[4] ciphertext; // Ciphertext order: IV & tag (16 bytes each), encodedMPK (senderMPK XOR receiverMPK), random & amount (16 bytes each), token
           bytes32 blindedSenderViewingKey;
           bytes32 blindedReceiverViewingKey;
           bytes annotationData; // Only for sender to decrypt
           bytes memo; // Added to note ciphertext for decryption
        }

        event CommitmentBatch(
            uint256 treeNumber,
            uint256 startPosition,
            uint256[] hash,
            CommitmentCiphertextLegacy[] ciphertext
        );

        event GeneratedCommitmentBatch(
            uint256 treeNumber,
            uint256 startPosition,
            CommitmentPreimageLegacy[] commitments,
            uint256[2][] encryptedRandom
        );

        struct CommitmentPreimage {
            bytes32 npk; // Poseidon(Poseidon(spending public key, nullifying key), random)
            TokenDataLegacy token; // Token field
            uint120 value; // Note value
         }

         struct ShieldCiphertext {
            bytes32[3] encryptedBundle; // IV shared (16 bytes), tag (16 bytes), random (16 bytes), IV sender (16 bytes), receiver viewing public key (32 bytes)
            bytes32 shieldKey; // Public key to generate shared key from
        }

        struct CommitmentCiphertextLegacy {
            uint256[4] ciphertext;
            uint256[2] ephemeralKeys;
            uint256[] memo;
        }

        struct CommitmentPreimageLegacy {
            uint256 npk;
            TokenDataLegacy token;
            uint120 value;
        }

        struct TokenDataLegacy {
            uint8 tokenType;
            address tokenAddress;
            uint256 tokenSubID;
        }
    }
}

use crate::crypto::aes::Ciphertext;

impl From<RailgunLegacy::CommitmentCiphertext> for Ciphertext {
   fn from(c: RailgunLegacy::CommitmentCiphertext) -> Self {
      let iv = c.ciphertext[0][..16].try_into().unwrap();
      let tag = c.ciphertext[0][16..32].try_into().unwrap();

      let mut data: Vec<Vec<u8>> = c.ciphertext[1..].iter().map(|chunk| chunk.to_vec()).collect();
      data.push(c.memo.to_vec());

      Ciphertext { iv, tag, data }
   }
}

impl From<RailgunLegacy::ShieldCiphertext> for Ciphertext {
   fn from(c: RailgunLegacy::ShieldCiphertext) -> Self {
      let iv = c.encryptedBundle[0][..16].try_into().unwrap();
      let tag = c.encryptedBundle[0][16..].try_into().unwrap();
      let data = vec![c.encryptedBundle[1][..16].to_vec()];
      Ciphertext { iv, tag, data }
   }
}

impl From<RailgunLegacy::TokenDataLegacy> for crate::caip::AssetId {
   fn from(td: RailgunLegacy::TokenDataLegacy) -> Self {
      match td.tokenType {
         0 => crate::caip::AssetId::Erc20(td.tokenAddress),
         1 => crate::caip::AssetId::Erc721(td.tokenAddress, td.tokenSubID),
         2 => crate::caip::AssetId::Erc1155(td.tokenAddress, td.tokenSubID),
         _ => crate::caip::AssetId::Erc20(td.tokenAddress), // fallback
      }
   }
}
