/**
 * Zeus Railgun Prover Sidecar
 *
 * Minimal Node.js process that handles:
 *   - Downloading Railgun proving artifacts from IPFS (same sources as official SDK)
 *   - Running snarkjs.groth16.fullProve (or native-prover)
 *
 * Communicates with Rust over line-delimited JSON on stdio.
 *
 * All Railgun witness construction and business logic lives in Rust (zeus-railgun + zeus-railgun-prover).
 *
 * This is the direct equivalent of the Waku sidecar pattern.
 */

import snarkjs from 'snarkjs';
import axios from 'axios';
import brotliDecompress from 'brotli/decompress';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const IPFS_GATEWAY = 'https://ipfs-lb.com';
const MASTER_IPFS_HASH = 'QmUsmnK4PFc7zDp2cmC4wBZxYLjNyRgWfs5GNcJJ2uLcpU';

function send(obj) {
  process.stdout.write(JSON.stringify(obj) + '\n');
}

function sendEvent(type, data) {
  send({ type, ...data });
}

function log(...args) {
  console.error('[prover-sidecar]', ...args);
}

let artifactsCache = new Map(); // circuitVariant -> { zkey, vkey, wasm }

async function downloadArtifact(artifactName, variant) {
  // artifactName: 'zkey' | 'wasm' | 'vkey'
  const base = `artifacts-v2.1/${variant}`;
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
      throw new Error('Unknown artifact');
  }

  const url = `${IPFS_GATEWAY}/ipfs/${MASTER_IPFS_HASH}/${ipfsPath}`;
  log(`Downloading ${artifactName} for ${variant} from ${url}`);

  const { data } = await axios.get(url, { responseType: 'arraybuffer' });

  if (artifactName === 'vkey') {
    return JSON.parse(data.toString());
  }

  // Decompress brotli
  const decompressed = brotliDecompress(Buffer.from(data));
  return decompressed;
}

async function getArtifacts(circuitVariant) {
  if (artifactsCache.has(circuitVariant)) {
    return artifactsCache.get(circuitVariant);
  }

  const [zkey, wasm, vkey] = await Promise.all([
    downloadArtifact('zkey', circuitVariant),
    downloadArtifact('wasm', circuitVariant),
    downloadArtifact('vkey', circuitVariant),
  ]);

  const artifacts = { zkey, wasm, vkey };
  artifactsCache.set(circuitVariant, artifacts);
  return artifacts;
}

async function handleProve(params) {
  const { id, params: { witness, circuit_variant } } = params;

  try {
    log(`Generating proof for circuit ${circuit_variant}...`);

    const artifacts = await getArtifacts(circuit_variant);

    // snarkjs expects the witness as a plain object with bigint values
    // The Rust side will send properly formatted data.
    const { proof, publicSignals } = await snarkjs.groth16.fullProve(
      witness,
      artifacts.wasm,
      artifacts.zkey
    );

    // Convert to the format Railgun expects (pi_a / pi_b / pi_c)
    const snarkProof = {
      pi_a: [proof.pi_a[0], proof.pi_a[1]],
      pi_b: [
        [proof.pi_b[0][1], proof.pi_b[0][0]],
        [proof.pi_b[1][1], proof.pi_b[1][0]],
      ],
      pi_c: [proof.pi_c[0], proof.pi_c[1]],
    };

    log('Proof generated successfully');

    send({
      id,
      type: 'proof_generated',
      success: true,
      proof: snarkProof,
    });
  } catch (err) {
    log('Proof generation failed:', err.message);
    send({
      id,
      type: 'proof_generated',
      success: false,
      error: err.message || String(err),
    });
  }
}

async function main() {
  log('Railgun prover sidecar started');

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
