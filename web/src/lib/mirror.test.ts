import { describe, expect, it } from 'vitest';

import { mirrorRows } from './mirror';

describe('mirrorRows', () => {
  it('reverses every row and leaves the rows in place', () => {
    const flipped = mirrorRows(Uint16Array.from([1, 2, 3, 4, 5, 6]), 3);
    expect([...flipped]).toEqual([3, 2, 1, 6, 5, 4]);
  });

  it('is its own inverse', () => {
    const pixels = Uint16Array.from({ length: 35 }, (_, index) => index * 7);
    expect([...mirrorRows(mirrorRows(pixels, 7), 7)]).toEqual([...pixels]);
  });
});
