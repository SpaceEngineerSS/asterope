/**
 * The frame flipped left to right. A telescope with a star diagonal, or
 * any odd number of mirrors, shows the sky reversed, and a reversed sky
 * matches no catalog: every triangle in it is the mirror image of a real one.
 */
export function mirrorRows(pixels: Uint16Array, width: number): Uint16Array {
  const flipped = new Uint16Array(pixels.length);
  for (let start = 0; start + width <= pixels.length; start += width) {
    for (let x = 0; x < width; x += 1) {
      flipped[start + x] = pixels[start + width - 1 - x] ?? 0;
    }
  }
  return flipped;
}
