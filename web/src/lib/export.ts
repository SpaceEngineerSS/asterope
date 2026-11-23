import { STAR_NAMES } from '../data/star-names';
import type { FrameResult } from '../engine/types';
import type { VideoSample } from '../state/video';

const RAD = 180 / Math.PI;
const FITS_BLOCK = 2880;

/** Solution, calibration and matched stars as indented JSON. */
export function solutionJson(frame: FrameResult, source: string | null): string {
  const attitude = frame.solve.attitude;
  return JSON.stringify(
    {
      generator: 'Asterope',
      source,
      image: { width: frame.width, height: frame.height },
      solved: attitude !== null,
      quaternion: attitude
        ? { convention: 'ICRS to sensor, scalar last [q1, q2, q3, q4]', value: attitude.q }
        : null,
      pointing: attitude
        ? { raDeg: attitude.pointing.raDeg, decDeg: attitude.pointing.decDeg, rollDeg: attitude.pointing.rollDeg }
        : null,
      camera: frame.solve.intrinsics,
      residualRmsArcsec: attitude?.rmsArcsec ?? null,
      sigmaArcsec: attitude?.sigmaArcsec ?? null,
      chanceProbability: attitude?.chanceProbability ?? null,
      matches: (attitude?.matches ?? []).map((m) => {
        const detection = frame.detect.detections[m.detection];
        return {
          hip: m.hip,
          name: STAR_NAMES.get(m.hip) ?? null,
          magnitude: m.mag,
          x: detection?.x ?? null,
          y: detection?.y ?? null,
          residualArcsec: m.residualArcsec,
        };
      }),
      detections: frame.detect.detections.length,
    },
    null,
    2,
  );
}

/** Matched stars as CSV with a header row. */
export function matchesCsv(frame: FrameResult): string {
  const rows = ['hip,name,magnitude,x,y,predicted_x,predicted_y,residual_arcsec'];
  for (const m of frame.solve.attitude?.matches ?? []) {
    const d = frame.detect.detections[m.detection];
    rows.push(
      [
        m.hip,
        STAR_NAMES.get(m.hip) ?? '',
        m.mag.toFixed(3),
        d ? d.x.toFixed(3) : '',
        d ? d.y.toFixed(3) : '',
        m.predictedX.toFixed(3),
        m.predictedY.toFixed(3),
        m.residualArcsec.toFixed(2),
      ].join(','),
    );
  }
  return `${rows.join('\n')}\n`;
}

/** One row per solved video frame: time, tracker state, quaternion (scalar last) and pointing. */
export function videoCsv(samples: readonly VideoSample[]): string {
  const rows = ['time_s,state,q1,q2,q3,q4,ra_deg,dec_deg,roll_deg,rate_deg_s,matches,detections,rms_arcsec,frames_stacked'];
  const number = (value: number | null | undefined, digits: number) =>
    value === null || value === undefined || !Number.isFinite(value) ? '' : value.toFixed(digits);
  for (const s of samples) {
    rows.push(
      [
        s.timeS.toFixed(4),
        s.event,
        ...[0, 1, 2, 3].map((i) => number(s.q?.[i], 9)),
        number(s.pointing?.raDeg, 5),
        number(s.pointing?.decDeg, 5),
        number(s.pointing?.rollDeg, 5),
        number(s.rateDegS, 4),
        s.matches,
        s.detections,
        number(s.rmsArcsec, 2),
        s.stackFrames,
      ].join(','),
    );
  }
  return `${rows.join('\n')}\n`;
}

function card(keyword: string, value: string, comment = ''): string {
  const text = `${keyword.padEnd(8)}= ${value.padStart(20)}${comment ? ` / ${comment}` : ''}`;
  return text.padEnd(80).slice(0, 80);
}

/**
 * FITS WCS keywords (gnomonic, `TAN`) for the solved frame.
 *
 * Pixel (1, 1) is the top-left pixel and the second axis runs down the
 * image, as the file is stored. With image "up" at position angle `roll`
 * (north through east), sensor x points along `-E cos(roll) + N sin(roll)`
 * and sensor y along `-E sin(roll) - N cos(roll)`, which gives the CD matrix
 * below. Lens distortion is not represented.
 */
export function wcsCards(frame: FrameResult): string[] | null {
  const attitude = frame.solve.attitude;
  if (!attitude) {
    return null;
  }
  const { fx, fy, cx, cy } = frame.solve.intrinsics;
  const roll = attitude.pointing.rollDeg / RAD;
  const number = (value: number) => value.toExponential(12).toUpperCase();
  return [
    card('WCSAXES', '2'),
    card('CTYPE1', "'RA---TAN'", 'gnomonic projection'),
    card('CTYPE2', "'DEC--TAN'"),
    card('RADESYS', "'ICRS    '"),
    card('EQUINOX', '2000.0'),
    card('CRVAL1', number(attitude.pointing.raDeg), 'deg, boresight RA'),
    card('CRVAL2', number(attitude.pointing.decDeg), 'deg, boresight Dec'),
    card('CRPIX1', number(cx + 1), 'principal point, 1-based'),
    card('CRPIX2', number(cy + 1), 'rows counted from the top'),
    card('CUNIT1', "'deg     '"),
    card('CUNIT2', "'deg     '"),
    card('CD1_1', number((-Math.cos(roll) / fx) * RAD)),
    card('CD1_2', number((-Math.sin(roll) / fy) * RAD)),
    card('CD2_1', number((Math.sin(roll) / fx) * RAD)),
    card('CD2_2', number((-Math.cos(roll) / fy) * RAD)),
  ];
}

export function wcsText(frame: FrameResult): string | null {
  const cards = wcsCards(frame);
  return cards ? `${cards.map((c) => c.trimEnd()).join('\n')}\nEND\n` : null;
}

/** RA / Dec (degrees) of a pixel according to a set of TAN WCS values. */
export function wcsPixelToSky(
  wcs: { crval: [number, number]; crpix: [number, number]; cd: [number, number, number, number] },
  x: number,
  y: number,
): [number, number] {
  const du = x + 1 - wcs.crpix[0];
  const dv = y + 1 - wcs.crpix[1];
  const xi = (wcs.cd[0] * du + wcs.cd[1] * dv) / RAD;
  const eta = (wcs.cd[2] * du + wcs.cd[3] * dv) / RAD;
  const ra0 = wcs.crval[0] / RAD;
  const dec0 = wcs.crval[1] / RAD;
  const denominator = Math.cos(dec0) - eta * Math.sin(dec0);
  const ra = ra0 + Math.atan2(xi, denominator);
  const dec = Math.atan2(Math.sin(dec0) + eta * Math.cos(dec0), Math.hypot(xi, denominator));
  return [((ra * RAD) % 360 + 360) % 360, dec * RAD];
}

/**
 * The frame as a 16-bit FITS file (unsigned via `BZERO = 32768`), bottom row
 * first as the format expects, with the WCS of the solution when there is one.
 */
export function frameFits(frame: FrameResult): Uint8Array<ArrayBuffer> {
  const { width, height, pixels } = frame;
  const cards = [
    card('SIMPLE', 'T', 'written by Asterope'),
    card('BITPIX', '16'),
    card('NAXIS', '2'),
    card('NAXIS1', String(width)),
    card('NAXIS2', String(height)),
    card('BZERO', '32768', 'unsigned 16-bit'),
    card('BSCALE', '1'),
  ];
  // The file stores rows bottom-up, so the WCS written here has its second
  // axis flipped relative to `wcsCards`.
  const wcs = wcsCards(frame);
  if (wcs && frame.solve.attitude) {
    const { fy, cy } = frame.solve.intrinsics;
    const roll = frame.solve.attitude.pointing.rollDeg / RAD;
    const number = (value: number) => value.toExponential(12).toUpperCase();
    cards.push(
      ...wcs.filter((c) => !/^(CRPIX2|CD1_2|CD2_2)/.test(c)),
      card('CRPIX2', number(height - cy), 'rows counted from the bottom'),
      card('CD1_2', number((Math.sin(roll) / fy) * RAD)),
      card('CD2_2', number((Math.cos(roll) / fy) * RAD)),
    );
  }
  cards.push('END'.padEnd(80));
  const headerBytes = Math.ceil((cards.length * 80) / FITS_BLOCK) * FITS_BLOCK;
  const dataBytes = Math.ceil((pixels.length * 2) / FITS_BLOCK) * FITS_BLOCK;
  const out = new Uint8Array(new ArrayBuffer(headerBytes + dataBytes));
  out.fill(0x20, 0, headerBytes);
  cards.forEach((text, i) => {
    for (let c = 0; c < 80; c += 1) {
      out[i * 80 + c] = text.charCodeAt(c);
    }
  });
  const view = new DataView(out.buffer);
  for (let row = 0; row < height; row += 1) {
    const source = (height - 1 - row) * width;
    for (let x = 0; x < width; x += 1) {
      view.setInt16(headerBytes + (row * width + x) * 2, (pixels[source + x] ?? 0) - 32768, false);
    }
  }
  return out;
}

/** Saves text or bytes as a file from the browser. */
export function download(name: string, content: string | Uint8Array<ArrayBuffer>, type: string): void {
  downloadBlob(name, new Blob([content], { type }));
}

export function downloadBlob(name: string, blob: Blob): void {
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = name;
  link.click();
  URL.revokeObjectURL(url);
}
