"""Writes solar_system.csv: where ERFA puts the Sun, the Moon and the planets,
and what it gives for refraction, for tests/solar_system.rs to compare with.

    python make_solar_reference.py        (needs pyerfa and numpy)

Planets: erfa.plan94 (Simon et al. 1994) minus the Earth from erfa.epv00,
with light time. Moon: erfa.moon98 (Meeus 1998, after ELP2000-82). All are
geocentric unit vectors in the J2000 mean equator and equinox, which lie
within 0.03 arcseconds of the ICRS axes.
"""
import erfa
import numpy as np

J2000 = 2451545.0
LIGHT_DAYS_PER_AU = 0.0057755183
PLANETS = {"mercury": 1, "venus": 2, "mars": 4, "jupiter": 5, "saturn": 6, "uranus": 7, "neptune": 8}

rows = ["kind,jd_tt,x,y,z,distance"]
for step in range(-17, 70):
    jd = J2000 + 211.3 * step
    earth = erfa.epv00(jd, 0.0)[0]["p"]
    rows.append("sun,%.1f,%.9f,%.9f,%.9f,%.6f" % (jd, *(-earth / np.linalg.norm(earth)), np.linalg.norm(earth)))
    for name, number in PLANETS.items():
        seen = erfa.plan94(jd, 0.0, number)["p"] - earth
        for _ in range(3):
            seen = erfa.plan94(jd - LIGHT_DAYS_PER_AU * np.linalg.norm(seen), 0.0, number)["p"] - earth
        distance = np.linalg.norm(seen)
        rows.append("%s,%.1f,%.9f,%.9f,%.9f,%.6f" % (name, jd, *(seen / distance), distance))
    moon = erfa.moon98(jd, 0.0)["p"]
    distance = np.linalg.norm(moon)
    rows.append("moon,%.1f,%.9f,%.9f,%.9f,%.9f" % (jd, *(moon / distance), distance))

# Refraction at 1010 hPa, 10 C, dry air, 0.55 um: R = A tan z + B tan^3 z.
a, b = erfa.refco(1010.0, 10.0, 0.0, 0.55)
for altitude in (15, 20, 30, 45, 60, 75, 90):
    tangent = np.tan(np.radians(90.0 - altitude))
    rows.append("refraction,%d,%.9e,0,0,0" % (altitude, a * tangent + b * tangent**3))

open("solar_system.csv", "w", newline="\n").write("\n".join(rows) + "\n")
print(len(rows) - 1, "rows")
