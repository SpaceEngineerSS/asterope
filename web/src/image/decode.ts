import { focalLength35mm, fovFromFocal35mm } from './exif';

export interface DecodedImage {
  width: number;
  height: number;
  pixels: Uint16Array;
  /** Integer block-averaging factor applied to fit the size limit. */
  binning: number;
  /** Field of view across the long side from EXIF, if the file carried one. */
  exifFovDeg: number | null;
}

export const MAX_SIDE = 4096;
const MIN_SIDE = 64;

const SRGB_TO_LINEAR = Float32Array.from({ length: 256 }, (_, value) => {
  const c = value / 255;
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
});

export function binningFor(width: number, height: number): number {
  return Math.max(1, Math.ceil(Math.max(width, height) / MAX_SIDE));
}

/** Linear-light luminance of sRGB bytes, block-averaged by `binning`, scaled to 16 bits. */
export function luminance16(rgba: Uint8ClampedArray, width: number, height: number, binning: number): Uint16Array {
  const outWidth = Math.floor(width / binning);
  const outHeight = Math.floor(height / binning);
  const out = new Uint16Array(outWidth * outHeight);
  const scale = 65535 / (binning * binning);
  for (let oy = 0; oy < outHeight; oy += 1) {
    for (let ox = 0; ox < outWidth; ox += 1) {
      let sum = 0;
      for (let by = 0; by < binning; by += 1) {
        let i = ((oy * binning + by) * width + ox * binning) * 4;
        for (let bx = 0; bx < binning; bx += 1) {
          sum +=
            0.2126 * (SRGB_TO_LINEAR[rgba[i] ?? 0] ?? 0) +
            0.7152 * (SRGB_TO_LINEAR[rgba[i + 1] ?? 0] ?? 0) +
            0.0722 * (SRGB_TO_LINEAR[rgba[i + 2] ?? 0] ?? 0);
          i += 4;
        }
      }
      out[oy * outWidth + ox] = Math.round(sum * scale);
    }
  }
  return out;
}

export function binPixels(pixels: Uint16Array, width: number, height: number, binning: number): Uint16Array {
  if (binning === 1) {
    return pixels;
  }
  const outWidth = Math.floor(width / binning);
  const outHeight = Math.floor(height / binning);
  const out = new Uint16Array(outWidth * outHeight);
  for (let oy = 0; oy < outHeight; oy += 1) {
    for (let ox = 0; ox < outWidth; ox += 1) {
      let sum = 0;
      for (let by = 0; by < binning; by += 1) {
        const row = (oy * binning + by) * width + ox * binning;
        for (let bx = 0; bx < binning; bx += 1) {
          sum += pixels[row + bx] ?? 0;
        }
      }
      out[oy * outWidth + ox] = Math.round(sum / (binning * binning));
    }
  }
  return out;
}

export function isFits(name: string, head: Uint8Array): boolean {
  const signature = String.fromCharCode(...head.subarray(0, 6));
  return signature === 'SIMPLE' || /\.(fits?|fts)$/i.test(name);
}

function checkSize(width: number, height: number): void {
  if (width < MIN_SIDE || height < MIN_SIDE) {
    throw new Error(`image is too small (${width} x ${height}); at least ${MIN_SIDE} px per side is needed`);
  }
}

/** A 2D context to read pixels back from; older Safari has no OffscreenCanvas. */
function scratchContext(width: number, height: number): OffscreenCanvasRenderingContext2D | CanvasRenderingContext2D | null {
  if (typeof OffscreenCanvas !== 'undefined') {
    return new OffscreenCanvas(width, height).getContext('2d', { willReadFrequently: true });
  }
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  return canvas.getContext('2d', { willReadFrequently: true });
}

/** Decodes a PNG or JPEG through the browser into 16-bit linear luminance. */
export async function decodeRaster(file: Blob, bytes: ArrayBuffer): Promise<DecodedImage> {
  const bitmap = await createImageBitmap(file, { colorSpaceConversion: 'default', imageOrientation: 'from-image' });
  try {
    checkSize(bitmap.width, bitmap.height);
    const context = scratchContext(bitmap.width, bitmap.height);
    if (!context) {
      throw new Error('2D canvas is unavailable');
    }
    context.drawImage(bitmap, 0, 0);
    const data = context.getImageData(0, 0, bitmap.width, bitmap.height);
    const binning = binningFor(bitmap.width, bitmap.height);
    const focal = focalLength35mm(bytes);
    return {
      width: Math.floor(bitmap.width / binning),
      height: Math.floor(bitmap.height / binning),
      pixels: luminance16(data.data, bitmap.width, bitmap.height, binning),
      binning,
      exifFovDeg: focal === null ? null : fovFromFocal35mm(focal),
    };
  } finally {
    bitmap.close();
  }
}

export function fromFits(width: number, height: number, pixels: Uint16Array): DecodedImage {
  checkSize(width, height);
  const binning = binningFor(width, height);
  return {
    width: Math.floor(width / binning),
    height: Math.floor(height / binning),
    pixels: binPixels(pixels, width, height, binning),
    binning,
    exifFovDeg: null,
  };
}
