import { useState } from 'react';

import type { FrameResult } from '../engine/types';
import { download, frameFits, matchesCsv, solutionJson, wcsText } from '../lib/export';

interface Props {
  frame: FrameResult | null;
  /** Base name for the saved files. */
  name: string;
  /** Returns a link that reproduces the current bench, when the bench supports one. */
  shareLink?: () => string;
}

export function ExportBar({ frame, name, shareLink }: Props) {
  const [copied, setCopied] = useState(false);
  const solved = frame?.solve.attitude != null;

  const copyLink = () => {
    if (!shareLink) {
      return;
    }
    void navigator.clipboard.writeText(shareLink()).then(() => {
      setCopied(true);
      setTimeout(() => {
        setCopied(false);
      }, 1500);
    });
  };

  return (
    <div className="export" role="group" aria-label="Export">
      <button
        type="button"
        className="button"
        disabled={!frame}
        title="Solution, calibration and matched stars"
        onClick={() => {
          if (frame) {
            download(`${name}.json`, solutionJson(frame, name), 'application/json');
          }
        }}
      >
        JSON
      </button>
      <button
        type="button"
        className="button"
        disabled={!solved}
        title="Matched stars, one row each"
        onClick={() => {
          if (frame) {
            download(`${name}-stars.csv`, matchesCsv(frame), 'text/csv');
          }
        }}
      >
        CSV
      </button>
      <button
        type="button"
        className="button"
        disabled={!solved}
        title="FITS WCS keywords (TAN projection) of the solution"
        onClick={() => {
          const text = frame ? wcsText(frame) : null;
          if (text) {
            download(`${name}.wcs.txt`, text, 'text/plain');
          }
        }}
      >
        WCS
      </button>
      <button
        type="button"
        className="button"
        disabled={!frame}
        title="The frame as a 16-bit FITS image, with the WCS when solved"
        onClick={() => {
          if (frame) {
            download(`${name}.fits`, frameFits(frame), 'application/fits');
          }
        }}
      >
        FITS
      </button>
      {shareLink ? (
        <button type="button" className="button" title="Copy a link that reproduces this bench" onClick={copyLink}>
          {copied ? 'Copied' : 'Link'}
        </button>
      ) : null}
    </div>
  );
}
