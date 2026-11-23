const MINUS = '−';

function withSign(text: string, value: number): string {
  if (value < 0 || Object.is(value, -0)) {
    return MINUS + text;
  }
  return `+${text}`;
}

export function fixed(value: number, digits: number): string {
  if (!Number.isFinite(value)) {
    return '—';
  }
  return value < 0 ? MINUS + Math.abs(value).toFixed(digits) : value.toFixed(digits);
}

export function signed(value: number, digits: number): string {
  if (!Number.isFinite(value)) {
    return '—';
  }
  return withSign(Math.abs(value).toFixed(digits), value);
}

/** Quaternion component with grouped decimals, e.g. `+0.412 733 19`. */
export function quaternionComponent(value: number): string {
  if (!Number.isFinite(value)) {
    return '—';
  }
  const [whole = '0', fraction = ''] = Math.abs(value).toFixed(8).split('.');
  const grouped = `${fraction.slice(0, 3)} ${fraction.slice(3, 6)} ${fraction.slice(6)}`;
  return withSign(`${whole}.${grouped}`, value);
}

/** Unsigned angle padded to three integer digits, e.g. `083.8221`. */
export function angle360(value: number, digits: number): string {
  if (!Number.isFinite(value)) {
    return '—';
  }
  return value.toFixed(digits).padStart(4 + digits, '0');
}

/** Signed angle padded to two integer digits, e.g. `-05.3909`. */
export function angle90(value: number, digits: number): string {
  if (!Number.isFinite(value)) {
    return '—';
  }
  return withSign(Math.abs(value).toFixed(digits).padStart(3 + digits, '0'), value);
}

export function scientific(value: number, digits = 1): string {
  if (!Number.isFinite(value)) {
    return '—';
  }
  return value.toExponential(digits).replace('-', MINUS).replace('+', '');
}

export function integer(value: number): string {
  return Number.isFinite(value) ? Math.round(value).toLocaleString('en-US').replace(/,/g, ' ') : '—';
}

export function percent(fraction: number, digits = 2): string {
  return Number.isFinite(fraction) ? `${(100 * fraction).toFixed(digits)} %` : '—';
}

/** Smallest signed difference `a - b` between two angles in degrees. */
export function angleDifference(a: number, b: number): number {
  return ((((a - b + 180) % 360) + 360) % 360) - 180;
}
