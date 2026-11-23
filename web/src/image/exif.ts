const TAG_EXIF_IFD = 0x8769;
const TAG_FOCAL_LENGTH_35MM = 0xa405;
const FULL_FRAME_LONG_SIDE_MM = 36;

interface Tiff {
  view: DataView;
  start: number;
  little: boolean;
}

function findTag(tiff: Tiff, ifdOffset: number, tag: number): number | null {
  const { view, start, little } = tiff;
  const base = start + ifdOffset;
  if (base + 2 > view.byteLength) {
    return null;
  }
  const entries = view.getUint16(base, little);
  for (let i = 0; i < entries; i += 1) {
    const entry = base + 2 + i * 12;
    if (entry + 12 > view.byteLength) {
      return null;
    }
    if (view.getUint16(entry, little) === tag) {
      const type = view.getUint16(entry + 2, little);
      return type === 3 ? view.getUint16(entry + 8, little) : view.getUint32(entry + 8, little);
    }
  }
  return null;
}

/** 35 mm-equivalent focal length from a JPEG's EXIF block, if present. */
export function focalLength35mm(bytes: ArrayBuffer): number | null {
  const view = new DataView(bytes);
  if (view.byteLength < 4 || view.getUint16(0) !== 0xffd8) {
    return null;
  }
  let offset = 2;
  while (offset + 4 <= view.byteLength) {
    const marker = view.getUint16(offset);
    const length = view.getUint16(offset + 2);
    if ((marker & 0xff00) !== 0xff00 || length < 2) {
      return null;
    }
    const isExif =
      marker === 0xffe1 &&
      offset + 10 <= view.byteLength &&
      view.getUint32(offset + 4) === 0x45786966 &&
      view.getUint16(offset + 8) === 0;
    if (isExif) {
      const start = offset + 10;
      if (start + 8 > view.byteLength) {
        return null;
      }
      const order = view.getUint16(start);
      if (order !== 0x4949 && order !== 0x4d4d) {
        return null;
      }
      const tiff: Tiff = { view, start, little: order === 0x4949 };
      const exifIfd = findTag(tiff, view.getUint32(start + 4, tiff.little), TAG_EXIF_IFD);
      if (exifIfd === null) {
        return null;
      }
      const focal = findTag(tiff, exifIfd, TAG_FOCAL_LENGTH_35MM);
      return focal !== null && focal > 0 ? focal : null;
    }
    if (marker === 0xffda) {
      return null;
    }
    offset += 2 + length;
  }
  return null;
}

/** Field of view (degrees) across the long image side for a 35 mm-equivalent focal length. */
export function fovFromFocal35mm(focalMm: number): number {
  return (2 * Math.atan(FULL_FRAME_LONG_SIDE_MM / (2 * focalMm)) * 180) / Math.PI;
}
