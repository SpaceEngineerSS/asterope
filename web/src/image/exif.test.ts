import { describe, expect, it } from 'vitest';

import { focalLength35mm, fovFromFocal35mm } from './exif';

function jpegWithFocal(focal: number, little: boolean): ArrayBuffer {
  const tiff = new DataView(new ArrayBuffer(8 + 2 + 12 + 4 + 2 + 12 + 4));
  tiff.setUint16(0, little ? 0x4949 : 0x4d4d);
  tiff.setUint16(2, 42, little);
  tiff.setUint32(4, 8, little);
  tiff.setUint16(8, 1, little);
  tiff.setUint16(10, 0x8769, little);
  tiff.setUint16(12, 4, little);
  tiff.setUint32(14, 1, little);
  tiff.setUint32(18, 26, little);
  tiff.setUint16(26, 1, little);
  tiff.setUint16(28, 0xa405, little);
  tiff.setUint16(30, 3, little);
  tiff.setUint32(32, 1, little);
  tiff.setUint16(36, focal, little);

  const out = new DataView(new ArrayBuffer(2 + 4 + 6 + tiff.byteLength + 2));
  out.setUint16(0, 0xffd8);
  out.setUint16(2, 0xffe1);
  out.setUint16(4, 2 + 6 + tiff.byteLength);
  out.setUint32(6, 0x45786966);
  out.setUint16(10, 0);
  new Uint8Array(out.buffer).set(new Uint8Array(tiff.buffer), 12);
  out.setUint16(12 + tiff.byteLength, 0xffd9);
  return out.buffer;
}

describe('focalLength35mm', () => {
  it('reads both byte orders', () => {
    expect(focalLength35mm(jpegWithFocal(24, true))).toBe(24);
    expect(focalLength35mm(jpegWithFocal(85, false))).toBe(85);
  });

  it('returns null for non-JPEG or truncated data', () => {
    expect(focalLength35mm(new ArrayBuffer(0))).toBeNull();
    expect(focalLength35mm(Uint8Array.of(0x89, 0x50, 0x4e, 0x47).buffer)).toBeNull();
    expect(focalLength35mm(jpegWithFocal(24, true).slice(0, 20))).toBeNull();
  });
});

describe('fovFromFocal35mm', () => {
  it('matches the full-frame field of view', () => {
    expect(fovFromFocal35mm(50)).toBeCloseTo(39.6, 1);
    expect(fovFromFocal35mm(135)).toBeCloseTo(15.19, 1);
  });
});
