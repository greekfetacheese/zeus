# Zeus Waku Broadcaster Sidecar

This is a minimal Node.js process that handles **only** the Waku networking layer for the Railgun broadcaster client.

## Why a sidecar?

The official pure TypeScript Waku implementation (`@waku/sdk`) is excellent and well-maintained. The Rust bindings were FFI to a Nim implementation and proved very difficult to update.

Zeus (Rust) owns all the Railgun domain logic:
- Fee message parsing + signature verification
- Broadcaster selection
- Encryption (shared key + response key for transact)
- Transaction construction
- Caching and state

The sidecar is a "dumb pipe":
- Connects to Waku as a light client
- Subscribes to Railgun content topics
- Forwards incoming messages to Rust
- Sends messages when asked by Rust
- Reports peer counts and health

## Communication Protocol (Line-delimited JSON over stdio)

All messages are JSON followed by a newline (`\n`).

### From Rust → Sidecar (Commands)

```json
{ "id": 42, "cmd": "start", "params": { "chain": { "type": 0, "id": 1 }, "options": { ... } } }
{ "id": 43, "cmd": "subscribe", "params": { "contentTopics": ["/railgun/v2/0-1-fees/json"] } }
{ "id": 44, "cmd": "publish", "params": { "contentTopic": "...", "payload": "base64...", "pubsubTopic": "/waku/2/rs/5/1" } }
{ "id": 45, "cmd": "get_status" }
{ "id": 46, "cmd": "stop" }
```

### From Sidecar → Rust (Responses + Events)

**Responses** (have matching `id`):
```json
{ "id": 42, "type": "started", "success": true, "peerId": "..." }
{ "id": 45, "type": "status", "meshPeers": 4, "pubsubPeers": 12 }
```

**Events** (no `id`, or use a special one):
```json
{ "type": "message", "contentTopic": "/railgun/v2/0-1-fees/json", "payload": "base64...", "timestamp": 1730000000000 }
{ "type": "peer_update", "mesh": 5, "pubsub": 18 }
{ "type": "error", "message": "..." }
```

## Running

```bash
cd js-sidecar
npm install
npm start
```

The sidecar is designed to be spawned by the Rust `zeus-waku-broadcaster` crate.

## Railgun Configuration

The sidecar will be configured with Railgun-specific settings:
- Cluster 5, Shard 1
- ENR tree for discovery
- Specific content topics per chain

## Development

We keep the sidecar as small as possible. All complex logic lives in Rust.
