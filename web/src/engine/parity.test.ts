import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import { initSync, parityDigest } from '../wasm/asterope_wasm.js';

const wasmPath = fileURLToPath(new URL('../wasm/asterope_wasm_bg.wasm', import.meta.url));
const recordedPath = fileURLToPath(new URL('../../../data/parity.txt', import.meta.url));

describe('WebAssembly build', () => {
  it('reproduces the native frame and solution bit for bit', () => {
    initSync({ module: readFileSync(wasmPath) });
    expect(parityDigest()).toBe(readFileSync(recordedPath, 'utf8').trim());
  });
});
