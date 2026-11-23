//! Lost-in-space star identification.
//!
//! Pattern search follows the Pyramid algorithm (Mortari, Samaan, Bruccoleri &
//! Junkins, "The Pyramid Star Identification Technique", Navigation 2004): a
//! triad of observed stars is matched against catalog triangles through
//! k-vector range searches on its three legs, and a fourth star must confirm
//! the triangle through its three additional legs. Every pyramid is then
//! verified against the whole field before a solution is reported.

use alloc::vec;
use alloc::vec::Vec;

use crate::attitude::{
    attitude_sigma, q_method, quest, radec_roll_from_attitude, svd_method, AttitudeSigma,
    Observation, RaDecRoll,
};
use crate::catalog::Catalog;
use crate::index::{IndexView, Pair, PairIndex, PairRange};
use crate::math::{abs, cos, sqrt, Mat3, Quat, Vec3, ARCSEC, PI};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LisConfig {
    /// Tolerance on inter-star angles, radians.
    pub tolerance: f64,
    /// Radius for associating a detection with a predicted catalog star, radians.
    pub match_radius: f64,
    /// Number of brightest detections used to form patterns.
    pub pattern_stars: usize,
    pub max_triads: usize,
    pub min_matches: usize,
    /// Largest accepted probability that the matches beyond the pyramid are
    /// coincidences, after correcting for every pyramid verified so far.
    pub max_chance_probability: f64,
    /// Pyramids already verified for this frame by earlier calls (other
    /// candidate focal lengths); they count toward the correction.
    pub prior_pyramids: u32,
    /// The search gives up once this many pyramids have been verified for
    /// the frame. Each attempt tightens the acceptance test, so a long
    /// search could no longer accept anything but an overwhelming match.
    pub max_pyramids: u32,
    /// Angle from the boresight to the farthest sensor corner, radians.
    pub field_radius: f64,
    /// Sensor solid angle, steradians.
    pub field_solid_angle: f64,
    /// Faintest catalog magnitude considered during verification.
    pub verify_mag_limit: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StarMatch {
    pub detection: u32,
    pub catalog_index: u32,
    pub hip: u32,
    pub mag: f64,
    pub residual_arcsec: f64,
}

#[derive(Clone, Debug)]
pub struct Solution {
    /// Attitude quaternion, ICRS -> sensor, scalar-last.
    pub q: Quat,
    pub attitude: Mat3,
    pub pointing: RaDecRoll,
    pub matches: Vec<StarMatch>,
    /// Detection indices of the confirming pyramid; `None` when the solution
    /// came from a predicted attitude instead of a pattern search.
    pub pyramid: Option<[u32; 4]>,
    pub rms_arcsec: f64,
    pub lambda_max: f64,
    /// Rotation angle between the QUEST and q-method solutions, radians.
    pub quest_q_method_delta: f64,
    /// Rotation angle between the QUEST and SVD solutions, radians.
    pub quest_svd_delta: f64,
    pub sigma: Option<AttitudeSigma>,
    /// Bound on the probability that this solution is a coincidence: the
    /// binomial tail for its extra matches times the number of pyramids
    /// verified before it was accepted (Bonferroni).
    pub chance_probability: f64,
}

impl Solution {
    /// Refits the attitude to the same matches with one weight per
    /// detection (`weights[detection]`, the inverse variance of its
    /// centroid in any common unit). Matches and the acceptance statistics
    /// are kept; the attitude, residuals and uncertainty are replaced.
    /// Returns `false`, leaving the solution as it was, when the refit fails.
    pub fn reweigh(&mut self, bodies: &[Vec3], catalog: &Catalog, weights: &[f64]) -> bool {
        let observations: Option<Vec<Observation>> = self
            .matches
            .iter()
            .map(|m| {
                Some(Observation {
                    body: *bodies.get(m.detection as usize)?,
                    reference: catalog.stars().get(m.catalog_index as usize)?.dir,
                    weight: *weights.get(m.detection as usize)?,
                })
            })
            .collect();
        let Some(mut observations) = observations else {
            return false;
        };
        let mean =
            observations.iter().map(|o| o.weight).sum::<f64>() / observations.len().max(1) as f64;
        if !(mean > 0.0 && mean.is_finite()) {
            return false;
        }
        for o in &mut observations {
            o.weight /= mean;
        }
        // A centroid spoiled by a hot pixel or a particle track keeps the
        // weight its brightness earned it and would drag the fit. After
        // each fit, a star whose residual is more than three times what its
        // weight allows, on a scale taken from the median star so that the
        // spoiled ones do not set it, has its weight cut in proportion
        // (Huber's weights).
        let given: Vec<f64> = observations.iter().map(|o| o.weight).collect();
        let mut fit = match quest(&observations) {
            Ok(fit) => fit,
            Err(_) => return false,
        };
        for _ in 0..ROBUST_ROUNDS {
            let attitude = fit.q.to_matrix();
            let squared: Vec<f64> = observations
                .iter()
                .zip(given.iter())
                .map(|(o, w)| {
                    let residual = o.body.angle_to(attitude.mul_vec(o.reference));
                    w * residual * residual
                })
                .collect();
            let mut sorted = squared.clone();
            sorted.sort_by(f64::total_cmp);
            // The median of a squared two-dimensional normal error is
            // 2 ln 2 times its variance per axis.
            let scale = sorted.get(sorted.len() / 2).copied().unwrap_or(0.0) / (2.0 * LN_2);
            if !(scale > 0.0) {
                break;
            }
            for ((o, w), z2) in observations
                .iter_mut()
                .zip(given.iter())
                .zip(squared.iter())
            {
                let z = sqrt(z2 / scale);
                o.weight = if z > ROBUST_LIMIT {
                    w * ROBUST_LIMIT / z
                } else {
                    *w
                };
            }
            fit = match quest(&observations) {
                Ok(fit) => fit,
                Err(_) => return false,
            };
        }
        let attitude = fit.q.to_matrix();
        let (mut sum_sq, mut weighted_sq) = (0.0, 0.0);
        for (m, o) in self.matches.iter_mut().zip(observations.iter()) {
            let residual = o.body.angle_to(attitude.mul_vec(o.reference));
            sum_sq += residual * residual;
            weighted_sq += o.weight * residual * residual;
            m.residual_arcsec = residual / ARCSEC;
        }
        let dof = (2 * observations.len()).saturating_sub(3).max(1) as f64;
        let unit_sigma = sqrt(weighted_sq / dof).max(0.1 * ARCSEC);
        self.quest_q_method_delta = q_method(&observations)
            .map(|(q, _)| q.angle_to(fit.q))
            .unwrap_or(f64::NAN);
        self.quest_svd_delta = svd_method(&observations)
            .map(|q| q.angle_to(fit.q))
            .unwrap_or(f64::NAN);
        self.q = fit.q;
        self.attitude = attitude;
        self.pointing = radec_roll_from_attitude(&attitude);
        self.rms_arcsec = sqrt(sum_sq / observations.len().max(1) as f64) / ARCSEC;
        self.lambda_max = fit.lambda_max;
        self.sigma = attitude_sigma(&observations, unit_sigma);
        true
    }
}

#[derive(Clone, Debug, Default)]
pub struct LisOutput {
    pub solution: Option<Solution>,
    pub triads_tested: u32,
    pub pyramids_tested: u32,
}

const NO_LINK: u32 = u32::MAX;
/// Radius, in match radii, in which stars are gathered again around an
/// accepted attitude that matched few of them.
const REGATHER_RADII: f64 = 4.0;
/// Rounds of cutting the weight of stars that do not fit, and the
/// residual, in units of what a star's weight allows, beyond which it is cut.
const ROBUST_ROUNDS: usize = 3;
const ROBUST_LIMIT: f64 = 3.0;
const LN_2: f64 = core::f64::consts::LN_2;
/// Most rounds of fitting and associating again in the verification.
const MAX_REFINEMENT_ROUNDS: usize = 10;

fn other_end(pair: &Pair, star: u32) -> Option<u32> {
    if pair.a == star {
        Some(pair.b)
    } else if pair.b == star {
        Some(pair.a)
    } else {
        None
    }
}

struct Search<'a> {
    bodies: &'a [Vec3],
    index: IndexView<'a>,
    catalog: &'a Catalog,
    config: &'a LisConfig,
    /// Head of each index star's partner list, valid when its stamp matches.
    head: Vec<u32>,
    stamp: Vec<u32>,
    generation: u32,
    /// `(partner, next link)` arena for the partner lists.
    links: Vec<(u32, u32)>,
    triangles: Vec<[u32; 3]>,
}

const MAX_TRIANGLES: usize = 48;

impl Search<'_> {
    fn leg(&self, a: usize, b: usize) -> Option<f64> {
        Some(self.bodies.get(a)?.angle_to(*self.bodies.get(b)?))
    }

    fn range(&self, angle: f64) -> PairRange<'_> {
        self.index
            .range(angle - self.config.tolerance, angle + self.config.tolerance)
    }

    fn dir(&self, star: u32) -> Option<Vec3> {
        self.index.star(star).map(|s| s.dir)
    }

    /// Catalog triangles whose three legs match the observed triad `(i, j, k)`.
    /// Returns false when the triad is too ambiguous to be useful.
    fn match_triangles(&mut self, i: usize, j: usize, k: usize) -> bool {
        self.triangles.clear();
        let (Some(ij), Some(ik), Some(jk)) = (self.leg(i, j), self.leg(i, k), self.leg(j, k))
        else {
            return false;
        };
        let tolerance = self.config.tolerance;
        let candidates = |angle: f64| self.index.range(angle - tolerance, angle + tolerance).len();

        // The leg with the most candidate pairs is tested directly; the other
        // two meet at the "hub" vertex and are searched through the index.
        // `slots` records where hub, a and b sit in the caller's (i, j, k).
        let (n_ij, n_ik, n_jk) = (candidates(ij), candidates(ik), candidates(jk));
        let (hub, a, b, hub_a, hub_b, a_b, slots) = if n_jk >= n_ij && n_jk >= n_ik {
            (i, j, k, ij, ik, jk, [0usize, 1, 2])
        } else if n_ik >= n_ij {
            (j, i, k, ij, jk, ik, [1, 0, 2])
        } else {
            (k, i, j, ik, jk, ij, [2, 0, 1])
        };
        let (Some(b_hub), Some(b_a), Some(b_b)) =
            (self.bodies.get(hub), self.bodies.get(a), self.bodies.get(b))
        else {
            return false;
        };
        let observed_handedness = b_hub.triple(*b_a, *b_b);
        let (ij, ik, jk) = (hub_a, hub_b, a_b);

        self.build_partners(ik);

        // The third leg is tested directly on the catalog directions.
        let cos_far = cos((jk + tolerance).min(PI));
        let cos_near = cos((jk - tolerance).max(0.0));

        for pair in self.index.range(ij - tolerance, ij + tolerance) {
            for (star_i, star_j) in [(pair.a, pair.b), (pair.b, pair.a)] {
                let slot = star_i as usize;
                if self.stamp.get(slot) != Some(&self.generation) {
                    continue;
                }
                let (Some(ri), Some(rj)) = (self.index.star(star_i), self.index.star(star_j))
                else {
                    continue;
                };
                let mut cursor = self.head.get(slot).copied().unwrap_or(NO_LINK);
                while let Some((star_k, next)) = self.links.get(cursor as usize).copied() {
                    cursor = next;
                    if star_k == star_j {
                        continue;
                    }
                    let Some(rk) = self.index.star(star_k) else {
                        continue;
                    };
                    let alignment = rj.dir.dot(rk.dir);
                    if alignment < cos_far || alignment > cos_near {
                        continue;
                    }
                    // Mirror-image triangles share all three legs; the sign of
                    // the triple product separates them.
                    let handedness = ri.dir.triple(rj.dir, rk.dir);
                    if abs(observed_handedness) > 1e-7 && handedness * observed_handedness < 0.0 {
                        continue;
                    }
                    if self.triangles.len() >= MAX_TRIANGLES {
                        return false;
                    }
                    let mut triangle = [0u32; 3];
                    for (slot, star) in slots.iter().zip([star_i, star_j, star_k]) {
                        if let Some(entry) = triangle.get_mut(*slot) {
                            *entry = star;
                        }
                    }
                    self.triangles.push(triangle);
                }
            }
        }
        true
    }

    /// Builds, for every index star, the list of partners whose separation
    /// from it matches `angle`. A generation stamp makes clearing the list
    /// heads free between calls.
    fn build_partners(&mut self, angle: f64) {
        let tolerance = self.config.tolerance;
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.stamp.fill(0);
            self.generation = 1;
        }
        self.links.clear();
        for pair in self.index.range(angle - tolerance, angle + tolerance) {
            for (from, to) in [(pair.a, pair.b), (pair.b, pair.a)] {
                let slot = from as usize;
                let (Some(stamp), Some(head)) = (self.stamp.get_mut(slot), self.head.get_mut(slot))
                else {
                    continue;
                };
                let previous = if *stamp == self.generation {
                    *head
                } else {
                    NO_LINK
                };
                *stamp = self.generation;
                *head = self.links.len() as u32;
                self.links.push((to, previous));
            }
        }
    }

    /// Fourth catalog star consistent with observed star `r` for a matched triangle.
    fn confirm(&self, triad: [usize; 3], triangle: [u32; 3], r: usize) -> Option<u32> {
        let [i, j, k] = triad;
        let [star_i, star_j, star_k] = triangle;
        let (ir, jr, kr) = (self.leg(i, r)?, self.leg(j, r)?, self.leg(k, r)?);
        let (dir_j, dir_k) = (self.dir(star_j)?, self.dir(star_k)?);
        let tolerance = self.config.tolerance;
        self.range(ir).iter().find_map(|pair| {
            let candidate = other_end(&pair, star_i)?;
            if candidate == star_j || candidate == star_k {
                return None;
            }
            let dir = self.dir(candidate)?;
            (abs(dir.angle_to(dir_j) - jr) <= tolerance
                && abs(dir.angle_to(dir_k) - kr) <= tolerance)
                .then_some(candidate)
        })
    }
}

/// Detections, catalog and tolerances shared by every attitude solve.
#[derive(Clone, Copy)]
struct Scene<'a> {
    bodies: &'a [Vec3],
    catalog: &'a Catalog,
    config: &'a LisConfig,
}

impl Scene<'_> {
    fn observations(&self, pairs: &[(u32, u32)]) -> Vec<Observation> {
        pairs
            .iter()
            .filter_map(|(detection, catalog_index)| {
                Some(Observation {
                    body: *self.bodies.get(*detection as usize)?,
                    reference: self.catalog.stars().get(*catalog_index as usize)?.dir,
                    weight: 1.0,
                })
            })
            .collect()
    }

    /// Associates every detection with the nearest predicted catalog star
    /// within `radius` radians.
    fn associate(&self, attitude: &Mat3, field: &[u32], radius: f64) -> Vec<(u32, u32)> {
        let cos_radius = cos(radius);
        let predicted: Vec<(u32, Vec3)> = field
            .iter()
            .filter_map(|catalog_index| {
                let star = self.catalog.stars().get(*catalog_index as usize)?;
                Some((*catalog_index, attitude.mul_vec(star.dir)))
            })
            .collect();
        let mut best_for_star: Vec<(u32, u32, f64)> = Vec::new();
        for (detection, body) in self.bodies.iter().enumerate() {
            let mut best: Option<(u32, f64)> = None;
            for (catalog_index, direction) in &predicted {
                let alignment = direction.dot(*body);
                if alignment >= cos_radius && best.is_none_or(|(_, a)| alignment > a) {
                    best = Some((*catalog_index, alignment));
                }
            }
            let Some((catalog_index, alignment)) = best else {
                continue;
            };
            match best_for_star
                .iter_mut()
                .find(|(_, c, _)| *c == catalog_index)
            {
                Some(existing) if alignment > existing.2 => {
                    *existing = (detection as u32, catalog_index, alignment);
                }
                Some(_) => {}
                None => best_for_star.push((detection as u32, catalog_index, alignment)),
            }
        }
        best_for_star.iter().map(|(d, c, _)| (*d, *c)).collect()
    }

    /// Solves the attitude starting from `initial`: associates every
    /// detection with the catalog, refits, and applies the acceptance test.
    ///
    /// `seed` holds matches that must survive (the pyramid, or nothing when
    /// starting from a predicted attitude) and `first_radius` is the
    /// association radius of the first pass.
    fn solve_from(
        &self,
        initial: &Mat3,
        seed: &[(u32, u32)],
        pyramid: Option<[u32; 4]>,
        attempts: u32,
        first_radius: f64,
    ) -> Option<Solution> {
        let boresight = initial.r2;
        let margin = 2.0 * first_radius.max(self.config.match_radius);
        let cos_field = cos((self.config.field_radius + margin).min(PI));
        let cos_inside = cos(self.config.field_radius);
        let mut inside = 0usize;
        let field: Vec<u32> = self
            .catalog
            .stars()
            .iter()
            .enumerate()
            .take_while(|(_, s)| s.mag <= self.config.verify_mag_limit)
            .filter(|(_, s)| {
                let alignment = s.dir.dot(boresight);
                if alignment >= cos_inside {
                    inside += 1;
                }
                alignment >= cos_field
            })
            .map(|(i, _)| i as u32)
            .collect();

        let mut matched = self.associate(initial, &field, first_radius);
        if matched.len() < 4 {
            return None;
        }
        // With nothing but the pyramid matched, refining would reproduce the
        // same attitude and the same four matches.
        if !seed.is_empty()
            && matched.len() == seed.len()
            && seed.iter().all(|pair| matched.contains(pair))
        {
            return None;
        }
        let mut solution = quest(&self.observations(&matched)).ok()?;
        // Fit, associate again, and repeat while that brings more stars in.
        // A first fit to stars that lie close together leaves the roll loose
        // and the far stars out of reach; each round pulls in the nearest
        // of them and tightens it. Two rounds are always made.
        for round in 0..MAX_REFINEMENT_ROUNDS {
            let again = self.associate(&solution.q.to_matrix(), &field, self.config.match_radius);
            if again.len() < 4 {
                return None;
            }
            let grew = again.len() > matched.len();
            matched = again;
            solution = quest(&self.observations(&matched)).ok()?;
            if round >= 1 && !grew {
                break;
            }
        }
        if seed.iter().any(|pair| !matched.contains(pair)) {
            return None;
        }

        // Stars per steradian near the boresight times the association disc
        // gives the chance that an unrelated detection lands on a catalog star.
        let cap_solid_angle = 2.0 * PI * (1.0 - cos_inside);
        let density = inside as f64 / cap_solid_angle.max(1e-12);
        let disc = PI * self.config.match_radius * self.config.match_radius;
        let single = (density * disc).min(1.0);
        let extra = matched.len().saturating_sub(seed.len());
        let trials = self.bodies.len().saturating_sub(seed.len());
        // Every verified pyramid is another chance for a coincidence, so the
        // tail probability is scaled by the number of attempts (Bonferroni).
        let chance_probability =
            (binomial_tail(trials, extra, single) * f64::from(attempts.max(1))).min(1.0);
        if matched.len() < self.config.min_matches
            || chance_probability > self.config.max_chance_probability
        {
            return None;
        }

        let observations = self.observations(&matched);
        let attitude = solution.q.to_matrix();
        let mut sum_sq = 0.0;
        let matches: Vec<StarMatch> = matched
            .iter()
            .zip(observations.iter())
            .filter_map(|((detection, catalog_index), o)| {
                let star = self.catalog.stars().get(*catalog_index as usize)?;
                let residual = o.body.angle_to(attitude.mul_vec(o.reference));
                sum_sq += residual * residual;
                Some(StarMatch {
                    detection: *detection,
                    catalog_index: *catalog_index,
                    hip: star.hip,
                    mag: star.mag,
                    residual_arcsec: residual / ARCSEC,
                })
            })
            .collect();
        let rms = sqrt(sum_sq / matches.len().max(1) as f64);
        // Per-star direction error from the fit, floored so a perfect fit of
        // few stars does not claim zero uncertainty.
        let dof = (2 * matches.len()).saturating_sub(3).max(1) as f64;
        let star_sigma = sqrt(sum_sq / dof).max(0.1 * ARCSEC);
        let quest_q_method_delta = q_method(&observations)
            .map(|(q, _)| q.angle_to(solution.q))
            .unwrap_or(f64::NAN);
        let quest_svd_delta = svd_method(&observations)
            .map(|q| q.angle_to(solution.q))
            .unwrap_or(f64::NAN);

        Some(Solution {
            q: solution.q,
            attitude,
            pointing: radec_roll_from_attitude(&attitude),
            matches,
            pyramid,
            rms_arcsec: rms / ARCSEC,
            lambda_max: solution.lambda_max,
            quest_q_method_delta,
            quest_svd_delta,
            sigma: attitude_sigma(&observations, star_sigma),
            chance_probability,
        })
    }
}

impl Search<'_> {
    fn verify(&self, pyramid: [usize; 4], stars: [u32; 4], attempts: u32) -> Option<Solution> {
        let seed: Vec<(u32, u32)> = pyramid
            .iter()
            .zip(stars.iter())
            .filter_map(|(d, s)| Some((*d as u32, self.index.star(*s)?.catalog_index)))
            .collect();
        if seed.len() != 4 {
            return None;
        }
        let scene = self.scene();
        let initial = quest(&scene.observations(&seed)).ok()?.q.to_matrix();
        let [a, b, c, d] = pyramid;
        let pyramid = Some([a as u32, b as u32, c as u32, d as u32]);
        let first =
            scene.solve_from(&initial, &seed, pyramid, attempts, self.config.match_radius)?;
        // A fit that rests on a few stars close together, one of them
        // perhaps mismatched, can be off about the boresight by more than
        // the match radius reaches at the far side of the field. When under
        // half the detections were matched, the stars are gathered again in
        // a wider ring around the attitude found, and the result is kept if
        // it explains more of them.
        if 2 * first.matches.len() < self.bodies.len() {
            let wider = scene.solve_from(
                &first.attitude,
                &[],
                pyramid,
                attempts,
                REGATHER_RADII * self.config.match_radius,
            );
            if let Some(wider) = wider {
                if wider.matches.len() > first.matches.len() {
                    return Some(wider);
                }
            }
        }
        Some(first)
    }

    fn scene(&self) -> Scene<'_> {
        Scene {
            bodies: self.bodies,
            catalog: self.catalog,
            config: self.config,
        }
    }
}

/// Solves the attitude from a predicted one, without a pattern search: the
/// tracking step of a star tracker. Detections are associated with catalog
/// stars within `window` radians of their predicted positions, then refined
/// and accepted exactly like a pattern-search solution. `bodies` are
/// sensor-frame unit vectors of the detections.
pub fn solve_from_prior(
    bodies: &[Vec3],
    catalog: &Catalog,
    prior: &Mat3,
    window: f64,
    config: &LisConfig,
) -> Option<Solution> {
    if !(window > 0.0) || !(config.match_radius > 0.0) {
        return None;
    }
    Scene {
        bodies,
        catalog,
        config,
    }
    .solve_from(prior, &[], None, 1, window)
}

/// `P(X >= k)` for `X ~ Binomial(n, p)`.
pub fn binomial_tail(n: usize, k: usize, p: f64) -> f64 {
    if k == 0 {
        return 1.0;
    }
    if k > n || p <= 0.0 {
        return 0.0;
    }
    if p >= 1.0 {
        return 1.0;
    }
    let mut term = crate::math::pow(1.0 - p, n as f64);
    let mut below = 0.0;
    for i in 0..k {
        below += term;
        term *= (n - i) as f64 / (i + 1) as f64 * p / (1.0 - p);
    }
    (1.0 - below).clamp(0.0, 1.0)
}

/// Identifies the stars behind `bodies` (sensor-frame unit vectors, brightest
/// first) and solves for the attitude.
pub fn identify(
    bodies: &[Vec3],
    index: &PairIndex,
    catalog: &Catalog,
    config: &LisConfig,
) -> LisOutput {
    let mut output = LisOutput::default();
    let n = bodies.len().min(config.pattern_stars);
    if n < 4 || !(config.tolerance > 0.0) || !(config.match_radius > 0.0) {
        return output;
    }
    let mut search = Search {
        bodies,
        index: index.view(),
        catalog,
        config,
        head: vec![NO_LINK; index.star_count()],
        stamp: vec![0; index.star_count()],
        generation: 0,
        links: Vec::new(),
        triangles: Vec::new(),
    };

    // Mortari's triad ordering: stepping by index offsets keeps a single
    // spurious star from occupying a long run of consecutive triads.
    for dj in 1..=(n - 2) {
        for dk in 1..=(n - dj - 1) {
            for i in 0..(n - dj - dk) {
                let j = i + dj;
                let k = j + dk;
                if output.triads_tested as usize >= config.max_triads {
                    return output;
                }
                output.triads_tested += 1;
                if !search.match_triangles(i, j, k) {
                    continue;
                }
                for t in 0..search.triangles.len() {
                    let Some(triangle) = search.triangles.get(t).copied() else {
                        continue;
                    };
                    for r in (0..n).filter(|r| *r != i && *r != j && *r != k) {
                        let Some(fourth) = search.confirm([i, j, k], triangle, r) else {
                            continue;
                        };
                        let attempts = config
                            .prior_pyramids
                            .saturating_add(output.pyramids_tested)
                            .saturating_add(1);
                        if attempts > config.max_pyramids {
                            return output;
                        }
                        output.pyramids_tested += 1;
                        let [a, b, c] = triangle;
                        if let Some(solution) =
                            search.verify([i, j, k, r], [a, b, c, fourth], attempts)
                        {
                            output.solution = Some(solution);
                            return output;
                        }
                    }
                }
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binomial_tail_matches_direct_sums() {
        assert_eq!(binomial_tail(10, 0, 0.3), 1.0);
        assert_eq!(binomial_tail(3, 4, 0.3), 0.0);
        assert!((binomial_tail(1, 1, 0.25) - 0.25).abs() < 1e-15);
        let exact = 1.0 - 0.9f64.powi(5) - 5.0 * 0.1 * 0.9f64.powi(4);
        assert!((binomial_tail(5, 2, 0.1) - exact).abs() < 1e-12);
    }
}
