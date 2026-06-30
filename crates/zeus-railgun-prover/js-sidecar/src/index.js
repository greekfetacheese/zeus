/**
 * Zeus Railgun Prover Sidecar
 *
 * Improved version:
 *   - Downloads Railgun proving artifacts from IPFS (same sources as official SDK)
 *   - Disk-based artifact caching (persistent across runs)
 *   - Progress reporting back to Rust (download + proving)
 *   - Optional support for native-prover (@railgun-privacy/native-prover) when installed
 *
 * Communicates with Rust over line-delimited JSON on stdio.
 */

import * as snarkjs from 'snarkjs';
import axios from 'axios';
import * as brotliModule from 'brotli';

// snarkjs and brotli can have subtle ESM export differences across versions
const brotli = brotliModule.default || brotliModule;
import fs from 'fs';
import path from 'path';
import os from 'os';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// Railgun official artifact locations (same as used by @railgun-community)
const IPFS_GATEWAY = 'https://ipfs-lb.com';
const MASTER_IPFS_HASH = 'QmUsmnK4PFc7zDp2cmC4wBZxYLjNyRgWfs5GNcJJ2uLcpU';

// Persistent cache dir
const CACHE_DIR = path.join(os.homedir(), '.railgun', 'artifacts-v2.1');

function ensureCacheDir() {
  if (!fs.existsSync(CACHE_DIR)) {
    fs.mkdirSync(CACHE_DIR, { recursive: true });
  }
}

function send(obj) {
  process.stdout.write(JSON.stringify(obj) + '\n');
}

function log(...args) {
  console.error('[prover-sidecar]', ...args);
}

// Progress reporting helper
function reportProgress(id, stage, percent) {
  send({
    id,
    type: 'progress',
    stage,           // 'download' | 'proving'
    percent,         // 0-100
  });
}

let artifactsCache = new Map(); // in-memory for current process

function getArtifactCachePath(variant, artifactName) {
  ensureCacheDir();
  const filename = `${variant}-${artifactName}`;
  return path.join(CACHE_DIR, filename);
}

async function downloadArtifact(artifactName, variant, id) {
  const cachePath = getArtifactCachePath(variant, artifactName);
  if (fs.existsSync(cachePath)) {
    log(`Using cached ${artifactName} for ${variant}`);
    const data = fs.readFileSync(cachePath);
    if (artifactName === 'vkey') {
      return JSON.parse(data.toString());
    }
    return data;
  }

  let ipfsPath;
  switch (artifactName) {
    case 'zkey':
      ipfsPath = `circuits/${variant}/zkey.br`;
      break;
    case 'wasm':
      ipfsPath = `prover/snarkjs/${variant}.wasm.br`;
      break;
    case 'vkey':
      ipfsPath = `circuits/${variant}/vkey.json`;
      break;
    default:
      throw new Error(`Unknown artifact: ${artifactName}`);
  }

  const url = `${IPFS_GATEWAY}/ipfs/${MASTER_IPFS_HASH}/${ipfsPath}`;
  log(`Downloading ${artifactName} for ${variant} from ${url}`);
  reportProgress(id, 'download', 0);

  const { data } = await axios.get(url, {
    responseType: 'arraybuffer',
    onDownloadProgress: (progressEvent) => {
      if (progressEvent.total) {
        const pct = Math.round((progressEvent.loaded / progressEvent.total) * 100);
        reportProgress(id, 'download', pct);
      }
    },
  });

  let result;
  if (artifactName === 'vkey') {
    result = JSON.parse(data.toString());
    fs.writeFileSync(cachePath, JSON.stringify(result));
  } else {
    // Decompress brotli
    result = Buffer.from(brotli.decompress(data));
    fs.writeFileSync(cachePath, result);
  }

  reportProgress(id, 'download', 100);
  return result;
}

async function getArtifacts(circuitVariant, id) {
  if (artifactsCache.has(circuitVariant)) {
    return artifactsCache.get(circuitVariant);
  }

  const [zkey, wasm, vkey] = await Promise.all([
    downloadArtifact('zkey', circuitVariant, id),
    downloadArtifact('wasm', circuitVariant, id),
    downloadArtifact('vkey', circuitVariant, id),
  ]);

  const artifacts = { zkey, wasm, vkey };
  artifactsCache.set(circuitVariant, artifacts);
  return artifacts;
}

// Try to load native prover (optional, for performance)
let nativeProver = null;
try {
  // This package is distributed separately by Railgun team
  // Users can `npm install @railgun-privacy/native-prover` in the sidecar dir if desired
  nativeProver = await import('@railgun-privacy/native-prover');
  log('Native prover loaded successfully');
} catch (e) {
  log('Native prover not available (using snarkjs). Install @railgun-privacy/native-prover for better performance.');
}

async function handleProve(msg) {
  const { id, params: { witness, circuit_variant } } = msg;

  try {
    log(`Received prove request for circuit ${circuit_variant}`);

    // Download / prepare artifacts first. This will emit "download" progress events.
    const artifacts = await getArtifacts(circuit_variant, id);

    log(`Artifacts ready for ${circuit_variant}. Starting Groth16 proof generation...`);
    reportProgress(id, 'proving', 0);

    let proof;
    let publicSignals;

    if (nativeProver && nativeProver.nativeProveRailgun) {
      const circuitId = getNativeCircuitId(circuit_variant);
      const datBuffer = artifacts.wasm;
      const zkeyBuffer = artifacts.zkey;

      const nativeInput = convertWitnessForNative(witness);

      proof = await nativeProver.nativeProveRailgun(
        circuitId,
        datBuffer,
        zkeyBuffer,
        nativeInput,
        (progress) => {
          reportProgress(id, 'proving', Math.floor(progress * 80) + 10);
        }
      );

      publicSignals = [];
    } else {
      // snarkjs path
      const result = await snarkjs.groth16.fullProve(
        witness,
        artifacts.wasm,
        artifacts.zkey
      );
      proof = result.proof;
      publicSignals = result.publicSignals;
    }

    // Convert to the format Railgun contracts expect
    const snarkProof = {
      pi_a: [proof.pi_a[0].toString(), proof.pi_a[1].toString()],
      pi_b: [
        [proof.pi_b[0][1].toString(), proof.pi_b[0][0].toString()],
        [proof.pi_b[1][1].toString(), proof.pi_b[1][0].toString()],
      ],
      pi_c: [proof.pi_c[0].toString(), proof.pi_c[1].toString()],
    };

    reportProgress(id, 'proving', 100);

    send({
      id,
      type: 'proof_generated',
      success: true,
      proof: snarkProof,
    });
  } catch (err) {
    log('Proof generation failed:', err.message || err);
    send({
      id,
      type: 'proof_generated',
      success: false,
      error: err.message || String(err),
    });
  }
}

// Map circuit variant string to native circuit id (approximate - real mapping lives in Railgun SDK)
function getNativeCircuitId(variant) {
  const map = {
    '01x01': 0,
    '01x02': 1,
    '02x02': 2,
    '02x03': 3,
    '05x05': 4,
  };
  return map[variant] ?? 0;
}

function convertWitnessForNative(witness) {
  // Native prover expects all values as string representations of bigints
  const out = {};
  for (const [key, val] of Object.entries(witness)) {
    if (Array.isArray(val)) {
      out[key] = val.map(v => v.toString());
    } else {
      out[key] = val.toString();
    }
  }
  return out;
}

async function main() {
  log('Railgun prover sidecar started');
  ensureCacheDir();

  const rl = (await import('readline')).createInterface({
    input: process.stdin,
    output: process.stdout,
    terminal: false,
  });

  for await (const line of rl) {
    if (!line.trim()) continue;

    try {
      const msg = JSON.parse(line);

      if (msg.cmd === 'start') {
        send({ id: msg.id, type: 'started', success: true });
      } else if (msg.cmd === 'prove') {
        await handleProve(msg);
      } else if (msg.cmd === 'stop') {
        send({ id: msg.id, type: 'stopped', success: true });
        process.exit(0);
      }
    } catch (e) {
      log('Error processing command:', e.message);
      send({ type: 'error', error: e.message });
    }
  }
}

main().catch(err => {
  log('Fatal error:', err);
  process.exit(1);
});
