import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import { STAR_NAMES, starLabel } from './star-names';

/** The project's namesake, the one faint star that is named on purpose. */
const NAMESAKE = 'Asterope';
const HEADER = 32;
const RECORD = 24;

/** `hip -> { mag, ra, dec }` read straight from the shipped catalog. */
function readCatalog(): Map<number, { mag: number; raDeg: number; decDeg: number }> {
  const bytes = readFileSync(fileURLToPath(new URL('../../../data/hip65.astc', import.meta.url)));
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const stars = new Map<number, { mag: number; raDeg: number; decDeg: number }>();
  for (let offset = HEADER; offset + RECORD <= bytes.byteLength; offset += RECORD) {
    const x = view.getFloat32(offset + 4, true);
    const y = view.getFloat32(offset + 8, true);
    const z = view.getFloat32(offset + 12, true);
    stars.set(view.getUint32(offset, true), {
      mag: view.getInt16(offset + 20, true) / 1000,
      raDeg: ((Math.atan2(y, x) * 180) / Math.PI + 360) % 360,
      decDeg: (Math.asin(z) * 180) / Math.PI,
    });
  }
  return stars;
}

describe('STAR_NAMES', () => {
  const catalog = readCatalog();

  it('names only catalog stars bright enough to carry a proper name', () => {
    for (const [hip, name] of STAR_NAMES) {
      const star = catalog.get(hip);
      expect(star, `${name} (HIP ${hip}) is in the catalog`).toBeDefined();
      expect(star?.mag, `${name} (HIP ${hip}) magnitude`).toBeLessThan(name === NAMESAKE ? 6 : 4.1);
    }
  });

  it('places well-known stars where they belong', () => {
    const expectations: [string, number, number, number][] = [
      ['Sirius', 101.29, -16.72, -1.44],
      ['Vega', 279.23, 38.78, 0.03],
      ['Betelgeuse', 88.79, 7.41, 0.45],
      ['Polaris', 37.95, 89.26, 1.97],
      ['Antares', 247.35, -26.43, 1.06],
      ['Dubhe', 165.93, 61.75, 1.81],
      ['Acrux', 186.65, -63.1, 0.77],
      ['Fomalhaut', 344.41, -29.62, 1.17],
      [NAMESAKE, 56.48, 24.55, 5.76],
    ];
    const byName = new Map([...STAR_NAMES].map(([hip, name]) => [name, hip]));
    for (const [name, raDeg, decDeg, mag] of expectations) {
      const star = catalog.get(byName.get(name) ?? -1);
      expect(star, name).toBeDefined();
      expect(Math.abs((star?.raDeg ?? 0) - raDeg), `${name} RA`).toBeLessThan(0.05);
      expect(Math.abs((star?.decDeg ?? 0) - decDeg), `${name} Dec`).toBeLessThan(0.05);
      expect(Math.abs((star?.mag ?? 9) - mag), `${name} magnitude`).toBeLessThan(0.15);
    }
  });

  it('has no duplicate names', () => {
    expect(new Set(STAR_NAMES.values()).size).toBe(STAR_NAMES.size);
  });

  it('falls back to the Hipparcos number', () => {
    expect(starLabel(27989)).toBe('Betelgeuse');
    expect(starLabel(26221)).toBe('HIP 26221');
  });
});
