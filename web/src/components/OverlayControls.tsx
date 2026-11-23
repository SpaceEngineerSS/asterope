import { COLORS } from '../overlay/draw';
import { setOverlay } from '../state/store';
import type { Bench, OverlayOptions } from '../state/store';
import { NumberField, Section, Toggle } from './controls';

type Flag = Exclude<keyof OverlayOptions, 'residualScale'>;

const FLAGS: readonly { key: Flag; label: string; swatch?: string }[] = [
  { key: 'centroids', label: 'Centroids' },
  { key: 'matches', label: 'Matches', swatch: COLORS.match },
  { key: 'labels', label: 'HIP labels' },
  { key: 'rejected', label: 'Rejected', swatch: COLORS.rejected },
  { key: 'pyramid', label: 'Pyramid' },
  { key: 'residuals', label: 'Residuals', swatch: COLORS.unmatched },
  { key: 'catalog', label: 'Catalog', swatch: COLORS.catalog },
  { key: 'graticule', label: 'RA/Dec grid' },
  { key: 'reticle', label: 'Reticle' },
];

export function OverlayControls({ bench, overlay }: { bench: Bench; overlay: OverlayOptions }) {
  return (
    <Section title="Overlay">
      <div className="toggles">
        {FLAGS.map(({ key, label, swatch }) => (
          <Toggle
            key={key}
            label={label}
            checked={overlay[key]}
            {...(swatch ? { swatch } : {})}
            onChange={(checked) => {
              setOverlay(bench, { [key]: checked });
            }}
          />
        ))}
      </div>
      <NumberField
        label="Residual gain"
        value={overlay.residualScale}
        min={1}
        max={500}
        step={5}
        digits={0}
        unit={'×'}
        title="Magnification of the drawn residual vectors"
        onChange={(residualScale) => {
          setOverlay(bench, { residualScale });
        }}
      />
    </Section>
  );
}
