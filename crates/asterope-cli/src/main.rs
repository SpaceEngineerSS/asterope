//! `asterope`: solve frames, simulate them, build onboard indexes, replay
//! frame sequences and run the validation suites from the command line.

mod args;
mod commands;
mod frame_io;
mod report;
mod validate;

use std::process::ExitCode;

const USAGE: &str = "\
usage: asterope <command> [arguments]

  solve <image>                 attitude of one frame (FITS, raw, PNG, JPEG)
      --fov DEG                 horizontal field of view; searched 6-100 deg when omitted
      --hint DEG                where an unknown field of view is searched first (20)
      --scale-span F            focal-length search half-width (0.1 photographs, 0 sensor frames)
      --index FILE --catalog FILE   use an onboard index, as flight software does
      --catalog FILE            another catalog, such as data/hip80.astc for narrow fields
      --preset sensor|photo     detection settings; chosen from the file type by default
      --width W --height H      size of a headerless .raw frame
      --hdu NAME                image extension of a FITS file (AVEPIXEL by default
                                for meteor-network four-frame files)
      --k1 --k2 --p1 --p2       lens distortion
      --tolerance PX            allowed mismatch of star separations (from the field of view
                                for photographs; 0.75 for sensor frames)
      --equidistant             the lens maps angle, not its tangent, to distance (wide lenses)
      --mirrored                the image is reversed left to right (a star diagonal); flipped before solving
      --camera FILE             a camera written by calibrate, instead of --fov and distortion
      --epoch YEAR              apply catalog proper motion (default J2000)
      --json                    machine-readable output

  simulate --out FILE           render a sensor frame (.fits, .raw, .png)
      --ra --dec --roll DEG     pointing (Orion, roll 0)
      --fov DEG --size PX       camera (14, 1024); or --width and --height
      --exposure S --psf-sigma PX --read-noise E   sensor (0.1, 1.2, 5)
      --frames N --dt S --rate X,Y,Z   sequence rotating at X,Y,Z deg/s
      --smear                   blur each frame by the rate during the exposure
      --seed N

  build-index <catalog.astc> <out.asti> --fov DEG
      --width W --height H      sensor size (1024)
      --stars-per-field N       sets the faintest indexed magnitude (25)
      --mag M                   or give it directly
      --max-angle DEG           largest indexed separation (frame diagonal)
      --blend-px PX             close pairs excluded from patterns (4)
      --catalog-out FILE --catalog-margin MAG   also write the cut catalog (0.5)

  replay <dir or frames...> --fov DEG
      --dt S                    time between frames (0.1)
      --out FILE                attitude log as CSV (standard output)
      --index FILE --catalog FILE, --preset, --width, --height, --camera, distortion as for solve

  calibrate <dir or frames...> --fov DEG
      fits focal length, principal point and k1 to the matched stars of all frames
      --k2 --tangential         also fit k2, and p1 with p2
      --fixed-center            keep the principal point at the frame center
      --out FILE                write the camera (camera.toml); printed when omitted

  compare-ecsv <frame> <file.ecsv>
      solves the frame blind and compares pixel-to-sky positions with the
      astrometry in a Global Meteor Network measurement file
      --refraction              allow for the atmosphere at the site and time the file states

  design --focal MM             closed-form prediction for a lens, as JSON with the sky grid
      --aperture MM --pixel UM --width PX --height PX --exposure S --rate DEG/S

  orbit                         what blinds a camera along a circular orbit
      --altitude KM --inclination DEG --node DEG --date JD   (550, 53, 0, 2460400.5)
      --boresight X,Y,Z         in the orbit's frame: along track, orbit normal, up (0,0,1)
      --spin X,Y,Z              tumbling at this rate in deg/s instead
      --sun DEG --earth DEG --moon DEG   exclusion angles (35, 25 from the limb, 10)
      --orbits N --step S --csv FILE     (1, 10 s)

  validate                      Monte Carlo suites
      --trials N --seed N --fov DEG --size PX
      --sweep                   field-of-view sweep
      --stress N                stress sweeps with N trials per step
      --profiles N              onboard index profiles with N trials each
      --out NAME                report base name (validation-report)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((command, rest)) = args.split_first() else {
        eprint!("{USAGE}");
        return ExitCode::FAILURE;
    };
    let result = match command.as_str() {
        "solve" => commands::solve(rest),
        "simulate" => commands::simulate_frames(rest),
        "build-index" => commands::build_index(rest),
        "replay" => commands::replay(rest),
        "calibrate" => commands::calibrate_camera(rest),
        "compare-ecsv" => commands::compare_ecsv(rest),
        "design" => commands::design(rest),
        "orbit" => commands::orbit(rest),
        "validate" => match validate::run(rest) {
            Ok(true) => Ok(()),
            Ok(false) => {
                eprintln!("baseline criteria not met");
                return ExitCode::from(2);
            }
            Err(error) => Err(error),
        },
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        other => Err(format!("unknown command {other}\n\n{USAGE}").into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
