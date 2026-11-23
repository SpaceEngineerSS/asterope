// Builds the emulated image and runs it under QEMU.
//
//   node run.mjs
//
// Needs the thumbv7em-none-eabihf Rust target and qemu-system-arm on the
// path (or its location in QEMU). `-icount shift=0` makes the emulator's
// clock advance by one nanosecond an instruction, which is what lets the
// image count instructions with the processor's own timer.
import { execFileSync, spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const image = path.join(here, 'target/thumbv7em-none-eabihf/release/asterope-emulated');
const qemu = process.env.QEMU ?? 'qemu-system-arm';

execFileSync('cargo', ['build', '--release'], { cwd: here, stdio: 'inherit' });
const run = spawnSync(
  qemu,
  ['-M', 'mps2-an500', '-nographic', '-semihosting-config', 'enable=on,target=native', '-icount', 'shift=0', '-kernel', image],
  { encoding: 'utf8', timeout: 600_000 },
);
if (run.error) {
  console.error(`could not run ${qemu}: ${run.error.message}`);
  process.exit(2);
}
process.stdout.write(run.stdout);
process.stderr.write(run.stderr);

const frames = [...run.stdout.matchAll(/frame (\d+): (\w+), bits equal: (\w+), instructions: (\d+), heap: (\d+) bytes, stack: (\d+) bytes/g)];
if (frames.length > 0) {
  console.log('\n| Frame | Event | Same bits as the build machine | Instructions | Heap in use | Stack in use |');
  console.log('|---|---|---|---|---|---|');
  for (const [, index, event, equal, instructions, heap, stack] of frames) {
    const millions = (Number(instructions) / 1e6).toFixed(1);
    console.log(`| ${index} | ${event} | ${equal === 'true' ? 'yes' : 'no'} | ${millions} million | ${(Number(heap) / 1024).toFixed(0)} kB | ${(Number(stack) / 1024).toFixed(1)} kB |`);
  }
}
process.exit(run.status ?? 1);
