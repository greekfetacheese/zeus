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
import * as wakuInterfaces from '@waku/interfaces';
const Protocols = wakuInterfaces.Protocols || { Store: 'store' };

const RAILGUN_CLUSTER_ID = 5;
const RAILGUN_SHARD_ID = 1;

// Default Railgun pubsub topic (sharded)
const DEFAULT_PUBSUB_TOPIC = `/waku/2/rs/${RAILGUN_CLUSTER_ID}/${RAILGUN_SHARD_ID}`;

// ENR tree used by Railgun (from their constants)
const RAILGUN_ENR_TREE = 'enrtree://APMYHUVNQWHJNPI5L2KQ765EMCKUAMRWPUH3U2QIKPK6XEV3OW442@discovery.rootedinprivacy.com';

// Known Railgun relays (from waku-broadcaster-client constants - WEB peers)
// These are the most reliable way to get initial connections on the Railgun fleet.
const RAILGUN_KNOWN_PEERS = [
  // WSS (current)
  "/dns4/relay-a.rootedinprivacy.com/tcp/8000/wss/p2p/16Uiu2HAmFbD2ZvAFi2j9jjDo6g4HFbQAhfjDfnTTrbyRGQRmtG7x",
  "/dns4/relay-b.rootedinprivacy.com/tcp/8000/wss/p2p/16Uiu2HAmPtEAoPPok7VLrpNNC6t92ZQFqLndHvkdx6Fk3CxA4MaG",
  "/dns4/client-edge.rootedinprivacy.com/tcp/8000/wss/p2p/16Uiu2HAmQdCGG5qREQCq96kucmpUVupmvLwrTRjMazPAaMTNP97A",
  // TCP variants (often better for Store on node)
  "/dns4/relay-a.rootedinprivacy.com/tcp/30304/p2p/16Uiu2HAmFbD2ZvAFi2j9jjDo6g4HFbQAhfjDfnTTrbyRGQRmtG7x",
  "/dns4/relay-b.rootedinprivacy.com/tcp/30304/p2p/16Uiu2HAmPtEAoPPok7VLrpNNC6t92ZQFqLndHvkdx6Fk3CxA4MaG",
  "/dns4/client-edge.rootedinprivacy.com/tcp/30304/p2p/16Uiu2HAmQdCGG5qREQCq96kucmpUVupmvLwrTRjMazPAaMTNP97A",
];

// Simple line-based JSON protocol
function send(obj) {
  try {
    process.stdout.write(JSON.stringify(obj) + '\n');
  } catch (e) {
    if (e.code === 'EPIPE' || e.errno === -32) {
      // Parent process closed the pipe (e.g. Ctrl+C in Rust)
      process.exit(0);
    }
    // ignore other write errors during shutdown
  }
}

function sendEvent(type, data) {
  send({ type, ...data });
}

function log(...args) {
  console.error('[sidecar]', ...args);
}

let waku = null;
let currentSubscriptions = new Map();
let goodStorePeers = new Set(); // peers that successfully delivered messages


async function getStorePeers() {
  if (!waku || !waku.store || !waku.store.peerManager) {
    return [];
  }
  try {
    const proto = (Protocols && Protocols.Store) || 'store';
    const peers = await waku.store.peerManager.getPeers({ protocol: proto });
    return peers || [];
  } catch (e) {
    // Only log the first few times to reduce noise
    if (!global.storePeerErrorLogged) {
      log('Error getting store peers:', e.message);
      global.storePeerErrorLogged = true;
    }
    return [];
  }
}

async function getConnectedPeerIds() {
  if (!waku || !waku.libp2p || !waku.libp2p.getConnections) return [];
  try {
    return waku.libp2p.getConnections().map(c => {
      const pid = c.remotePeer ? c.remotePeer.toString() : 'unknown';
      return pid;
    });
  } catch { return []; }
}



async function ensureStrongConnectivity() {
  if (!waku) return 0;
  
  // Re-dial all known Railgun relays (helps when initial dials were partial)
  let dialed = 0;
  for (const peer of RAILGUN_KNOWN_PEERS) {
    try {
      await waku.dial(peer);
      dialed++;
    } catch (e) {
      // normal - many will fail
    }
  }
  
  // Give discovery a moment
  await new Promise(r => setTimeout(r, 3000));
  
  const connected = await getConnectedPeerIds();
  log('ensureStrongConnectivity: dialed known relays + now have', connected.length, 'connections');
  return connected.length;
}

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
    // Pass the raw enrtree:// URL strings directly (this was the working pattern)
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
      // Explicit store peers - critical for historical queries (matches official client)
      store: {
        peers: bootstrapPeers,
      },
    });

    await waku.start();

    log('Waku light node created and started, using', bootstrapPeers.length, 'known bootstrap peers + DNS/PeerExchange');
    log('Store configured with explicit peers for historical queries (matching official Railgun client)');

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

    // Listen for store connect events for diagnostics
    try {
      if (waku.store && waku.store.peerManager && waku.store.peerManager.events) {
        waku.store.peerManager.events.addEventListener('store:connect', (ev) => {
          const peer = ev?.detail?.peerId?.toString?.() || ev?.detail || 'unknown';
          log('🟢 Store peer connected event received:', peer);
        });
        log('Subscribed to store:connect events');
      }
    } catch (e) {
      log('Could not subscribe to store events:', e.message);
    }

    // Wait for connectivity (general)
    await waitForRemotePeer(waku, ['lightpush', 'filter'], 30000).catch(() => {
      log('waitForRemotePeer (filter/lightpush) timed out');
    });

    // Wait for all protocols like the official Railgun client does
    // (Filter + LightPush + Store)
    log('Waiting for remote peers (Filter + LightPush + Store) like official client...');
    if (typeof waku.waitForPeers === 'function') {
      try {
        await waku.waitForPeers(['filter', 'lightpush', 'store'], 120000);
        log('waitForPeers([filter, lightpush, store]) completed');
      } catch (e) {
        log('waitForPeers for full set timed out or failed:', e.message);
      }
    }

    // Dedicated store wait as backup
    await waitForRemotePeer(waku, ['store'], 120000).catch(() => {
      log('Dedicated waitForRemotePeer(store) timed out');
    });

    const peerId = waku.libp2p.peerId.toString();
    log('Waku light node started', peerId);

    // Diagnostics for Store
    const storePeersAfterStart = await getStorePeers();
    log('Store peers after waits (from peerManager):', storePeersAfterStart.length, storePeersAfterStart);
    const connected = await getConnectedPeerIds();
    log('Total connected peerIds:', connected.length, connected);
    if (storePeersAfterStart.length === 0) {
      log('Note: peerManager still reports 0 store peers (we will try multiple connected peers on queries)');
    }

    // Final boost before telling Rust we're ready
    await ensureStrongConnectivity();

    send({ id: params.id, type: 'started', success: true, peerId });

    startPeerReporter();

    return true;
  } catch (err) {
    log('Failed to start Waku node:', err);
    try { send({ id: params.id, type: 'started', success: false, error: err.message || String(err) }); } catch (_) {}
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
async function ensureConnectedToKnownPeers() {
  if (!waku) return;
  const toDial = [...RAILGUN_KNOWN_PEERS];
  for (const peer of toDial) {
    try {
      await waku.dial(peer).catch(() => {});
    } catch (_) {}
  }
  // Also try to get more via peer exchange if possible
  try {
    if (waku.libp2p && waku.libp2p.services && waku.libp2p.services.peerExchange) {
      // trigger if available
    }
  } catch (_) {}
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


async function handleQueryHistorical(params) {
  const { contentTopics = [], timeStartMs, timeEndMs, pageSize = 100 } = params;

  if (!waku || !waku.store) {
    return send({ id: params.id, type: 'historical_queried', success: false, error: 'not_started_or_no_store' });
  }

  if (!Array.isArray(contentTopics) || contentTopics.length === 0) {
    return send({ id: params.id, type: 'historical_queried', success: false, error: 'no contentTopics' });
  }

  let total = 0;

  try {
    const now = Date.now();
    const defaultLookbackMs = 1000 * 60 * 5; // 5 min default (fees republish often)
    const startTime = timeStartMs ? new Date(timeStartMs) : new Date(now - defaultLookbackMs);
    const endTime = timeEndMs ? new Date(timeEndMs) : new Date(now);

    log(`Querying historical on ${contentTopics.length} topics, range: ${startTime.toISOString()} -> ${endTime.toISOString()}`);

    // === NEW: aggressively ensure we have good connectivity before querying ===
    const connCount = await ensureStrongConnectivity();
    if (connCount < 4) {
      log('Low connectivity before query, waiting a bit more...');
      await new Promise(r => setTimeout(r, 8000));
    }

    // Collect candidate peers (prefer good ones we have seen work before)
    let candidates = [];
    const goodOnes = Array.from(goodStorePeers);
    if (goodOnes.length > 0) candidates.push(...goodOnes);

    const storeP = await getStorePeers();
    candidates.push(...storeP);

    const connected = await getConnectedPeerIds();
    candidates.push(...connected);

    // Dedup + shuffle for better chance
    candidates = [...new Set(candidates)].sort(() => Math.random() - 0.5);

    if (candidates.length === 0) {
      log('No candidate peers at all — will still try default query');
      candidates = [null];
    }

    log(`Trying historical query with ${candidates.length} candidate peer(s) (some may be duplicates of connected)`);

    let successWithPeer = false;

    for (const candidate of candidates.slice(0, 6)) {  // try up to 6 different peers
      if (successWithPeer) break;

      const forcedPeerId = candidate;
      if (forcedPeerId) {
        log(`  → Trying peer: ${forcedPeerId}`);
      } else {
        log('  → Trying without explicit peerId (default behavior)');
      }

      // Small wait for store before each peer attempt
      await waitForRemotePeer(waku, ['store'], 8000).catch(() => {});

      for (const contentTopic of contentTopics) {
        const decoder = createDecoder(contentTopic, DEFAULT_PUBSUB_TOPIC);

        const queryOpts = {
          pubsubTopic: DEFAULT_PUBSUB_TOPIC,
          contentTopics: [contentTopic],
          timeStart: startTime,
          timeEnd: endTime,
          pageSize,
        };
        if (forcedPeerId) {
          queryOpts.peerId = forcedPeerId;
        }

        let thisPeerCount = 0;
        try {
          for await (const page of waku.store.queryGenerator([decoder], queryOpts)) {
            for (const msgPromise of page) {
              const msg = await msgPromise;
              if (msg && msg.payload) {
                const payload = Buffer.from(msg.payload).toString('base64');
                sendEvent('message', {
                  contentTopic,
                  payload,
                  timestamp: msg.timestamp ? msg.timestamp.getTime() : Date.now(),
                  pubsubTopic: DEFAULT_PUBSUB_TOPIC,
                  source: 'historical',
                });
                thisPeerCount++;
                total++;
              }
              if (thisPeerCount > 30) break; // per-peer cap
            }
          }

          if (thisPeerCount > 0) {
            log(`    ✓ Got ${thisPeerCount} messages from peer ${forcedPeerId || 'default'}`);
            if (forcedPeerId) goodStorePeers.add(forcedPeerId);
            successWithPeer = true;
          }
        } catch (e) {
          const m = e.message || String(e);
          if (!m.includes('No peers')) {
            log(`    peer ${forcedPeerId || 'default'} error: ${m}`);
          }
        }
      }
    }

    log('Historical query complete. Delivered', total, 'messages total');
    send({ id: params.id, type: 'historical_queried', success: true, count: total });
  } catch (err) {
    log('Historical query failed:', err);
    send({ id: params.id, type: 'historical_queried', success: false, error: err.message || String(err) });
  }
}async function handleGetStatus() {
  if (!waku || !waku.libp2p) {
    return send({ type: 'status', started: false, meshPeers: 0, pubsubPeers: 0, storePeers: 0 });
  }

  const connections = waku.libp2p.getConnections ? waku.libp2p.getConnections().length : 0;
  const storePeersList = await getStorePeers();
  const storeCount = storePeersList.length;

  send({
    type: 'status',
    started: true,
    meshPeers: connections,
    pubsubPeers: connections,
    storePeers: storeCount,
  });

  if (storeCount > 0) {
    log('Store peers available:', storeCount);
  }
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
    case 'query_historical':
      await handleQueryHistorical(params);
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
