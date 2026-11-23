"""The Python interface against the simulator and the command line."""
import json
import pathlib
import subprocess

import numpy as np
import pytest

import asterope

ROOT = pathlib.Path(__file__).resolve().parents[3]
ORION = dict(ra_deg=83.8, dec_deg=-5.4, roll_deg=12.0)


def angle_between(a, b):
    """Rotation angle between two attitude quaternions, arcseconds."""
    dot = abs(sum(x * y for x, y in zip(a, b)))
    return np.degrees(2 * np.arccos(min(1.0, dot))) * 3600


@pytest.fixture(scope="module")
def tracker():
    return asterope.StarTracker(1024, 1024, 14.0)


def test_a_simulated_frame_is_solved_where_it_was_pointed(tracker):
    frame = asterope.simulate(1024, 1024, 14.0, **ORION, seed=3)
    assert frame.dtype == np.uint16 and frame.shape == (1024, 1024)
    fix = tracker.solve(frame)
    assert fix.locked and fix.event == "acquired"
    assert abs(fix.ra_deg - 83.8) < 0.01 and abs(fix.dec_deg + 5.4) < 0.01
    assert abs(fix.roll_deg - 12.0) < 0.02
    assert len(fix.matches) >= 20 and fix.chance_probability < 1e-6
    names = {match.star for match in fix.matches}
    assert {"HIP 26727", "HIP 26311", "HIP 25930"} <= names, "Orion's belt"
    assert all(match.residual_arcsec < 30 for match in fix.matches)
    x, y, z, w = fix.quaternion
    assert abs(x * x + y * y + z * z + w * w - 1) < 1e-12 and w >= 0


def test_frames_in_a_row_are_tracked(tracker):
    tracker.reset()
    events = []
    for step in range(4):
        frame = asterope.simulate(1024, 1024, 14.0, 83.8 + 0.05 * step, -5.4, 12.0, seed=10 + step)
        events.append(tracker.update(frame, 0.2).event)
    assert events == ["acquired", "tracked", "tracked", "tracked"]


def test_a_frame_without_stars_gives_no_attitude(tracker):
    fix = tracker.solve(np.full((1024, 1024), 64, dtype=np.uint16))
    assert not fix.locked and fix.quaternion is None and fix.matches == []


def test_wrong_shapes_and_types_are_refused(tracker):
    with pytest.raises(ValueError):
        tracker.solve(np.zeros((512, 512), dtype=np.uint16))
    with pytest.raises(TypeError):
        tracker.solve(np.zeros((1024, 1024), dtype=np.float32))
    with pytest.raises(ValueError):
        asterope.StarTracker(1024, 1024, 0.0)
    with pytest.raises(ValueError):
        asterope.StarTracker(1024, 1024, 14.0, catalog=b"not a catalog")


def test_the_onboard_files_give_the_same_attitude_as_the_command_line(tmp_path):
    cli = ROOT / "target/release/asterope.exe"
    if not cli.exists():
        cli = ROOT / "target/release/asterope"
    if not cli.exists():
        pytest.skip("the asterope command is not built")
    catalog = (ROOT / "data/flight-14deg.astc").read_bytes()
    index = (ROOT / "data/flight-14deg.asti").read_bytes()
    onboard = asterope.StarTracker.from_bytes(1024, 1024, 14.0, catalog, index)
    frame = asterope.simulate(1024, 1024, 14.0, **ORION, seed=3)
    path = tmp_path / "frame.raw"
    frame.astype("<u2").tofile(path)
    out = subprocess.run(
        [cli, "solve", path, "--width", "1024", "--height", "1024", "--fov", "14",
         "--index", ROOT / "data/flight-14deg.asti", "--catalog", ROOT / "data/flight-14deg.astc", "--json"],
        capture_output=True, text=True, check=True,
    )
    theirs = json.loads(out.stdout)
    ours = onboard.solve(frame)
    assert theirs["solved"] and ours.locked
    assert abs(theirs["ra_deg"] - ours.ra_deg) < 1e-6 and abs(theirs["dec_deg"] - ours.dec_deg) < 1e-6


def test_a_photograph_is_solved_without_its_field_of_view():
    frame = asterope.simulate(1024, 1024, 22.0, 201.3, -11.2, 40.0, seed=5)
    found = asterope.solve_image(frame, hint_fov_deg=30.0)
    assert found.fix.locked
    assert abs(found.fov_deg - 22.0) < 0.2 and not found.equidistant
    assert abs(found.fix.ra_deg - 201.3) < 0.05 and abs(found.fix.dec_deg + 11.2) < 0.05


def test_a_gyro_carries_the_attitude_through_a_blank_frame():
    tracker = asterope.StarTracker(1024, 1024, 14.0)
    tracker.enable_gyro(0.003, 3e-5)
    rate = (0.0, 0.25, 0.0)
    first = asterope.simulate(1024, 1024, 14.0, **ORION, seed=1)
    fix, attitude, sigma, bias = tracker.update_aided(first)
    assert fix.locked and attitude is not None
    start = attitude
    for _ in range(10):
        tracker.gyro(rate, 0.02)
    blank = np.full((1024, 1024), 64, dtype=np.uint16)
    fix, attitude, sigma, bias = tracker.update_aided(blank)
    assert not fix.locked and attitude is not None
    # 0.25 deg/s for 0.2 s is 0.05 degrees, or 180 arcseconds.
    assert abs(angle_between(start, attitude) - 180) < 5
    assert max(sigma) > 0
