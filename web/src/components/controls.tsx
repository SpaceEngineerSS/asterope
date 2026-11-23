import { useEffect, useId, useState } from 'react';
import type { KeyboardEvent, ReactNode } from 'react';

interface SectionProps {
  title: string;
  aside?: ReactNode;
  children: ReactNode;
}

export function Section({ title, aside, children }: SectionProps) {
  return (
    <section className="group">
      <header className="group__head">
        <h2>{title}</h2>
        {aside ? <span className="group__aside">{aside}</span> : null}
      </header>
      {children}
    </section>
  );
}

interface NumberFieldProps {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  digits: number;
  unit?: string;
  /** Wraps past the limits instead of clamping (angles). */
  wrap?: boolean;
  /** Adds a slider under the field. */
  slider?: boolean;
  /** Slider moves on a logarithmic scale; requires `min > 0`. */
  log?: boolean;
  title?: string;
  onChange: (value: number) => void;
}

function clampOrWrap(value: number, min: number, max: number, wrap: boolean): number {
  if (wrap) {
    const span = max - min;
    return ((((value - min) % span) + span) % span) + min;
  }
  return Math.min(Math.max(value, min), max);
}

export function NumberField({
  label,
  value,
  min,
  max,
  step,
  digits,
  unit,
  wrap = false,
  slider = false,
  log = false,
  title,
  onChange,
}: NumberFieldProps) {
  const id = useId();
  const [draft, setDraft] = useState<string | null>(null);

  useEffect(() => {
    setDraft(null);
  }, [value]);

  const commit = (text: string) => {
    const parsed = Number(text.replace(',', '.').replace('−', '-'));
    setDraft(null);
    if (Number.isFinite(parsed)) {
      onChange(clampOrWrap(parsed, min, max, wrap));
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter') {
      commit(event.currentTarget.value);
      return;
    }
    if (event.key === 'Escape') {
      setDraft(null);
      return;
    }
    if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') {
      return;
    }
    event.preventDefault();
    const scale = event.shiftKey ? 10 : event.altKey ? 0.1 : 1;
    const direction = event.key === 'ArrowUp' ? 1 : -1;
    onChange(clampOrWrap(value + direction * step * scale, min, max, wrap));
  };

  const toSlider = (v: number) => (log ? Math.log(Math.max(v, min)) : v);
  const fromSlider = (v: number) => (log ? Math.exp(v) : v);

  return (
    <div className="field" title={title}>
      <label htmlFor={id}>{label}</label>
      <span className="field__input">
        <input
          id={id}
          type="text"
          inputMode="decimal"
          spellCheck={false}
          autoComplete="off"
          value={draft ?? value.toFixed(digits)}
          onChange={(event) => {
            setDraft(event.currentTarget.value);
          }}
          onBlur={(event) => {
            if (draft !== null) {
              commit(event.currentTarget.value);
            }
          }}
          onKeyDown={onKeyDown}
        />
        {unit ? <span className="field__unit">{unit}</span> : null}
      </span>
      {slider ? (
        <input
          className="field__slider"
          type="range"
          aria-label={label}
          min={toSlider(min)}
          max={toSlider(max)}
          step={log ? (toSlider(max) - toSlider(min)) / 400 : step}
          value={toSlider(value)}
          onChange={(event) => {
            onChange(clampOrWrap(fromSlider(Number(event.currentTarget.value)), min, max, false));
          }}
        />
      ) : null}
    </div>
  );
}

interface ToggleProps {
  label: string;
  checked: boolean;
  swatch?: string;
  onChange: (checked: boolean) => void;
}

export function Toggle({ label, checked, swatch, onChange }: ToggleProps) {
  return (
    <label className="toggle">
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => {
          onChange(event.currentTarget.checked);
        }}
      />
      <span className="toggle__box" style={swatch && checked ? { background: swatch, borderColor: swatch } : undefined} />
      {label}
    </label>
  );
}

interface SelectProps<T extends string | number> {
  label: string;
  value: T;
  options: readonly { value: T; label: string }[];
  onChange: (value: T) => void;
}

export function Select<T extends string | number>({ label, value, options, onChange }: SelectProps<T>) {
  const id = useId();
  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      <span className="field__input">
        <select
          id={id}
          value={String(value)}
          onChange={(event) => {
            const picked = options.find((option) => String(option.value) === event.currentTarget.value);
            if (picked) {
              onChange(picked.value);
            }
          }}
        >
          {options.map((option) => (
            <option key={String(option.value)} value={String(option.value)}>
              {option.label}
            </option>
          ))}
        </select>
      </span>
    </div>
  );
}
