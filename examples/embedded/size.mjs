// Prints the section sizes of a 32-bit little-endian ELF file: node size.mjs <file>
import { readFileSync } from 'node:fs';

const file = process.argv[2];
if (!file) {
  console.error('usage: node size.mjs <elf file>');
  process.exit(1);
}
const bytes = readFileSync(file);
if (bytes.readUInt32BE(0) !== 0x7f454c46 || bytes[4] !== 1 || bytes[5] !== 1) {
  console.error('not a 32-bit little-endian ELF file');
  process.exit(1);
}
const sectionOffset = bytes.readUInt32LE(0x20);
const entrySize = bytes.readUInt16LE(0x2e);
const count = bytes.readUInt16LE(0x30);
const namesIndex = bytes.readUInt16LE(0x32);
const header = (index) => {
  const at = sectionOffset + index * entrySize;
  return {
    name: bytes.readUInt32LE(at),
    type: bytes.readUInt32LE(at + 4),
    flags: bytes.readUInt32LE(at + 8),
    offset: bytes.readUInt32LE(at + 16),
    size: bytes.readUInt32LE(at + 20),
  };
};
const names = header(namesIndex);
const nameOf = (offset) => {
  const start = names.offset + offset;
  return bytes.toString('latin1', start, bytes.indexOf(0, start));
};

const ALLOC = 2;
const WRITE = 1;
const NOBITS = 8;
const totals = { code: 0, constants: 0, data: 0, zeroed: 0 };
for (let i = 0; i < count; i += 1) {
  const section = header(i);
  if (!(section.flags & ALLOC) || section.size === 0) continue;
  const kind =
    section.type === NOBITS ? 'zeroed' : section.flags & WRITE ? 'data' : section.flags & 4 ? 'code' : 'constants';
  totals[kind] += section.size;
  console.log(`${nameOf(section.name).padEnd(18)} ${String(section.size).padStart(9)}  ${kind}`);
}
console.log('');
console.log(`code                ${String(totals.code).padStart(9)} bytes`);
console.log(`constants           ${String(totals.constants).padStart(9)} bytes (catalog, index, tables)`);
console.log(`flash               ${String(totals.code + totals.constants + totals.data).padStart(9)} bytes`);
console.log(`static RAM          ${String(totals.data + totals.zeroed).padStart(9)} bytes (includes the example heap)`);
