/**
 * Zeus Waku Broadcaster Sidecar
 * 
 * Minimal Node.js process that only handles Waku networking.
 * Communicates with the Rust Zeus wallet over line-delimited JSON on stdio.
 * 
 * All Railgun business logic lives in Rust.
 * 
 * Uses the same discovery mechanisms as the official @railgun-community/waku-broadcaster-client
 * (DNS ENR tree + peer exchange) + hardcoded known Railgun relays for faster bootstrap.
 */

import { createLightNode, createEncoder, createDecoder } from '@waku/sdk';
import { waitForRemotePeer } from '@waku/sdk';
import { wakuDnsDiscovery, wakuPeerExchangeDiscovery } from '@waku/discovery';

const RAILGUN_CLUSTER_ID = 5;
const RAILGUN_SHARD_ID = 1;

// Default Railgun pubsub topic (sharded)
const DEFAULT_PUBSUB_TOPIC = `/waku/2/rs/${RAILGUN_CLUSTER_ID}/${RAILGUN_SHARD_ID}`;

// ENR tree used by Railgun (from their constants)
const RAILGUN_ENR_TREE = 'enrtree://APMYHUVNQWHJNPI5L2KQ765EMCKUAMRWPUH3U2QIKPK6XEV3OW442@discovery.rootedinprivacy.com';

// Known Railgun relays (from waku-broadcaster-client constants - WEB peers)
// These are the most reliable way to get initial connections on the Railgun fleet.
const RAILGUN_KNOWN_PEERS = [
  "/dns4/relay-a.rootedinprivacy.com/tcp/8000/wss/p2p/16Uiu2HAmFbD2ZvAFi2j9jjDo6g4HFbQAhfjDfnTTrbyRGQRmtG7x",
  "/dns4/relay-b.rootedinprivacy.com/tcp/8000/wss/p2p/16Uiu2HAmPtEAoPPok7VLrpNNC6t92ZQFqLndHvkdx6Fk3CxA4MaG",
  "/dns4/client-edge.rootedinprivacy.com/tcp/8000/wss/p2p/16Uiu2HAmQdCGG5qREQCq96kucmpUVupmvLwrTRjMazPAaMTNP97A",
];

// Simple line-based JSON protocol
function send(obj) {
  process.stdout.write(JSON.stringify(obj) + '\n');
}

function sendEvent(type, data) {
  send({ type, ...data });
}

function log(...args) {
  console.error('[sidecar]', ...args);
}

let waku = null;
let currentSubscriptions = new Map();

async function handleStart(params) {
  const { chain, options = {} } = params;

  if (waku) {
    try { await waku.stop(); } catch (_) {}
    waku = null;
  }

  const networkConfig = {
    clusterId: RAILGUN_CLUSTER_ID,
    shards: [RAILGUN_SHARD_ID],
  };

  // Combine known Railgun relays + any additional from Rust side
  const additionalPeers = options.additionalDirectPeers || [];
  const bootstrapPeers = [...RAILGUN_KNOWN_PEERS, ...additionalPeers];

  try {
    // Use the same discovery as the official Railgun broadcaster client
    const enrTrees = [RAILGUN_ENR_TREE];

    waku = await createLightNode({
      networkConfig,
      libp2p: {
        peerDiscovery: [
          wakuDnsDiscovery(enrTrees),
          wakuPeerExchangeDiscovery(),
        ],
      },
      autoStart: false,
      defaultBootstrap: true,
      bootstrapPeers,
    });

    await waku.start();

    log('Waku light node created and started, using', bootstrapPeers.length, 'known bootstrap peers + DNS/PeerExchange');

    // Dial the known Railgun relays explicitly using the high-level Waku dial (helps bootstrap)
    for (const peer of bootstrapPeers) {
      try {
        // Use waku.dial (string multiaddr works)
        await waku.dial(peer);
        log('Successfully dialed bootstrap peer via waku.dial', peer);
      } catch (e) {
        log('Dial bootstrap peer failed (this is often normal):', peer, e.message);
      }
    }

    // Wait for connectivity (this often times out on first cold start for custom fleets)
    await waitForRemotePeer(waku, ['lightpush', 'filter', 'store'], 30000).catch(() => {
      log('waitForRemotePeer timed out (will continue with background discovery + known peers)');
    });

    const peerId = waku.libp2p.peerId.toString();
    log('Waku light node started', peerId);

    send({ id: params.id, type: 'started', success: true, peerId });

    startPeerReporter();

    return true;
  } catch (err) {
    log('Failed to start Waku node:', err);
    send({ id: params.id, type: 'started', success: false, error: err.message || String(err) });
    return false;
  }
}

function startPeerReporter() {
  setInterval(() => {
    if (!waku || !waku.libp2p) return;
    try {
      const connections = waku.libp2p.getConnections ? waku.libp2p.getConnections() : [];
      const count = connections.length;
      sendEvent('peer_update', {
        mesh: count,
        pubsub: count,
      });
      if (count > 0) {
        log('Peer count update:', count, 'connections');
      }
    } catch (e) {
      // ignore transient errors
    }
  }, 10000);
}

async function handleSubscribe(params) {
  const contentTopics = params.contentTopics || params.content_topics || [];
  if (!Array.isArray(contentTopics) || contentTopics.length === 0) {
    return send({ id: params.id, type: 'subscribed', success: false, error: 'no contentTopics provided' });
  }

  if (!waku) {
    return send({ id: params.id, type: 'subscribed', success: false, error: 'not_started' });
  }

  let successCount = 0;

  for (const contentTopic of contentTopics) {
    if (currentSubscriptions.has(contentTopic)) {
      log('Already subscribed to', contentTopic);
      successCount++;
      continue;
    }

    try {
      const decoder = createDecoder(contentTopic, DEFAULT_PUBSUB_TOPIC);

      const subscription = await waku.filter.subscribe([decoder], (message) => {
        try {
          const payload = message.payload ? Buffer.from(message.payload).toString('base64') : '';
          const preview = payload.length > 100 ? payload.substring(0, 100) + '...' : payload;
          log(`📨 WAKU MESSAGE on ${contentTopic} | len=${payload.length} | preview=${preview}`);

          sendEvent('message', {
            contentTopic,
            payload,
            timestamp: message.timestamp ? message.timestamp.getTime() : Date.now(),
            pubsubTopic: DEFAULT_PUBSUB_TOPIC,
          });
        } catch (e) {
          log('Error processing incoming message', e);
        }
      });

      currentSubscriptions.set(contentTopic, { decoder, subscription });
      log('Subscribed to', contentTopic);
      successCount++;
    } catch (err) {
      log('Subscribe failed for', contentTopic, err.message);
      send({ id: params.id, type: 'subscribed', success: false, error: err.message, contentTopic });
      return;
    }
  }

  send({ id: params.id, type: 'subscribed', success: true, contentTopics });
}

async function handlePublish(params) {
  const { contentTopic, payload, pubsubTopic = DEFAULT_PUBSUB_TOPIC } = params;

  if (!waku) {
    return send({ id: params.id, type: 'published', success: false, error: 'not_started' });
  }

  try {
    const encoder = createEncoder({ contentTopic, pubsubTopic: pubsubTopic || DEFAULT_PUBSUB_TOPIC });
    const bytes = Buffer.from(payload, 'base64');

    const result = await waku.lightPush.send(encoder, { payload: bytes });

    if (result.successes && result.successes.length > 0) {
      send({ id: params.id, type: 'published', success: true });
    } else {
      const err = (result.failures && result.failures[0] && result.failures[0].error) || 'unknown';
      send({ id: params.id, type: 'published', success: false, error: String(err) });
    }
  } catch (err) {
    log('Publish failed', err);
    send({ id: params.id, type: 'published', success: false, error: err.message || String(err) });
  }
}

async function handleGetStatus() {
  if (!waku || !waku.libp2p) {
    return send({ type: 'status', started: false, meshPeers: 0, pubsubPeers: 0 });
  }

  const connections = waku.libp2p.getConnections ? waku.libp2p.getConnections().length : 0;

  send({
    type: 'status',
    started: true,
    meshPeers: connections,
    pubsubPeers: connections,
  });
}

async function handleCommand(line) {
  let msg;
  try {
    msg = JSON.parse(line);
  } catch (e) {
    return sendEvent('error', { message: 'invalid_json' });
  }

  const { cmd, id, params = {} } = msg;
  params.id = id;

  switch (cmd) {
    case 'start':
      await handleStart(params);
      break;
    case 'subscribe':
      await handleSubscribe(params);
      break;
    case 'publish':
      await handlePublish(params);
      break;
    case 'get_status':
      await handleGetStatus();
      break;
    case 'stop':
      if (waku) {
        try { await waku.stop(); } catch (_) {}
        waku = null;
      }
      currentSubscriptions.clear();
      send({ id, type: 'stopped' });
      process.exit(0);
      break;
    default:
      send({ id, type: 'error', error: `unknown_cmd: ${cmd}` });
  }
}

// Main loop - read from stdin
process.stdin.setEncoding('utf8');

let buffer = '';
process.stdin.on('data', (chunk) => {
  buffer += chunk;
  let lines = buffer.split('\n');
  buffer = lines.pop();

  for (const line of lines) {
    if (line.trim()) {
      handleCommand(line.trim()).catch(err => {
        log('Command handler error', err);
        sendEvent('error', { message: err.message || String(err) });
      });
    }
  }
});

process.on('SIGINT', async () => {
  if (waku) { try { await waku.stop(); } catch (_) {} }
  process.exit(0);
});

process.on('SIGTERM', async () => {
  if (waku) { try { await waku.stop(); } catch (_) {} }
  process.exit(0);
});

log('Sidecar started. Waiting for commands on stdin...');
sendEvent('ready', { version: '0.1.0' });
