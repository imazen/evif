// Run the same WASI benchmark module in V8. Hashing is timed inside the module;
// loading, compilation, and this JavaScript wrapper are outside the timed calls.
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { WASI } from 'node:wasi';

const [modulePath, artifacts, output] = process.argv.slice(2);
if (!modulePath || !artifacts || !output || output.includes('/')) {
  throw new Error('usage: node run-wasi.mjs module.wasm artifact-directory output.json');
}
const wasi = new WASI({
  version: 'preview1',
  args: ['hash-cost', `/artifacts/${output}`, '--portable-only'],
  env: { TMPDIR: '/artifacts', ZENBENCH_NO_BUSY_GATE: '1' },
  preopens: { '/artifacts': resolve(artifacts) },
});
const module = await WebAssembly.compile(await readFile(modulePath));
const instance = await WebAssembly.instantiate(module, wasi.getImportObject());
process.exitCode = wasi.start(instance);
