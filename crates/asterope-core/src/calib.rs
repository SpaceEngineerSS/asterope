//! Solving frames whose field of view is only approximately known.

use crate::camera::{Camera, Distortion, Projection};
use crate::catalog::Catalog;
use crate::detect::Detection;
use alloc::vec::Vec;

use crate::error::AsteropeError;
use crate::index::PairIndex;
use crate::lis::{solve_from_prior, LisOutput, Solution};
use crate::math::{abs, ceil, ln, solve_linear, tan, DEG};
use crate::pipeline::{
    detection_vectors, identify_detections, SolverConfig, MAX_PYRAMIDS_PER_FRAME,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScaleSearch {
    /// Half-width of the focal-length search as a fraction (0.1 = +/-10 %).
    pub span: f64,
    /// Refine focal length and radial distortion after the first lock.
    pub refine: bool,
}

#[derive(Clone, Debug)]
pub struct ImageSolution {
    /// Camera after the scale search and optional refinement.
    pub camera: Camera,
    pub lis: LisOutput,
    pub scales_tried: u32,
    pub refined: bool,
}

/// Camera with the widest field the scale search can visit; a pair index
/// built for it covers every candidate scale.
pub fn widest_camera(camera: &Camera, search: &ScaleSearch) -> Camera {
    camera.with_focal_scale(1.0 / (1.0 + search.span.max(0.0)))
}

/// Matching tolerances `(pair tolerance, match radius)` in pixels for a
/// photograph. Lens distortion grows roughly with the square of the field
/// angle, so wide fields need a wider window; the floor covers centroid error
/// on compressed images. Mirrored by `autoTolerances` in the web client.
///
/// An equidistant camera already follows a wide lens, so what is left over
/// is small and does not grow with the field: 0.3 % of the long side. The
/// wider window of the pinhole rule would there span most of a degree and
/// make every star pattern ambiguous.
pub fn auto_tolerances(camera: &Camera) -> (f64, f64) {
    let long_side = f64::from(camera.width.max(camera.height));
    let tolerance = match camera.projection {
        Projection::Gnomonic => {
            let half_field = tan(0.5 * camera.fov_x());
            (0.021 * half_field * half_field * long_side).clamp(2.0, 12.0)
        }
        Projection::Equidistant => (0.003 * long_side).clamp(2.0, 6.0),
    };
    let rounded = crate::math::round(tolerance * 10.0) / 10.0;
    (rounded, crate::math::round(rounded * 14.0) / 10.0)
}

/// Fields of view (degrees) tried when the true one is unknown. Each rung is
/// searched over `LADDER_SPAN`, and neighbouring rungs overlap.
pub const FOV_LADDER_DEG: [f64; 8] = [6.0, 9.0, 13.0, 20.0, 30.0, 45.0, 68.0, 100.0];
pub const LADDER_SPAN: f64 = 0.22;
/// Pyramid budget of one ladder rung. A coarse image (a few arcminutes per
/// pixel) yields ten or more chance pyramids per candidate scale, and a rung
/// holds over a hundred scales, so a smaller budget ends the rung before it
/// reaches its far edge.
pub const LADDER_RUNG_BUDGET: u32 = 6000;
/// Fields at least this wide get a second search over the central stars.
const WIDE_FIELD: f64 = 40.0 * DEG;
/// Radius of the central region as a fraction of the half-diagonal. A radial
/// distortion grows with the cube of the radius, so at half the radius it
/// is an eighth of its value in the corners.
const CENTRAL_FRACTION: f64 = 0.5;
const MIN_CENTRAL_STARS: usize = 8;
/// Narrowest ladder field also searched with an equidistant lens.
const EQUIDISTANT_FROM: f64 = 60.0 * DEG;

/// Lens models tried at one field of the ladder: a pinhole, and for wide
/// fields an equidistant lens as well.
pub fn ladder_models(fov_deg: f64) -> [Option<Projection>; 2] {
    [
        Some(Projection::Gnomonic),
        (fov_deg * DEG >= EQUIDISTANT_FROM).then_some(Projection::Equidistant),
    ]
}

/// Undistorted camera for one field and lens model of the ladder.
pub fn ladder_camera(
    width: u32,
    height: u32,
    fov_deg: f64,
    projection: Projection,
) -> Result<Camera, AsteropeError> {
    match projection {
        Projection::Gnomonic => Camera::from_fov(width, height, fov_deg * DEG),
        Projection::Equidistant => Camera::from_fov_equidistant(width, height, fov_deg * DEG),
    }
}

/// Least-squares focal length and `k1` from matched stars, holding the
/// attitude and principal point fixed. With `(x, y)` the ideal normalized
/// coordinates and `r^2 = x^2 + y^2`, the model `u - cx = f x + g x r^2`
/// (and likewise for `v`) is linear in `f` and `g = f k1`.
fn fit_focal_and_k1(
    detections: &[Detection],
    camera: &Camera,
    catalog: &Catalog,
    solution: &Solution,
    fit_distortion: bool,
) -> Option<Camera> {
    let (mut s_aa, mut s_ab, mut s_bb, mut s_au, mut s_bu) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for m in &solution.matches {
        let detection = detections.get(m.detection as usize)?;
        let star = catalog.stars().get(m.catalog_index as usize)?;
        let body = solution.attitude.mul_vec(star.dir);
        let Some((x, y)) = camera.ideal(body) else {
            continue;
        };
        let r2 = x * x + y * y;
        for (a, target) in [(x, detection.x - camera.cx), (y, detection.y - camera.cy)] {
            let b = a * r2;
            s_aa += a * a;
            s_ab += a * b;
            s_bb += b * b;
            s_au += a * target;
            s_bu += b * target;
        }
    }
    let (focal, k1) = if fit_distortion {
        let [f, g] = solve_linear([[s_aa, s_ab], [s_ab, s_bb]], [s_au, s_bu])?;
        (f, g / f)
    } else {
        (s_au / s_aa, 0.0)
    };
    if !(focal > 0.0) || !focal.is_finite() || !(abs(k1) < 2.0) {
        return None;
    }
    Some(Camera {
        fx: focal,
        fy: focal,
        distortion: Distortion {
            k1,
            ..Distortion::NONE
        },
        ..*camera
    })
}

fn is_better(candidate: &Solution, best: &Solution) -> bool {
    candidate.matches.len() > best.matches.len()
        || (candidate.matches.len() == best.matches.len() && candidate.rms_arcsec < best.rms_arcsec)
}

/// Alternates attitude solutions with focal-length / `k1` fits and keeps the
/// calibration that matches the most stars (ties broken by residual).
fn refine(
    detections: &[Detection],
    start: &Camera,
    index: &PairIndex,
    catalog: &Catalog,
    config: &SolverConfig,
    first: LisOutput,
    prior_pyramids: u32,
) -> (Camera, LisOutput, bool) {
    const ROUNDS: usize = 12;
    let mut best_camera = *start;
    let mut best = first;
    let mut refined = false;
    let mut camera = *start;
    let mut current = best.solution.clone();
    let mut pyramids = prior_pyramids;

    for round in 0..ROUNDS {
        let Some(solution) = &current else { break };
        // Radial distortion is only observable once stars span the field.
        let fit_distortion = round >= 1 && solution.matches.len() >= 8;
        let Some(candidate) =
            fit_focal_and_k1(detections, &camera, catalog, solution, fit_distortion)
        else {
            break;
        };
        let settled = abs(candidate.fx / camera.fx - 1.0) < 1e-6
            && abs(candidate.distortion.k1 - camera.distortion.k1) < 1e-6;
        let mut next = identify_detections(
            detections,
            &candidate,
            index,
            catalog,
            config,
            pyramids,
            u32::MAX,
        );
        pyramids = pyramids.saturating_add(next.pyramids_tested);
        // A new pattern search fails while the outer stars are still
        // misplaced by unfitted distortion. Associating again from the
        // attitude already found keeps the matches that fit and picks up
        // the stars the latest camera has brought within reach, so the
        // matched region grows outward from where the lock started.
        let followed = solve_from_prior(
            &detection_vectors(&candidate, detections),
            catalog,
            &solution.attitude,
            2.0 * config.match_radius_px * candidate.pixel_scale(),
            &config.lis_config(&candidate, catalog, pyramids, u32::MAX),
        );
        if let Some(grown) = followed {
            if next.solution.as_ref().is_none_or(|s| is_better(&grown, s)) {
                next.solution = Some(Solution {
                    pyramid: solution.pyramid,
                    ..grown
                });
            }
        }
        camera = candidate;
        if let (Some(new), Some(old)) = (&next.solution, &best.solution) {
            if is_better(new, old) {
                best_camera = candidate;
                best = next.clone();
                refined = true;
            }
        }
        current = next.solution;
        if settled {
            break;
        }
    }
    if sweep_distortion(
        detections,
        catalog,
        config,
        &mut best_camera,
        &mut best,
        pyramids,
    ) {
        refined = true;
    }
    (best_camera, best, refined)
}

/// Stars matched from a known attitude with `camera`, keeping the pyramid
/// of the solution the attitude came from.
fn follow(
    detections: &[Detection],
    catalog: &Catalog,
    config: &SolverConfig,
    camera: &Camera,
    from: &Solution,
    pyramids: u32,
) -> Option<Solution> {
    solve_from_prior(
        &detection_vectors(camera, detections),
        catalog,
        &from.attitude,
        2.0 * config.match_radius_px * camera.pixel_scale(),
        &config.lis_config(camera, catalog, pyramids, u32::MAX),
    )
    .map(|grown| Solution {
        pyramid: from.pyramid,
        ..grown
    })
}

/// Tries radial distortions around the current one and keeps the camera
/// that matches the most stars.
///
/// The alternation in `refine` fits `k1` to the stars already matched. When
/// those all lie near the image center they say little about it, the outer
/// stars stay out of reach and the fit stalls. Stepping `k1` directly and
/// counting matches does not depend on which stars happen to be matched.
fn sweep_distortion(
    detections: &[Detection],
    catalog: &Catalog,
    config: &SolverConfig,
    best_camera: &mut Camera,
    best: &mut LisOutput,
    pyramids: u32,
) -> bool {
    const STEP: f64 = 0.02;
    const STEPS: i32 = 10;
    let mut improved = false;
    for _ in 0..2 {
        let Some(start) = best.solution.clone() else {
            break;
        };
        let center = *best_camera;
        for step in -STEPS..=STEPS {
            let mut camera = center;
            camera.distortion.k1 += STEP * f64::from(step);
            let Some(mut solution) = follow(detections, catalog, config, &camera, &start, pyramids)
            else {
                continue;
            };
            // Polish focal length and `k1` on the stars this guess reached.
            for _ in 0..3 {
                let Some(fitted) = fit_focal_and_k1(detections, &camera, catalog, &solution, true)
                else {
                    break;
                };
                let Some(next) = follow(detections, catalog, config, &fitted, &solution, pyramids)
                else {
                    break;
                };
                if !is_better(&next, &solution) {
                    break;
                }
                camera = fitted;
                solution = next;
            }
            if best
                .solution
                .as_ref()
                .is_some_and(|b| is_better(&solution, b))
            {
                *best_camera = camera;
                best.solution = Some(solution);
                improved = true;
            }
        }
    }
    improved
}

/// A focal-length search that can be advanced a few candidates at a time, so
/// a caller can report progress or stop between steps.
///
/// Candidates run outward from the nominal focal length: `1`, `1 + s`,
/// `1 - s`, `1 + 2s`, ... A scale error `e` shifts a leg spanning the sensor
/// by `e * extent` pixels; stepping by `tolerance / extent` leaves at most
/// half a step of error, which keeps even diagonal legs inside the matching
/// window.
#[derive(Clone, Debug)]
pub struct ScaleSearchState {
    camera: Camera,
    config: SolverConfig,
    search: ScaleSearch,
    step: f64,
    total: u32,
    next: u32,
    triads: u32,
    /// Pyramids verified for this frame so far, including earlier searches.
    pyramids: u32,
    pyramid_cap: u32,
    outcome: Option<ImageSolution>,
}

impl ScaleSearchState {
    /// `prior_pyramids` counts pyramids already verified for the same frame
    /// (they tighten the acceptance test); `budget` is how many more this
    /// search may verify before giving up.
    pub fn new(
        camera: &Camera,
        config: &SolverConfig,
        search: &ScaleSearch,
        prior_pyramids: u32,
        budget: u32,
    ) -> ScaleSearchState {
        let extent = f64::from(camera.width.max(camera.height));
        let step = (config.tolerance_px / extent).max(1e-4);
        let steps = ceil(search.span.max(0.0) / step).clamp(0.0, 1.0e6) as u32;
        ScaleSearchState {
            camera: *camera,
            config: *config,
            search: *search,
            step,
            total: 2 * steps + 1,
            next: 0,
            triads: 0,
            pyramids: prior_pyramids,
            // A wide field is searched twice per scale, once over all stars
            // and once over the central ones.
            pyramid_cap: prior_pyramids.saturating_add(
                if search.refine && camera.fov_x() >= WIDE_FIELD {
                    budget.saturating_mul(2)
                } else {
                    budget
                },
            ),
            outcome: None,
        }
    }

    pub fn total_scales(&self) -> u32 {
        self.total
    }

    pub fn scales_tried(&self) -> u32 {
        self.next
    }

    pub fn triads(&self) -> u32 {
        self.triads
    }

    /// Pyramids verified for the frame so far, including earlier searches.
    pub fn pyramids(&self) -> u32 {
        self.pyramids
    }

    pub fn is_done(&self) -> bool {
        self.outcome.is_some()
    }

    fn give_up(&mut self, mut last: LisOutput) {
        last.triads_tested = self.triads;
        last.pyramids_tested = self.pyramids;
        self.outcome = Some(ImageSolution {
            camera: self.camera,
            lis: last,
            scales_tried: self.next,
            refined: false,
        });
    }

    /// Tries up to `max_scales` more candidates. Returns true once finished.
    pub fn advance(
        &mut self,
        detections: &[Detection],
        index: &PairIndex,
        catalog: &Catalog,
        max_scales: u32,
    ) -> bool {
        for _ in 0..max_scales {
            if self.outcome.is_some() {
                break;
            }
            if self.next >= self.total || self.pyramids >= self.pyramid_cap {
                self.give_up(LisOutput::default());
                break;
            }
            let offset = f64::from(self.next.div_ceil(2));
            let sign = if self.next % 2 == 1 { 1.0 } else { -1.0 };
            let candidate = self
                .camera
                .with_focal_scale(1.0 + sign * offset * self.step);
            self.next += 1;
            let output = identify_detections(
                detections,
                &candidate,
                index,
                catalog,
                &self.config,
                self.pyramids,
                self.pyramid_cap,
            );
            self.triads = self.triads.saturating_add(output.triads_tested);
            self.pyramids = self.pyramids.saturating_add(output.pyramids_tested);
            let output = match output.solution {
                Some(_) => output,
                None => match self.central_lock(detections, &candidate, index, catalog) {
                    Some(central) => central,
                    None => continue,
                },
            };
            let (camera, mut lis, refined) = if self.search.refine {
                refine(
                    detections,
                    &candidate,
                    index,
                    catalog,
                    &self.config,
                    output,
                    self.pyramids,
                )
            } else {
                (candidate, output, false)
            };
            lis.triads_tested = self.triads;
            lis.pyramids_tested = self.pyramids;
            self.outcome = Some(ImageSolution {
                camera,
                lis,
                scales_tried: self.next,
                refined,
            });
        }
        self.outcome.is_some()
    }

    /// On a wide field with unknown distortion, a pattern search over the
    /// stars near the image center, where a radial distortion displaces
    /// them least. The solution refers to the full detection list.
    fn central_lock(
        &mut self,
        detections: &[Detection],
        candidate: &Camera,
        index: &PairIndex,
        catalog: &Catalog,
    ) -> Option<LisOutput> {
        if !self.search.refine || candidate.fov_x() < WIDE_FIELD {
            return None;
        }
        let reach = CENTRAL_FRACTION
            * crate::math::hypot(f64::from(candidate.width), f64::from(candidate.height))
            * 0.5;
        let (kept, central): (Vec<u32>, Vec<Detection>) = detections
            .iter()
            .enumerate()
            .filter(|(_, d)| crate::math::hypot(d.x - candidate.cx, d.y - candidate.cy) <= reach)
            .map(|(i, d)| (i as u32, *d))
            .unzip();
        if central.len() < MIN_CENTRAL_STARS || central.len() == detections.len() {
            return None;
        }
        let mut output = identify_detections(
            &central,
            candidate,
            index,
            catalog,
            &self.config,
            self.pyramids,
            self.pyramid_cap,
        );
        self.triads = self.triads.saturating_add(output.triads_tested);
        self.pyramids = self.pyramids.saturating_add(output.pyramids_tested);
        let solution = output.solution.as_mut()?;
        let original = |i: u32| kept.get(i as usize).copied().unwrap_or(i);
        for m in &mut solution.matches {
            m.detection = original(m.detection);
        }
        solution.pyramid = solution.pyramid.map(|p| p.map(original));
        Some(output)
    }

    /// The result; `None` while the search is still running.
    pub fn outcome(&self) -> Option<&ImageSolution> {
        self.outcome.as_ref()
    }

    /// Ends the search now and returns what it has (no lock if unfinished).
    pub fn finish(mut self) -> ImageSolution {
        if self.outcome.is_none() {
            self.give_up(LisOutput::default());
        }
        self.outcome.unwrap_or(ImageSolution {
            camera: self.camera,
            lis: LisOutput::default(),
            scales_tried: self.next,
            refined: false,
        })
    }
}

/// Tries focal-length scales outward from the nominal value until the star
/// pattern locks.
pub fn solve_with_scale_search(
    detections: &[Detection],
    camera: &Camera,
    index: &PairIndex,
    catalog: &Catalog,
    config: &SolverConfig,
    search: &ScaleSearch,
) -> ImageSolution {
    let mut state = ScaleSearchState::new(camera, config, search, 0, MAX_PYRAMIDS_PER_FRAME);
    while !state.advance(detections, index, catalog, 16) {}
    state.finish()
}

/// Solves a frame whose field of view is unknown. The fields of
/// `FOV_LADDER_DEG` are tried nearest to `hint_fov` (radians) first, each
/// with its own pair index, automatic tolerances and a scale search that
/// overlaps its neighbors. Pyramids verified on earlier rungs count against
/// the acceptance test of later ones.
pub fn solve_unknown_fov(
    detections: &[Detection],
    (width, height): (u32, u32),
    catalog: &Catalog,
    config: &SolverConfig,
    hint_fov: f64,
    (stars_per_field, blend_pixels): (f64, f64),
) -> Result<ImageSolution, AsteropeError> {
    if !(hint_fov > 0.0) {
        return Err(AsteropeError::InvalidParameter);
    }
    let mut ladder = FOV_LADDER_DEG;
    ladder.sort_by(|a, b| abs(ln(a * DEG / hint_fov)).total_cmp(&abs(ln(b * DEG / hint_fov))));
    let search = ScaleSearch {
        span: LADDER_SPAN,
        refine: true,
    };
    let (mut pyramids, mut triads, mut scales) = (0u32, 0u32, 0u32);
    let mut last = None;
    for (fov, projection) in ladder.iter().flat_map(|fov| {
        ladder_models(*fov)
            .into_iter()
            .flatten()
            .map(move |p| (*fov, p))
    }) {
        let camera = ladder_camera(width, height, fov, projection)?;
        let (tolerance_px, match_radius_px) = auto_tolerances(&camera);
        let rung = SolverConfig {
            tolerance_px,
            match_radius_px,
            ..*config
        };
        let index = PairIndex::for_camera(
            catalog,
            &widest_camera(&camera, &search),
            stars_per_field,
            blend_pixels,
        )?;
        let mut state =
            ScaleSearchState::new(&camera, &rung, &search, pyramids, LADDER_RUNG_BUDGET);
        while !state.advance(detections, &index, catalog, 64) {}
        let mut solved = state.finish();
        triads = triads.saturating_add(solved.lis.triads_tested);
        pyramids = solved.lis.pyramids_tested;
        scales = scales.saturating_add(solved.scales_tried);
        solved.lis.triads_tested = triads;
        solved.scales_tried = scales;
        if solved.lis.solution.is_some() {
            return Ok(solved);
        }
        last = Some(solved);
    }
    last.ok_or(AsteropeError::InvalidParameter)
}
