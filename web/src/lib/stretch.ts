export type Stretch = 'linear' | 'asinh' | 'zscale';

export interface StretchInput {
  background: number;
  noise: number;
  maximum: number;
}

/** 16-bit to 8-bit display lookup table. */
export function buildLut(stretch: Stretch, input: StretchInput): Uint8Array {
  const lut = new Uint8Array(65536);
  const noise = Math.max(input.noise, 0.5);
  const black = stretch === 'linear' ? input.background : input.background - 2 * noise;
  const white =
    stretch === 'zscale' ? input.background + 25 * noise : Math.max(input.maximum, black + 4 * noise);
  const span = Math.max(white - black, 1e-9);
  const softening = (8 * noise) / span;
  const asinhScale = 1 / Math.asinh(1 / softening);
  for (let value = 0; value < lut.length; value += 1) {
    const t = Math.min(Math.max((value - black) / span, 0), 1);
    const mapped = stretch === 'asinh' ? Math.asinh(t / softening) * asinhScale : t;
    lut[value] = Math.round(255 * mapped);
  }
  return lut;
}

export function maximumOf(pixels: Uint16Array): number {
  let maximum = 0;
  for (const value of pixels) {
    if (value > maximum) {
      maximum = value;
    }
  }
  return maximum;
}

/** Fills RGBA bytes with the stretched grayscale frame. */
export function paint(pixels: Uint16Array, lut: Uint8Array, rgba: Uint8ClampedArray): void {
  const words = new Uint32Array(rgba.buffer, rgba.byteOffset, rgba.byteLength >>> 2);
  const count = Math.min(pixels.length, words.length);
  for (let i = 0; i < count; i += 1) {
    const level = lut[pixels[i] ?? 0] ?? 0;
    words[i] = 0xff000000 | (level << 16) | (level << 8) | level;
  }
}
