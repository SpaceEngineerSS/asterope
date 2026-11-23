import { describe, expect, it } from 'vitest';

import { binPixels, binningFor, isFits, luminance16 } from './decode';

describe('binningFor', () => {
  it('keeps the long side within the limit', () => {
    expect(binningFor(4096, 3000)).toBe(1);
    expect(binningFor(6000, 4000)).toBe(2);
    expect(binningFor(12000, 8000)).toBe(3);
  });
});

describe('luminance16', () => {
  it('maps white to full scale and black to zero', () => {
    const rgba = Uint8ClampedArray.of(255, 255, 255, 255, 0, 0, 0, 255);
    expect(Array.from(luminance16(rgba, 2, 1, 1))).toEqual([65535, 0]);
  });

  it('linearizes sRGB before weighting', () => {
    const rgba = Uint8ClampedArray.of(128, 128, 128, 255);
    const [value] = luminance16(rgba, 1, 1, 1);
    expect(value).toBeGreaterThan(14000);
    expect(value).toBeLessThan(14300);
  });

  it('averages blocks when binning', () => {
    const rgba = Uint8ClampedArray.of(255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255);
    expect(Array.from(luminance16(rgba, 2, 2, 2))).toEqual([32768]);
  });
});

describe('binPixels', () => {
  it('averages 16-bit blocks and drops partial ones', () => {
    const pixels = Uint16Array.of(10, 20, 99, 30, 40, 99);
    expect(Array.from(binPixels(pixels, 3, 2, 2))).toEqual([25]);
    expect(binPixels(pixels, 3, 2, 1)).toBe(pixels);
  });
});

describe('isFits', () => {
  it('recognizes the signature or the extension', () => {
    const simple = new TextEncoder().encode('SIMPLE  =');
    expect(isFits('frame.bin', simple)).toBe(true);
    expect(isFits('m42.FIT', new Uint8Array(8))).toBe(true);
    expect(isFits('m42.png', new Uint8Array(8))).toBe(false);
  });
});
