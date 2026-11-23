//! `solve`, `simulate`, `build-index` and `replay`.

use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use asterope_core::attitude::{attitude_from_radec_roll, RaDecRoll};
use asterope_core::calib::{
    auto_tolerances, solve_unknown_fov, solve_with_scale_search, widest_camera, ScaleSearch,
};
use asterope_core::calibrate::{
    calibrate, Calibration, CalibrationFrame, CalibrationModel, CalibrationStar, PARAMETER_NAMES,
};
use asterope_core::camera::{Camera, Distortion, Projection};
use asterope_core::catalog::{Catalog, Designation};
use asterope_core::detect::{detect, Frame};
use asterope_core::flight::{Fix, StarTracker};
use asterope_core::index::{IndexConfig, PairIndex};
use asterope_core::lis::Solution;
use asterope_core::math::{Quat, Vec3, ARCSEC, DEG};
use asterope_core::pipeline::SolverConfig;
use asterope_core::sim::{simulate, SensorModel};
use asterope_core::track::{propagate, TrackEvent};

use crate::args::Args;
use crate::frame_io::{self, Image};

/// The catalog built into the command. A copy of `data/hip65.astc`, kept
/// inside the crate so that the published package holds it; a test
/// compares the two.
pub const CATALOG: &[u8] = include_bytes!("../data/hip65.astc");
pub const STARS_PER_FIELD: f64 = 25.0;
pub const BLEND_PIXELS: f64 = 4.0;

type CommandResult = Result<(), Box<dyn Error>>;

fn text(error: impl std::fmt::Display) -> Box<dyn Error> {
    error.to_string().into()
}

fn load_catalog(args: &Args) -> Result<Catalog, Box<dyn Error>> {
    let catalog = match args.text("catalog") {
        Some(path) => Catalog::parse(&fs::read(path)?),
        None => Catalog::parse(CATALOG),
    }
    .map_err(text)?;
    Ok(match args.value::<f64>("epoch")? {
        Some(year) => catalog.at_epoch(year),
        None => catalog,
    })
}

fn raw_size(args: &Args) -> Result<Option<(u32, u32)>, Box<dyn Error>> {
    Ok(match (args.value("width")?, args.value("height")?) {
        (Some(width), Some(height)) => Some((width, height)),
        _ => None,
    })
}

fn distortion(args: &Args) -> Result<Distortion, Box<dyn Error>> {
    Ok(Distortion {
        k1: args.value_or("k1", 0.0)?,
        k2: args.value_or("k2", 0.0)?,
        k3: 0.0,
        p1: args.value_or("p1", 0.0)?,
        p2: args.value_or("p2", 0.0)?,
    })
}

/// A camera as written by `asterope calibrate`: `key = value` lines.
pub fn camera_from_toml(source: &str) -> Result<Camera, Box<dyn Error>> {
    let mut values = std::collections::HashMap::new();
    for line in source.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if let Some((key, value)) = line.split_once('=') {
            values.insert(key.trim().to_string(), value.trim().parse::<f64>()?);
        }
    }
    let need = |key: &str| {
        values
            .get(key)
            .copied()
            .ok_or(format!("camera file lacks `{key}`"))
    };
    let get = |key: &str| values.get(key).copied().unwrap_or(0.0);
    let (width, height, fx) = (need("width")? as u32, need("height")? as u32, need("fx")?);
    let fov_x = 2.0 * (0.5 * f64::from(width) / fx).atan();
    let nominal = Camera::from_fov(width, height, fov_x).map_err(text)?;
    Ok(Camera {
        fx,
        fy: values.get("fy").copied().unwrap_or(fx),
        cx: need("cx")?,
        cy: need("cy")?,
        projection: if get("equidistant") != 0.0 {
            Projection::Equidistant
        } else {
            Projection::Gnomonic
        },
        distortion: Distortion {
            k1: get("k1"),
            k2: get("k2"),
            k3: get("k3"),
            p1: get("p1"),
            p2: get("p2"),
        },
        ..nominal
    })
}

fn camera_to_toml(fit: &Calibration) -> String {
    let c = &fit.camera;
    let d = c.distortion;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Asterope camera, fitted to {} stars in {} frames; residual {:.3} px RMS.",
        fit.stars, fit.frames, fit.rms_px
    );
    let _ = writeln!(out, "# Pixel (0, 0) is the center of the top-left pixel.");
    let _ = writeln!(out, "width = {}\nheight = {}", c.width, c.height);
    let _ = writeln!(
        out,
        "fx = {:.4}\nfy = {:.4}\ncx = {:.3}\ncy = {:.3}",
        c.fx, c.fy, c.cx, c.cy
    );
    let _ = writeln!(
        out,
        "k1 = {:.6}\nk2 = {:.6}\nk3 = {:.6}\np1 = {:.7}\np2 = {:.7}",
        d.k1, d.k2, d.k3, d.p1, d.p2
    );
    let _ = writeln!(
        out,
        "equidistant = {}",
        u8::from(c.projection == Projection::Equidistant)
    );
    let _ = writeln!(out, "fov_x_deg = {:.4}", c.fov_x() / DEG);
    out
}

/// The camera from `--camera FILE`, or from `--fov` and the distortion
/// options; `None` when neither is given.
fn known_camera(args: &Args, width: u32, height: u32) -> Result<Option<Camera>, Box<dyn Error>> {
    if let Some(path) = args.text("camera") {
        let camera = camera_from_toml(&fs::read_to_string(path)?)?;
        if (camera.width, camera.height) != (width, height) {
            return Err(format!(
                "{path} describes a {} x {} sensor but the frame is {width} x {height}",
                camera.width, camera.height
            )
            .into());
        }
        return Ok(Some(camera));
    }
    match args.value::<f64>("fov")? {
        Some(fov) => {
            let camera = if args.switch("equidistant") {
                Camera::from_fov_equidistant(width, height, fov * DEG)
            } else {
                Camera::from_fov(width, height, fov * DEG)
            };
            Ok(Some(
                camera.map_err(text)?.with_distortion(distortion(args)?),
            ))
        }
        None => Ok(None),
    }
}

/// Whether the frame is solved as a photograph: wide tolerances from the
/// field of view and a focal-length search. True for PNG and JPEG files and
/// whenever `--preset photo` is given.
fn photographic(args: &Args, image: &Image) -> bool {
    match args.text("preset") {
        Some(preset) => preset == "photo",
        None => image.photograph,
    }
}

/// The preset follows the file type unless `--preset` names one.
fn solver_for(args: &Args, image: &Image) -> Result<SolverConfig, Box<dyn Error>> {
    match args.text("preset") {
        Some("sensor") => Ok(SolverConfig::default()),
        Some("photo") => Ok(SolverConfig::photograph()),
        Some(other) => Err(format!("unknown preset {other}; use sensor or photo").into()),
        None if image.photograph => Ok(SolverConfig::photograph()),
        None => Ok(SolverConfig::default()),
    }
}

struct Solved {
    camera: Camera,
    solution: Option<Solution>,
    detections: usize,
    event: Option<TrackEvent>,
}

fn report(solved: &Solved, elapsed_ms: f64, json: bool) -> String {
    let mut out = String::new();
    let fov = solved.camera.fov_x() / DEG;
    let Some(s) = &solved.solution else {
        if json {
            let _ = writeln!(
                out,
                "{{\"solved\":false,\"detections\":{},\"elapsed_ms\":{elapsed_ms:.2}}}",
                solved.detections
            );
        } else {
            let _ = writeln!(
                out,
                "NO LOCK  {} detections  {elapsed_ms:.1} ms",
                solved.detections
            );
        }
        return out;
    };
    let p = s.pointing;
    if json {
        let stars: Vec<String> = s
            .matches
            .iter()
            .map(|m| {
                format!(
                    "{{\"hip\":{},\"star\":\"{}\",\"magnitude\":{:.2},\"residual_arcsec\":{:.2}}}",
                    m.hip,
                    Designation::of(m.hip),
                    m.mag,
                    m.residual_arcsec
                )
            })
            .collect();
        let _ = writeln!(
            out,
            "{{\"solved\":true,\"q\":[{:.12},{:.12},{:.12},{:.12}],\"ra_deg\":{:.6},\"dec_deg\":{:.6},\"roll_deg\":{:.6},\
             \"fov_x_deg\":{fov:.4},\"equidistant\":{},\"k1\":{:.6},\"rms_arcsec\":{:.3},\"matches\":{},\"detections\":{},\
             \"chance_probability\":{:.3e},\"elapsed_ms\":{elapsed_ms:.2},\"stars\":[{}]}}",
            s.q.x,
            s.q.y,
            s.q.z,
            s.q.w,
            p.ra / DEG,
            p.dec / DEG,
            p.roll / DEG,
            solved.camera.projection == Projection::Equidistant,
            solved.camera.distortion.k1,
            s.rms_arcsec,
            s.matches.len(),
            solved.detections,
            s.chance_probability,
            stars.join(",")
        );
        return out;
    }
    let lens = match solved.camera.projection {
        Projection::Gnomonic => "",
        Projection::Equidistant => "  equidistant lens",
    };
    let label = match solved.event {
        Some(TrackEvent::Tracked) => "TRACK",
        Some(TrackEvent::Acquired) => "ACQUIRE",
        _ => "LOCK",
    };
    let _ = writeln!(
        out,
        "{label}  {} of {} detections matched  {elapsed_ms:.1} ms",
        s.matches.len(),
        solved.detections
    );
    let _ = writeln!(
        out,
        "RA {:.4} deg  Dec {:+.4} deg  Roll {:.4} deg",
        p.ra / DEG,
        p.dec / DEG,
        p.roll / DEG
    );
    let _ = writeln!(
        out,
        "q (ICRS -> sensor, scalar last)  {:+.9} {:+.9} {:+.9} {:+.9}",
        s.q.x, s.q.y, s.q.z, s.q.w
    );
    let _ = writeln!(
        out,
        "field of view {fov:.3} deg  k1 {:+.4}{lens}  residual RMS {:.2}\"  chance match {:.1e}",
        solved.camera.distortion.k1, s.rms_arcsec, s.chance_probability
    );
    if let Some(sigma) = s.sigma {
        let _ = writeln!(
            out,
            "1-sigma about x, y, boresight  {:.1}\" {:.1}\" {:.1}\"",
            sigma.about_x / ARCSEC,
            sigma.about_y / ARCSEC,
            sigma.about_boresight / ARCSEC
        );
    }
    let mut stars = s.matches.clone();
    stars.sort_by(|a, b| a.mag.total_cmp(&b.mag));
    let _ = writeln!(out, "{:<18} {:>6}  residual\"", "star", "mag");
    for m in &stars {
        let _ = writeln!(
            out,
            "{:<18} {:>6.2} {:>9.1}",
            Designation::of(m.hip),
            m.mag,
            m.residual_arcsec
        );
    }
    out
}

/// `asterope solve <image> [--fov DEG] [--index FILE --catalog FILE] ...`
pub fn solve(args: &[String]) -> CommandResult {
    let args = Args::parse(
        args,
        &[
            "fov",
            "width",
            "height",
            "catalog",
            "index",
            "epoch",
            "preset",
            "scale-span",
            "k1",
            "k2",
            "p1",
            "p2",
            "hint",
            "camera",
            "hdu",
            "tolerance",
            "match-radius",
        ],
        &["json", "equidistant", "mirrored"],
    )?;
    let [path] = args.positional() else {
        return Err("usage: asterope solve <image> [--fov DEG] [options]".into());
    };
    let mut image = frame_io::load(Path::new(path), raw_size(&args)?, args.text("hdu"))?;
    if args.switch("mirrored") {
        image.pixels = asterope_core::detect::mirrored(image.width as usize, &image.pixels);
    }
    let catalog = load_catalog(&args)?;
    let mut solver = solver_for(&args, &image)?;
    let frame = Frame::new(image.width, image.height, &image.pixels).map_err(text)?;
    let known = known_camera(&args, image.width, image.height)?;
    let start = Instant::now();

    let solved = if let Some(index_path) = args.text("index") {
        let camera = known.ok_or("--index needs the camera it was built for: --fov or --camera")?;
        let bytes = fs::read(index_path)?;
        let index = PairIndex::from_bytes(&bytes).map_err(text)?;
        let mut tracker = StarTracker::new(camera, catalog, index).map_err(text)?;
        tracker.set_solver(solver);
        let Fix {
            solution,
            detections,
            ..
        } = tracker.solve(&image.pixels).map_err(text)?;
        Solved {
            camera,
            solution,
            detections,
            event: None,
        }
    } else {
        let detected = detect(&frame, &solver.detect).map_err(text)?;
        let found = match known {
            Some(camera) => {
                if photographic(&args, &image) {
                    (solver.tolerance_px, solver.match_radius_px) = auto_tolerances(&camera);
                }
                if let Some(tolerance) = args.value::<f64>("tolerance")? {
                    solver.tolerance_px = tolerance;
                    solver.match_radius_px = args.value_or("match-radius", 1.4 * tolerance)?;
                }
                let calibrated = args.text("camera").is_some();
                let span = args.value_or(
                    "scale-span",
                    if image.photograph && !calibrated {
                        0.1
                    } else {
                        0.0
                    },
                )?;
                let search = ScaleSearch {
                    span,
                    refine: span > 0.0,
                };
                let index = PairIndex::for_camera(
                    &catalog,
                    &widest_camera(&camera, &search),
                    STARS_PER_FIELD,
                    BLEND_PIXELS,
                )
                .map_err(text)?;
                solve_with_scale_search(
                    &detected.detections,
                    &camera,
                    &index,
                    &catalog,
                    &solver,
                    &search,
                )
            }
            None => solve_unknown_fov(
                &detected.detections,
                (image.width, image.height),
                &catalog,
                &solver,
                args.value_or("hint", 20.0)? * DEG,
                (STARS_PER_FIELD, BLEND_PIXELS),
            )
            .map_err(text)?,
        };
        Solved {
            camera: found.camera,
            solution: found.lis.solution,
            detections: detected.detections.len(),
            event: None,
        }
    };
    print!(
        "{}",
        report(
            &solved,
            start.elapsed().as_secs_f64() * 1e3,
            args.switch("json")
        )
    );
    Ok(())
}

fn parse_rate(text: &str) -> Result<Vec3, Box<dyn Error>> {
    let parts: Vec<f64> = text
        .split(',')
        .map(|part| part.trim().parse())
        .collect::<Result<_, _>>()?;
    match parts.as_slice() {
        [x, y, z] => Ok(Vec3::new(*x, *y, *z).scale(DEG)),
        _ => Err("--rate takes three comma-separated values in degrees per second".into()),
    }
}

fn numbered(path: &Path, index: u32) -> PathBuf {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("frame");
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("fits");
    path.with_file_name(format!("{stem}-{index:04}.{extension}"))
}

/// `asterope simulate --out FILE [--ra --dec --roll --fov --size ...]`
pub fn simulate_frames(args: &[String]) -> CommandResult {
    let args = Args::parse(
        args,
        &[
            "out",
            "ra",
            "dec",
            "roll",
            "fov",
            "size",
            "width",
            "height",
            "exposure",
            "psf-sigma",
            "read-noise",
            "seed",
            "frames",
            "dt",
            "rate",
            "catalog",
            "epoch",
            "k1",
            "k2",
            "p1",
            "p2",
        ],
        &["smear"],
    )?;
    let out = Path::new(
        args.text("out")
            .ok_or("usage: asterope simulate --out <frame.fits> [options]")?,
    );
    let size: u32 = args.value_or("size", 1024)?;
    let width = args.value_or("width", size)?;
    let height = args.value_or("height", size)?;
    let camera = Camera::from_fov(width, height, args.value_or("fov", 14.0)? * DEG)
        .map_err(text)?
        .with_distortion(distortion(&args)?);
    let catalog = load_catalog(&args)?;
    let rate = match args.text("rate") {
        Some(value) => parse_rate(value)?,
        None => Vec3::ZERO,
    };
    let base = SensorModel::default();
    let sensor = SensorModel {
        exposure: args.value_or("exposure", base.exposure)?,
        psf_sigma: args.value_or("psf-sigma", base.psf_sigma)?,
        read_noise: args.value_or("read-noise", base.read_noise)?,
        body_rate: if args.switch("smear") {
            rate
        } else {
            Vec3::ZERO
        },
        ..base
    };
    let frames: u32 = args.value_or("frames", 1)?;
    let dt: f64 = args.value_or("dt", 0.1)?;
    let seed: u64 = args.value_or("seed", 1)?;
    let start = attitude_from_radec_roll(RaDecRoll {
        ra: args.value_or("ra", 83.822)? * DEG,
        dec: args.value_or("dec", -5.391)? * DEG,
        roll: args.value_or("roll", 0.0)? * DEG,
    });
    let mut attitude = Quat::from_matrix(&start).ok_or("pointing is not a valid attitude")?;
    for index in 0..frames {
        let frame = simulate(
            catalog.stars(),
            &attitude.to_matrix(),
            &camera,
            &sensor,
            seed.wrapping_add(u64::from(index)),
        )
        .map_err(text)?;
        let path = if frames == 1 {
            out.to_path_buf()
        } else {
            numbered(out, index)
        };
        frame_io::save(
            &path,
            frame.width,
            frame.height,
            &frame.pixels,
            sensor.max_dn(),
        )?;
        println!("{}  {} stars in frame", path.display(), frame.truth.len());
        attitude = propagate(attitude, rate, dt);
    }
    Ok(())
}

/// `asterope build-index <catalog.astc> <out.asti> --fov DEG [options]`
pub fn build_index(args: &[String]) -> CommandResult {
    let args = Args::parse(
        args,
        &[
            "fov",
            "width",
            "height",
            "stars-per-field",
            "mag",
            "blend-px",
            "max-angle",
            "catalog-out",
            "catalog-margin",
        ],
        &[],
    )?;
    let [input, output] = args.positional() else {
        return Err(
            "usage: asterope build-index <catalog.astc> <out.asti> --fov DEG [options]".into(),
        );
    };
    let fov: f64 = args.value("fov")?.ok_or("--fov <deg> is required")?;
    let catalog = Catalog::parse(&fs::read(input)?).map_err(text)?;
    let camera = Camera::from_fov(
        args.value_or("width", 1024)?,
        args.value_or("height", 1024)?,
        fov * DEG,
    )
    .map_err(text)?;
    let mut config = IndexConfig::for_camera(
        &catalog,
        &camera,
        args.value_or("stars-per-field", STARS_PER_FIELD)?,
        args.value_or("blend-px", BLEND_PIXELS)?,
    )
    .map_err(text)?;
    if let Some(mag) = args.value::<f64>("mag")? {
        config.star_count = catalog.count_brighter_than(mag);
    }
    if let Some(max_angle) = args.value::<f64>("max-angle")? {
        config.max_angle = max_angle * DEG;
    }
    let index = PairIndex::build(&catalog, &config).map_err(text)?;
    let bytes = index.as_bytes();
    PairIndex::from_bytes(bytes).map_err(text)?;
    fs::write(output, bytes)?;
    println!(
        "{} stars (V <= {:.2}), {} pairs up to {:.2} deg, {} bytes -> {}",
        index.star_count(),
        index.mag_limit(),
        index.pair_count(),
        config.max_angle / DEG,
        bytes.len(),
        output
    );
    if let Some(path) = args.text("catalog-out") {
        let onboard =
            catalog.brighter_than(index.mag_limit() + args.value_or("catalog-margin", 0.5)?);
        let bytes = onboard.encode();
        fs::write(path, &bytes)?;
        println!(
            "{} stars (V <= {:.2}), {} bytes -> {}",
            onboard.len(),
            onboard.mag_limit(),
            bytes.len(),
            path
        );
    }
    Ok(())
}

fn frame_files(inputs: &[String]) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut files = Vec::new();
    for input in inputs {
        let path = PathBuf::from(input);
        if path.is_dir() {
            for entry in fs::read_dir(&path)? {
                let entry = entry?.path();
                let known = matches!(
                    entry
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(str::to_ascii_lowercase)
                        .as_deref(),
                    Some("fits" | "fit" | "fts" | "raw" | "bin" | "png" | "jpg" | "jpeg")
                );
                if entry.is_file() && known {
                    files.push(entry);
                }
            }
        } else {
            files.push(path);
        }
    }
    files.sort();
    if files.is_empty() {
        return Err("no frames found".into());
    }
    Ok(files)
}

/// `asterope replay <dir or frames...> --fov DEG [--dt S] [--out log.csv]`
///
/// Feeds the frames, in name order, to one tracker and writes the attitude log.
pub fn replay(args: &[String]) -> CommandResult {
    let args = Args::parse(
        args,
        &[
            "fov", "dt", "out", "width", "height", "catalog", "index", "epoch", "preset", "k1",
            "k2", "p1", "p2", "camera", "hdu",
        ],
        &["equidistant"],
    )?;
    let files = frame_files(args.positional())?;
    let dt: f64 = args.value_or("dt", 0.1)?;
    let raw = raw_size(&args)?;
    let first = frame_io::load(&files[0], raw, args.text("hdu"))?;
    let camera = known_camera(&args, first.width, first.height)?
        .ok_or("--fov <deg> or --camera <file> is required")?;
    let catalog = load_catalog(&args)?;
    let mut solver = solver_for(&args, &first)?;
    if photographic(&args, &first) {
        (solver.tolerance_px, solver.match_radius_px) = auto_tolerances(&camera);
    }
    let stored;
    let index = match args.text("index") {
        Some(path) => {
            stored = fs::read(path)?;
            PairIndex::from_bytes(&stored).map_err(text)?
        }
        None => {
            PairIndex::for_camera(&catalog, &camera, STARS_PER_FIELD, BLEND_PIXELS).map_err(text)?
        }
    };
    let mut tracker = StarTracker::new(camera, catalog, index).map_err(text)?;
    tracker.set_solver(solver);

    let mut log = String::from(
        "frame,time_s,state,q1,q2,q3,q4,ra_deg,dec_deg,roll_deg,rate_x_deg_s,rate_y_deg_s,rate_z_deg_s,matches,detections,rms_arcsec\n",
    );
    let mut counts = [0u32; 4];
    for (number, file) in files.iter().enumerate() {
        let image = if number == 0 {
            frame_io::load(file, raw, args.text("hdu"))?
        } else {
            let next = frame_io::load(file, raw, args.text("hdu"))?;
            if (next.width, next.height) != (first.width, first.height) {
                return Err(format!("{} has a different size", file.display()).into());
            }
            next
        };
        let fix = tracker.update(&image.pixels, dt).map_err(text)?;
        let (state, slot) = match fix.event {
            TrackEvent::Acquired => ("acquired", 0),
            TrackEvent::Tracked => ("tracked", 1),
            TrackEvent::Coasting => ("coasting", 2),
            TrackEvent::Lost => ("lost", 3),
        };
        counts[slot] += 1;
        let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let rate = fix.body_rate.scale(1.0 / DEG);
        let _ = write!(log, "{name},{:.4},{state},", number as f64 * dt);
        match &fix.solution {
            Some(s) => {
                let _ = writeln!(
                    log,
                    "{:.9},{:.9},{:.9},{:.9},{:.5},{:.5},{:.5},{:.4},{:.4},{:.4},{},{},{:.2}",
                    s.q.x,
                    s.q.y,
                    s.q.z,
                    s.q.w,
                    s.pointing.ra / DEG,
                    s.pointing.dec / DEG,
                    s.pointing.roll / DEG,
                    rate.x,
                    rate.y,
                    rate.z,
                    s.matches.len(),
                    fix.detections,
                    s.rms_arcsec
                );
            }
            None => {
                let _ = writeln!(log, ",,,,,,,,,,0,{},", fix.detections);
            }
        }
    }
    match args.text("out") {
        Some(path) => fs::write(path, &log)?,
        None => print!("{log}"),
    }
    eprintln!(
        "{} frames: {} acquired, {} tracked, {} coasting, {} lost",
        files.len(),
        counts[0],
        counts[1],
        counts[2],
        counts[3]
    );
    Ok(())
}

/// Matched stars of one frame solved with `camera`, or `None` without a lock.
fn calibration_frame(
    image: &Image,
    camera: &Camera,
    catalog: &Catalog,
    solver: &SolverConfig,
    span: f64,
) -> Result<Option<(CalibrationFrame, Camera)>, Box<dyn Error>> {
    let frame = Frame::new(image.width, image.height, &image.pixels).map_err(text)?;
    let detected = detect(&frame, &solver.detect).map_err(text)?;
    let search = ScaleSearch {
        span,
        refine: span > 0.0,
    };
    let index = PairIndex::for_camera(
        catalog,
        &widest_camera(camera, &search),
        STARS_PER_FIELD,
        BLEND_PIXELS,
    )
    .map_err(text)?;
    let found = solve_with_scale_search(
        &detected.detections,
        camera,
        &index,
        catalog,
        solver,
        &search,
    );
    let Some(solution) = found.lis.solution else {
        return Ok(None);
    };
    let stars = solution
        .matches
        .iter()
        .filter_map(|m| {
            let detection = detected.detections.get(m.detection as usize)?;
            Some(CalibrationStar {
                u: detection.x,
                v: detection.y,
                reference: catalog.stars().get(m.catalog_index as usize)?.dir,
            })
        })
        .collect();
    Ok(Some((
        CalibrationFrame {
            attitude: solution.attitude,
            stars,
        },
        found.camera,
    )))
}

/// `asterope calibrate <dir or frames...> --fov DEG [--k2] [--tangential] [--fixed-center] [--out camera.toml]`
///
/// Solves every frame with the approximate field of view, fits the camera
/// to all matched stars, then repeats once with the fitted camera so that
/// stars the first pass could not match are used as well.
pub fn calibrate_camera(args: &[String]) -> CommandResult {
    let args = Args::parse(
        args,
        &[
            "fov",
            "out",
            "width",
            "height",
            "catalog",
            "epoch",
            "preset",
            "scale-span",
        ],
        &["k2", "tangential", "fixed-center"],
    )?;
    let files = frame_files(args.positional())?;
    let fov: f64 = args
        .value("fov")?
        .ok_or("--fov <deg> is required (an approximate value is enough)")?;
    let raw = raw_size(&args)?;
    let catalog = load_catalog(&args)?;
    let model = CalibrationModel {
        principal_point: !args.switch("fixed-center"),
        k2: args.switch("k2"),
        tangential: args.switch("tangential"),
    };
    let images: Vec<Image> = files
        .iter()
        .map(|file| frame_io::load(file, raw, args.text("hdu")))
        .collect::<Result<_, _>>()?;
    let first = images.first().ok_or("no frames found")?;
    let mut solver = solver_for(&args, first)?;
    let nominal = Camera::from_fov(first.width, first.height, fov * DEG).map_err(text)?;
    if photographic(&args, first) {
        (solver.tolerance_px, solver.match_radius_px) = auto_tolerances(&nominal);
    }

    let mut camera = nominal;
    let mut fit = None;
    for (pass, span) in [args.value_or("scale-span", 0.1)?, 0.0]
        .into_iter()
        .enumerate()
    {
        let mut frames = Vec::new();
        let mut start = camera;
        for (image, file) in images.iter().zip(&files) {
            if (image.width, image.height) != (first.width, first.height) {
                return Err(format!("{} has a different size", file.display()).into());
            }
            if let Some((frame, refined)) =
                calibration_frame(image, &camera, &catalog, &solver, span)?
            {
                if frames.is_empty() && pass == 0 {
                    start = refined;
                }
                frames.push(frame);
            }
        }
        let result = calibrate(&start, &frames, &model)
            .map_err(|e| format!("{e}: {} of {} frames solved", frames.len(), files.len()))?;
        eprintln!(
            "pass {}: {} of {} frames solved, {} stars, residual {:.3} -> {:.3} px RMS",
            pass + 1,
            result.frames,
            files.len(),
            result.stars,
            result.initial_rms_px,
            result.rms_px
        );
        camera = result.camera;
        fit = Some(result);
    }
    let fit = fit.ok_or("calibration did not run")?;
    let c = &fit.camera;
    let d = c.distortion;
    let values = [c.fx, c.cx, c.cy, d.k1, d.k2, d.p1, d.p2];
    println!("parameter        value        1-sigma");
    for ((name, value), sigma) in PARAMETER_NAMES.iter().zip(values).zip(fit.sigma) {
        if sigma > 0.0 {
            println!("{name:<8} {value:>13.6} {sigma:>14.6}");
        } else {
            println!("{name:<8} {value:>13.6}          fixed");
        }
    }
    println!("field of view {:.4} deg", c.fov_x() / DEG);
    let toml = camera_to_toml(&fit);
    match args.text("out") {
        Some(path) => {
            fs::write(path, &toml)?;
            println!("wrote {path}");
        }
        None => print!("\n{toml}"),
    }
    Ok(())
}

/// Rotation of the Earth against the stars, radians per second.
const SIDEREAL_RATE: f64 = 7.292_115_9e-5;

/// Seconds since midnight of an ISO time such as `2023-04-27T08:51:44.78`.
fn seconds_of_day(stamp: &str) -> Option<f64> {
    let clock = stamp.trim().split('T').nth(1)?;
    let mut parts = clock.split(':');
    let hours: f64 = parts.next()?.parse().ok()?;
    let minutes: f64 = parts.next()?.parse().ok()?;
    let seconds: f64 = parts.next()?.parse().ok()?;
    Some(3600.0 * hours + 60.0 * minutes + seconds)
}

/// Middle of the 10.24 s a four-frame file covers, in seconds since
/// midnight, from its name: `FF_<station>_<yyyymmdd>_<hhmmss>_<ms>_<frame>`.
fn four_frame_mid_time(path: &Path) -> Option<f64> {
    let name = path.file_name()?.to_str()?;
    let mut parts = name.split('_').skip(3);
    let clock = parts.next()?;
    let millis: f64 = parts.next()?.parse().ok()?;
    let field = |range: core::ops::Range<usize>| clock.get(range)?.parse::<f64>().ok();
    Some(3600.0 * field(0..2)? + 60.0 * field(2..4)? + field(4..6)? + millis / 1000.0 + 5.12)
}

/// Zenith of the observing site at the start of the observation, from the
/// header of a measurement file.
fn ecsv_site(table: &str) -> Option<Vec3> {
    let value = |key: &str| {
        table
            .lines()
            .find_map(|line| {
                line.strip_prefix("# - {")?
                    .strip_prefix(key)?
                    .strip_prefix(": ")
            })
            .map(|rest| rest.trim_end_matches('}').trim_matches('\''))
    };
    let latitude: f64 = value("obs_latitude")?.parse().ok()?;
    let longitude: f64 = value("obs_longitude")?.parse().ok()?;
    let stamp = value("isodate_start_obs")?;
    let (date, _) = stamp.split_once('T')?;
    let mut parts = date.split('-').map(|part| part.parse::<i64>().ok());
    let (year, month, day) = (parts.next()??, parts.next()??, parts.next()??);
    // Julian day number of the date at noon (Fliegel and Van Flandern 1968).
    let a = (month - 14) / 12;
    let noon = day - 32_075 + 1461 * (year + 4800 + a) / 4 + 367 * (month - 2 - 12 * a) / 12
        - 3 * ((year + 4900 + a) / 100) / 4;
    let jd = noon as f64 - 0.5 + seconds_of_day(stamp)? / 86_400.0;
    Some(asterope_core::solar::zenith(
        jd,
        latitude * DEG,
        longitude * DEG,
    ))
}

/// `asterope compare-ecsv <frame> <measurements.ecsv> [--hint DEG] [--refraction]`
///
/// Solves the frame without a field of view and compares the result with
/// independent astrometry: a Global Meteor Network ECSV file lists pixel
/// positions together with the right ascension and declination its own
/// calibration assigns to them. Prints the angle between those and the
/// sky positions this solution gives the same pixels.
pub fn compare_ecsv(args: &[String]) -> CommandResult {
    let args = Args::parse(args, &["hint", "hdu", "catalog", "epoch"], &["refraction"])?;
    let [frame_path, ecsv_path] = args.positional() else {
        return Err("usage: asterope compare-ecsv <frame> <measurements.ecsv>".into());
    };
    let image = frame_io::load(Path::new(frame_path), None, args.text("hdu"))?;
    let mut catalog = load_catalog(&args)?;
    let table = fs::read_to_string(ecsv_path)?;
    // With --refraction the stars are put where the atmosphere shows them
    // from the site and at the time the measurement file states, and the
    // sky positions of the pixels are taken back out of it afterwards.
    let zenith = if args.switch("refraction") {
        let site = ecsv_site(&table).ok_or(
            "the measurement file does not state obs_latitude, obs_longitude and isodate_start_obs",
        )?;
        catalog.refract(site);
        Some(site)
    } else {
        None
    };
    let solver = SolverConfig::photograph();
    let frame = Frame::new(image.width, image.height, &image.pixels).map_err(text)?;
    let detected = detect(&frame, &solver.detect).map_err(text)?;
    let found = solve_unknown_fov(
        &detected.detections,
        (image.width, image.height),
        &catalog,
        &solver,
        args.value_or("hint", 60.0)? * DEG,
        (STARS_PER_FIELD, BLEND_PIXELS),
    )
    .map_err(text)?;
    let Some(solution) = found.lis.solution else {
        println!("NO LOCK  {} detections", detected.detections.len());
        return Ok(());
    };
    let to_sky = solution.attitude.transpose();
    let frame_time = four_frame_mid_time(Path::new(frame_path))
        .ok_or("the frame name does not carry a time (FF_<station>_<date>_<hhmmss>_<ms>_...)")?;
    let mut separations = Vec::new();
    let mut rows = table.lines().filter(|line| !line.starts_with('#'));
    let header: Vec<&str> = rows
        .next()
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .collect();
    let column = |name: &str| {
        header
            .iter()
            .position(|h| *h == name)
            .ok_or(format!("the measurement file has no `{name}` column"))
    };
    let (time_at, ra_at, dec_at, x_at, y_at) = (
        column("datetime")?,
        column("ra")?,
        column("dec")?,
        column("x_image")?,
        column("y_image")?,
    );
    for line in rows {
        let fields: Vec<&str> = line.split(',').map(str::trim).collect();
        let number = |at: usize| fields.get(at).and_then(|f| f.parse::<f64>().ok());
        let (Some(row_time), Some(ra), Some(dec), Some(x), Some(y)) = (
            fields.get(time_at).and_then(|f| seconds_of_day(f)),
            number(ra_at),
            number(dec_at),
            number(x_at),
            number(y_at),
        ) else {
            continue;
        };
        let (ra, dec) = (ra * DEG, dec * DEG);
        let theirs = Vec3::new(dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin());
        // The camera is fixed to the ground: a pixel sweeps eastward in right
        // ascension at the sidereal rate between the frame and the measurement.
        let mut elapsed = row_time - frame_time;
        if elapsed > 43_200.0 {
            elapsed -= 86_400.0;
        } else if elapsed < -43_200.0 {
            elapsed += 86_400.0;
        }
        let turn = SIDEREAL_RATE * elapsed;
        let seen = to_sky.mul_vec(found.camera.unproject(x, y));
        let at_frame = zenith.map_or(seen, |z| asterope_core::solar::remove_refraction(seen, z));
        let ours = Vec3::new(
            at_frame.x * turn.cos() - at_frame.y * turn.sin(),
            at_frame.x * turn.sin() + at_frame.y * turn.cos(),
            at_frame.z,
        );
        let off_axis = found.camera.unproject(x, y).angle_to(Vec3::Z);
        separations.push((ours.angle_to(theirs) / DEG, off_axis / DEG));
    }
    if separations.is_empty() {
        return Err("no measurement rows found".into());
    }
    separations.sort_by(|a, b| a.0.total_cmp(&b.0));
    let median = separations[separations.len() / 2].0;
    let worst = separations[separations.len() - 1];
    let off_axis = separations.iter().map(|s| s.1).sum::<f64>() / separations.len() as f64;
    println!(
        "LOCK  {} of {} matched  field {:.2} deg  k1 {:+.3}  {} points at {:.0} deg off axis: median {:.3} deg ({:.1} px), worst {:.3} deg",
        solution.matches.len(),
        detected.detections.len(),
        found.camera.fov_x() / DEG,
        found.camera.distortion.k1,
        separations.len(),
        off_axis,
        median,
        median * DEG / found.camera.pixel_scale(),
        worst.0
    );
    Ok(())
}

/// `asterope design --focal MM [--aperture MM --pixel UM --width PX --height PX --exposure S]`
///
/// The closed-form prediction of the Design bench as JSON, with the star
/// count of every cell of the 72 x 36 sky grid.
pub fn design(args: &[String]) -> CommandResult {
    use asterope_core::design::{evaluate, Exposure, Optics};
    let args = Args::parse(
        args,
        &[
            "focal", "aperture", "pixel", "width", "height", "exposure", "rate",
        ],
        &[],
    )?;
    let optics = Optics {
        focal_length_mm: args.value("focal")?.ok_or("--focal <mm> is required")?,
        aperture_mm: args.value_or("aperture", 14.5)?,
        pixel_um: args.value_or("pixel", 6.0)?,
        width: args.value_or("width", 1024)?,
        height: args.value_or("height", 1024)?,
        transmission: 0.8,
        quantum_efficiency: 0.5,
        bandwidth_nm: 300.0,
    };
    let exposure = Exposure {
        seconds: args.value_or("exposure", 0.1)?,
        psf_sigma_px: 1.2,
        read_noise: 5.0,
        dark_current: 10.0,
        sky_background: 20.0,
        body_rate: args.value_or("rate", 0.0)? * DEG,
        peak_sigma: 5.0,
    };
    let catalog = Catalog::parse(CATALOG).map_err(text)?;
    let report = evaluate(&catalog, &optics, &exposure).map_err(text)?;
    let counts: Vec<String> = report.coverage.counts.iter().map(u16::to_string).collect();
    println!(
        "{{\"focal_mm\":{},\"fov_deg\":{:.3},\"limiting_magnitude\":{:.2},\"stars_per_field\":{:.2},\"solvable\":{:.4},\"cross_arcsec\":{:.3},\"roll_arcsec\":{:.2},\"streak_deg_s\":{:.3},\"counts\":[{}]}}",
        optics.focal_length_mm,
        report.camera.fov_x() / DEG,
        report.limiting_magnitude,
        report.stars_per_field,
        report.coverage.fractions[1],
        report.median_cross_arcsec(),
        report.median_roll_arcsec(),
        report.streak_rate / DEG,
        counts.join(",")
    );
    Ok(())
}

/// Where the Sun, the Earth and the Moon stand to a camera along an orbit,
/// and how much of the orbit they leave it.
pub fn orbit(args: &[String]) -> CommandResult {
    use asterope_core::orbit::{Blinder, Exclusion, Orbit, Pointing, Surroundings};
    let args = Args::parse(
        args,
        &[
            "altitude",
            "inclination",
            "node",
            "date",
            "boresight",
            "spin",
            "sun",
            "earth",
            "moon",
            "orbits",
            "step",
            "csv",
        ],
        &[],
    )?;
    let orbit = Orbit::circular(
        args.value_or("altitude", 550.0)?,
        args.value_or("inclination", 53.0)? * DEG,
        args.value_or("node", 0.0)? * DEG,
        args.value_or("date", 2_460_400.5)?,
    );
    let pointing = match args.text("spin") {
        Some(rate) => Pointing::Spinning {
            start: asterope_core::math::Quat::IDENTITY,
            rate: parse_rate(rate)?,
        },
        None => Pointing::Local {
            boresight: match args.text("boresight") {
                // The same three-number syntax as a rate, without its unit.
                Some(direction) => parse_rate(direction)?.scale(1.0 / DEG),
                None => Vec3::Z,
            },
            roll: 0.0,
        },
    };
    let exclusion = Exclusion {
        sun: args.value_or("sun", 35.0)? * DEG,
        earth_limb: args.value_or("earth", 25.0)? * DEG,
        moon: args.value_or("moon", 10.0)? * DEG,
    };
    let step: f64 = args.value_or("step", 10.0)?;
    let span = orbit.period() * args.value_or("orbits", 1.0)?;
    let positive = |value: f64| value.is_finite() && value > 0.0;
    if !positive(step) || !positive(span) {
        return Err("--step and --orbits must be positive".into());
    }
    let mut csv = String::from(
        "seconds,sun_deg,earth_limb_deg,moon_deg,eclipse,blinded_by
",
    );
    let (mut samples, mut blinded, mut eclipsed) = (0u32, [0u32; 3], 0u32);
    let (mut stretch, mut longest) = (0.0f64, 0.0f64);
    let mut time = 0.0;
    while time < span {
        let (attitude, _) = pointing.attitude(&orbit, time);
        let (position, _) = orbit.state(time);
        let around = Surroundings::of(attitude.to_matrix().r2, position, orbit.julian_date(time));
        let blinder = around.blinder(&exclusion);
        let name = match blinder {
            Some(Blinder::Sun) => "sun",
            Some(Blinder::Earth) => "earth",
            Some(Blinder::Moon) => "moon",
            None => "",
        };
        let _ = writeln!(
            csv,
            "{time:.0},{:.2},{:.2},{:.2},{},{name}",
            around.sun_angle / DEG,
            around.earth_limb_angle / DEG,
            around.moon_angle / DEG,
            u8::from(around.in_eclipse)
        );
        samples += 1;
        eclipsed += u32::from(around.in_eclipse);
        match blinder {
            Some(body) => {
                let slot = match body {
                    Blinder::Sun => 0,
                    Blinder::Earth => 1,
                    Blinder::Moon => 2,
                };
                if let Some(count) = blinded.get_mut(slot) {
                    *count += 1;
                }
                stretch += step;
                longest = longest.max(stretch);
            }
            None => stretch = 0.0,
        }
        time += step;
    }
    if let Some(path) = args.text("csv") {
        fs::write(path, csv)?;
    }
    let share = |count: u32| 100.0 * f64::from(count) / f64::from(samples.max(1));
    let [sun, earth, moon] = blinded;
    println!(
        "period {:.1} min, {samples} samples every {step} s",
        orbit.period() / 60.0
    );
    println!(
        "blinded by the Sun {:.1} %, the Earth {:.1} %, the Moon {:.1} %; clear {:.1} %",
        share(sun),
        share(earth),
        share(moon),
        share(samples - sun - earth - moon)
    );
    println!(
        "longest blinded stretch {longest:.0} s; in the Earth's shadow {:.1} %",
        share(eclipsed)
    );
    Ok(())
}
