//! Transaction request creation, encryption, sending, and response handling over Waku.
//! Port of transact/ logic + BroadcasterTransaction.

pub struct BroadcasterTransactResponse;

pub struct BroadcasterTransaction {
    // internal encrypted data etc.
}

impl BroadcasterTransaction {
    pub async fn create(/* txid_version, to, data, broadcaster_railgun_addr, fees_id, chain, nullifiers, ... */) -> anyhow::Result<Self> {
        // TODO:
        // 1. generate responseKey (16 random bytes)
        // 2. derive sharedKey (random priv + broadcaster pubkey)
        // 3. encrypt the raw params
        // 4. build message {method: "transact", params: {pubkey, encryptedData}}
        todo!("BroadcasterTransaction encryption + build (see TS broadcaster-transaction.ts)")
    }

    pub async fn send(self) -> anyhow::Result<BroadcasterTransactResponse> {
        // publish on transact topic via waku
        // setup listener for matching responseKey on response topic
        // decrypt + return
        todo!("send + poll for response")
    }
}
