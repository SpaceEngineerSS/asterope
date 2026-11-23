"""Solves the same frames with tetra3 and with asterope and prints both.

    pip install pillow git+https://github.com/esa/tetra3.git
    python compare_tetra3.py <folder of .jpg frames>

The frames quoted in docs/validation.md are those in tetra3's own
`examples/data`. Columns: frame, then right ascension, declination, roll,
field of view and matches from tetra3, the same from asterope, and the
angle between the two boresights in arcseconds.
"""
import glob, sys, os
import numpy as np
from PIL import Image
import math
np.math = math  # tetra3 predates numpy 2, which dropped this alias
import tetra3, asterope

t3 = tetra3.Tetra3()
rows = []
for path in sorted(glob.glob(os.path.join(sys.argv[1], "*.jpg"))):
    image = Image.open(path)
    theirs = t3.solve_from_image(image)
    gray = np.asarray(image.convert("L"), dtype=np.uint16) * 257
    ours = asterope.solve_image(np.ascontiguousarray(gray), hint_fov_deg=12.0)
    name = os.path.basename(path).split("_", 1)[1].replace("_Try1.jpg", "")
    if theirs["RA"] is None or not ours.fix.locked:
        rows.append((name, theirs["RA"], ours.fix.locked))
        continue
    def unit(ra, dec):
        ra, dec = np.radians(ra), np.radians(dec)
        return np.array([np.cos(dec) * np.cos(ra), np.cos(dec) * np.sin(ra), np.sin(dec)])
    apart = np.degrees(np.arccos(np.clip(unit(theirs["RA"], theirs["Dec"]) @ unit(ours.fix.ra_deg, ours.fix.dec_deg), -1, 1))) * 3600
    # tetra3's roll is the angle of north from image up, counted the other way.
    rows.append((name, theirs["RA"], theirs["Dec"], theirs["Roll"], theirs["FOV"], theirs["Matches"],
                 ours.fix.ra_deg, ours.fix.dec_deg, ours.fix.roll_deg, ours.fov_deg, len(ours.fix.matches), apart))
for row in rows:
    print(" | ".join(f"{v:.4f}" if isinstance(v, float) else str(v) for v in row))
