# Asterope lost-in-space validation

Master seed 1, 10000 trials per configuration, 1024x1024 sensor. Each trial draws a uniformly random attitude, hot-pixel fraction (1e-4 to 1e-3), 0-5 cosmic-ray streaks and, in 30 % of trials, a stray-light gradient.

Classification: **success** = solved with cross-boresight error <= 60" and roll error <= 300"; **false positive** = an attitude reported more than 0.1 deg from the truth; **inaccurate** = solved between those limits. No trial is removed from the denominator. Intervals are exact Clopper-Pearson 95 %.

## Baseline (FOV 14.0 deg)

| Criterion | Target | Measured | 95 % interval | Verdict |
|---|---|---|---|---|
| Success rate | > 98 % | 100.00 % | [99.96, 100.00] % | pass |
| False-positive rate | < 0.1 % | 0.000 % (0 of 10000) | [0, 0.037] % | pass |

| Accuracy (solved trials) | Median | 95th percentile | Maximum |
|---|---|---|---|
| Cross-boresight (") | 0.53 | 1.28 | 3.13 |
| Roll (") | 3.1 | 10.2 | 41.8 |

Pattern index: 5218 stars (V <= 6.05), 444071 pairs, built in 71 ms. Pixel scale 49.2"/px.

| Stage (native, single thread, median) | Time (ms) |
|---|---|
| Frame synthesis | 32.54 |
| Detection and centroiding | 8.03 |
| Identification and attitude | 0.14 |

### Trials that did not succeed

None.

## Field-of-view sweep

| FOV (deg) | Index stars | Catalog stars / frame | Detections / frame | Success (%) | 95 % CI | False positives | FP upper bound (%) | No solution | Inaccurate | Cross median (") | Cross p95 (") | Roll median (") | Roll p95 (") | Frames with < 4 stars (%) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 8 | 8790 | 13.8 | 13.9 | 98.38 | [98.11, 98.62] | 1 | 0.056 | 161 | 0 | 0.56 | 1.62 | 6.1 | 24.6 | 0.50 |
| 12 | 7109 | 31.1 | 30.4 | 100.00 | [99.96, 100.00] | 0 | 0.037 | 0 | 0 | 0.54 | 1.31 | 3.8 | 12.7 | 0.00 |
| 16 | 3990 | 55.2 | 49.2 | 100.00 | [99.96, 100.00] | 0 | 0.037 | 0 | 0 | 0.53 | 1.23 | 2.8 | 8.8 | 0.00 |
| 20 | 2543 | 85.8 | 61.9 | 100.00 | [99.96, 100.00] | 0 | 0.037 | 0 | 0 | 0.54 | 1.22 | 2.2 | 7.0 | 0.00 |
| 25 | 1621 | 133.2 | 64.0 | 100.00 | [99.96, 100.00] | 0 | 0.037 | 0 | 0 | 0.58 | 1.26 | 1.9 | 5.9 | 0.00 |

The success target applies to the baseline. Narrow fields are listed as measured: with a V <= 6.5 catalog some pointings hold too few stars.

## Stress sweeps

One sensor quantity is varied at a time with every other disturbance off, 300 trials per step, the same attitudes at every step. Body rate is a rotation about the sensor y axis during the exposure.

### Read noise (e-)

| Value | Success (%) | 95 % interval | No solution | Inaccurate | False positives | Cross median (") |
|---|---|---|---|---|---|---|
| 2 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.47 |
| 5 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.46 |
| 10 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.56 |
| 20 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.86 |
| 30 | 98.3 | [96.2, 99.5] | 5 | 0 | 0 | 1.12 |
| 45 | 79.0 | [73.9, 83.5] | 63 | 0 | 0 | 1.30 |
| 60 | 56.0 | [50.2, 61.7] | 132 | 0 | 0 | 1.58 |
| 80 | 32.0 | [26.8, 37.6] | 204 | 0 | 0 | 1.75 |
| 110 | 13.3 | [9.7, 17.7] | 260 | 0 | 0 | 1.72 |
| 150 | 9.0 | [6.0, 12.8] | 273 | 0 | 0 | 2.08 |

### Stray light (e-/px/s)

| Value | Success (%) | 95 % interval | No solution | Inaccurate | False positives | Cross median (") |
|---|---|---|---|---|---|---|
| 0 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.46 |
| 1000 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.56 |
| 3000 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.64 |
| 10000 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.94 |
| 30000 | 86.7 | [82.3, 90.3] | 40 | 0 | 0 | 1.23 |
| 60000 | 50.3 | [44.5, 56.1] | 149 | 0 | 0 | 1.37 |
| 100000 | 24.3 | [19.6, 29.6] | 227 | 0 | 0 | 1.28 |
| 200000 | 6.7 | [4.1, 10.1] | 280 | 0 | 0 | 0.95 |
| 400000 | 0.3 | [0.0, 1.8] | 299 | 0 | 0 | 1.09 |
| 800000 | 0.0 | [0.0, 1.2] | 300 | 0 | 0 | - |

### Hot pixels (%)

| Value | Success (%) | 95 % interval | No solution | Inaccurate | False positives | Cross median (") |
|---|---|---|---|---|---|---|
| 0 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.46 |
| 0.01 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.45 |
| 0.05 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.54 |
| 0.1 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.57 |
| 0.3 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.65 |
| 1 | 94.3 | [91.1, 96.7] | 17 | 0 | 0 | 1.03 |
| 2 | 51.7 | [45.9, 57.4] | 144 | 0 | 1 | 2.53 |
| 4 | 11.0 | [7.7, 15.1] | 267 | 0 | 0 | 6.31 |
| 8 | 5.7 | [3.3, 8.9] | 283 | 0 | 0 | 9.15 |
| 15 | 2.0 | [0.7, 4.3] | 294 | 0 | 0 | 12.03 |

### Cosmic rays (per frame)

| Value | Success (%) | 95 % interval | No solution | Inaccurate | False positives | Cross median (") |
|---|---|---|---|---|---|---|
| 0 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.46 |
| 5 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.46 |
| 20 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.46 |
| 50 | 94.7 | [91.5, 96.9] | 16 | 0 | 0 | 0.46 |
| 100 | 42.0 | [36.4, 47.8] | 174 | 0 | 0 | 0.43 |
| 200 | 9.0 | [6.0, 12.8] | 273 | 0 | 0 | 0.41 |
| 400 | 4.7 | [2.6, 7.7] | 286 | 0 | 0 | 0.62 |
| 600 | 2.0 | [0.7, 4.3] | 294 | 0 | 0 | 0.33 |
| 800 | 1.0 | [0.2, 2.9] | 297 | 0 | 0 | 0.80 |
| 1000 | 0.7 | [0.1, 2.4] | 298 | 0 | 0 | 0.94 |

### PSF sigma (px)

| Value | Success (%) | 95 % interval | No solution | Inaccurate | False positives | Cross median (") |
|---|---|---|---|---|---|---|
| 0.4 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.78 |
| 0.6 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.45 |
| 0.8 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.32 |
| 1.2 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.46 |
| 2 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.92 |
| 3 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 2.42 |
| 4 | 88.3 | [84.1, 91.7] | 34 | 0 | 1 | 3.84 |
| 5 | 55.3 | [49.5, 61.0] | 132 | 0 | 2 | 5.30 |
| 6.5 | 17.0 | [12.9, 21.7] | 249 | 0 | 0 | 6.89 |
| 8 | 2.7 | [1.2, 5.2] | 292 | 0 | 0 | 12.19 |

### Exposure (s)

| Value | Success (%) | 95 % interval | No solution | Inaccurate | False positives | Cross median (") |
|---|---|---|---|---|---|---|
| 0.002 | 1.7 | [0.5, 3.8] | 295 | 0 | 0 | 3.94 |
| 0.005 | 18.3 | [14.1, 23.2] | 245 | 0 | 0 | 1.90 |
| 0.01 | 71.7 | [66.2, 76.7] | 85 | 0 | 0 | 2.09 |
| 0.02 | 99.7 | [98.2, 100.0] | 1 | 0 | 0 | 1.57 |
| 0.05 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.77 |
| 0.1 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.46 |
| 0.2 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.37 |
| 0.5 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.24 |
| 1 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.18 |
| 2 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.12 |

### Body rate (deg/s)

| Value | Success (%) | 95 % interval | No solution | Inaccurate | False positives | Cross median (") |
|---|---|---|---|---|---|---|
| 0 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.46 |
| 0.25 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.54 |
| 0.5 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 0.65 |
| 1 | 100.0 | [98.8, 100.0] | 0 | 0 | 0 | 1.28 |
| 1.5 | 0.0 | [0.0, 1.2] | 300 | 0 | 0 | - |
| 2 | 0.0 | [0.0, 1.2] | 293 | 7 | 0 | 157.97 |
| 3 | 0.0 | [0.0, 1.2] | 299 | 0 | 1 | - |
| 4 | 0.0 | [0.0, 1.2] | 300 | 0 | 0 | - |
| 6 | 0.0 | [0.0, 1.2] | 300 | 0 | 0 | - |
| 8 | 0.0 | [0.0, 1.2] | 300 | 0 | 0 | - |

## Onboard profiles

The baseline scenario at 14 degrees, 10000 trials per profile, with the index serialized and searched in place and the solver limited to the onboard catalog. Frames are rendered from the whole catalog. "Stars per field" sets the faintest indexed magnitude; "reach" is the largest indexed separation as a fraction of the frame diagonal.

| Profile | Stars per field | Reach | Index stars | Index V limit | Pairs | Index (kB) | Catalog stars | Catalog (kB) | Success (%) | 95 % interval | No solution | False positives | Cross median (") | Identify (ms) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| bench | 25 | 1 | 5218 | 6.05 | 444071 | 4205 | 8870 | 213 | 100.00 | [99.96, 100.00] | 0 | 0 | 0.53 | 0.17 |
| wide-16 | 16 | 1 | 3334 | 5.65 | 181566 | 1768 | 5923 | 142 | 99.83 | [99.73, 99.90] | 17 | 0 | 0.55 | 0.14 |
| wide-12 | 12 | 1 | 2496 | 5.40 | 103435 | 1031 | 4506 | 108 | 98.84 | [98.61, 99.04] | 116 | 0 | 0.57 | 0.13 |
| near-16 | 16 | 0.6 | 3334 | 5.65 | 67414 | 740 | 5923 | 142 | 99.80 | [99.69, 99.88] | 20 | 0 | 0.55 | 0.12 |
| tight-16 | 16 | 0.5 | 3334 | 5.65 | 47223 | 558 | 5923 | 142 | 99.62 | [99.48, 99.73] | 38 | 0 | 0.55 | 0.11 |
| near-14 | 14 | 0.6 | 2916 | 5.54 | 52086 | 585 | 5244 | 126 | 99.55 | [99.40, 99.67] | 45 | 0 | 0.56 | 0.12 |
| near-12 | 12 | 0.6 | 2496 | 5.40 | 38614 | 447 | 4506 | 108 | 98.52 | [98.26, 98.75] | 148 | 0 | 0.57 | 0.13 |
| near-8 | 8 | 0.6 | 1666 | 5.04 | 17339 | 223 | 2988 | 72 | 87.07 | [86.40, 87.72] | 1293 | 0 | 0.58 | 0.14 |

