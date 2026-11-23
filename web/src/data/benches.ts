import type { Tab } from '../state/store';

/** What each bench is for, shown above it and in the manual. */
export const BENCHES: readonly { tab: Tab; name: string; asks: string; use: string }[] = [
  {
    tab: 'synthetic',
    name: 'Synthetic',
    asks: 'Would this sensor, at this attitude and under these disturbances, find its attitude from one frame?',
    use: 'Drag the image to slew, raise the noise, stray light or body rate, and watch where the lamp turns from LOCK to NO LOCK. Run a sequence to see the tracker acquire and follow. Record it as video.',
  },
  {
    tab: 'image',
    name: 'Image',
    asks: 'Which stars are in this photograph, and where was the camera pointing?',
    use: 'Drop a PNG, JPEG or FITS file. Enter the field of view if you know it, or mark it unknown and let it be searched. An image reversed by a mirror has to be marked as mirrored. Matched stars are labelled with their names and Hipparcos numbers.',
  },
  {
    tab: 'video',
    name: 'Video',
    asks: 'How does the pointing change over this video, and where is the lock lost?',
    use: 'Drop a video or open a camera. Every sampled frame is solved; the timeline shows right ascension, declination, roll and rate, and the sky map shows the path. Stack frames when single ones show too few stars.',
  },
  {
    tab: 'design',
    name: 'Design',
    asks: 'Would a camera with this lens and detector work as a star tracker, and how well?',
    use: 'Enter focal length, aperture, pixel size and noise. The map shows how many stars each part of the sky offers. Then let the simulator test the prediction.',
  },
  {
    tab: 'mission',
    name: 'Mission',
    asks: 'Carried round this orbit, how much of the time can the camera see stars, and what stops it?',
    use: 'Set the orbit, the direction the camera looks relative to it, and how close the Sun, the Earth and the Moon may come. The chart shows their angles from the boresight through the orbit and the stretches in which the camera is blinded. Click a moment to open that sky on the Synthetic bench.',
  },
  {
    tab: 'validation',
    name: 'Validation',
    asks: 'Do the numbers this application reports hold up, and where does the solver break?',
    use: 'Run the centroid, attitude-solver and lost-in-space suites, and the stress sweeps. The same suites run from the command line with identical results.',
  },
];
