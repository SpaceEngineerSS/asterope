use std::fmt::Write;

use asterope_core::math::DEG;
use asterope_core::validation::{
    TrialClass, FALSE_POSITIVE_DEG, SUCCESS_BORESIGHT_ARCSEC, SUCCESS_ROLL_ARCSEC,
};

use crate::validate::RunSummary;

pub const SUCCESS_TARGET: f64 = 0.98;
pub const FALSE_POSITIVE_TARGET: f64 = 0.001;

fn rate(count: u32, trials: u32) -> f64 {
    f64::from(count) / f64::from(trials.max(1))
}

/// Point estimates against the targets. Whether the trial count also supports
/// the claim statistically is reported separately through the intervals.
pub fn passes(run: &RunSummary) -> bool {
    rate(run.tally.success, run.tally.trials) > SUCCESS_TARGET
        && rate(run.tally.false_positive, run.tally.trials) < FALSE_POSITIVE_TARGET
}

pub fn console_line(run: &RunSummary) -> String {
    format!(
        "FOV {:>4.1} deg | {} trials | success {:.2} % [{:.2}, {:.2}] | false positive {} (<= {:.3} % at 95 %) | \
         cross {:.2}\" / roll {:.1}\" median | {:.1} s",
        run.fov_deg,
        run.tally.trials,
        100.0 * rate(run.tally.success, run.tally.trials),
        100.0 * run.success_interval.0,
        100.0 * run.success_interval.1,
        run.tally.false_positive,
        100.0 * run.false_positive_interval.1,
        run.cross.median,
        run.roll.median,
        run.wall_seconds
    )
}

fn class_name(class: TrialClass) -> &'static str {
    match class {
        TrialClass::Success => "success",
        TrialClass::Inaccurate => "inaccurate",
        TrialClass::FalsePositive => "false_positive",
        TrialClass::NoSolution => "no_solution",
    }
}

fn table_row(out: &mut String, run: &RunSummary) {
    let t = &run.tally;
    let _ = writeln!(
        out,
        "| {:.0} | {} | {:.1} | {:.1} | {:.2} | [{:.2}, {:.2}] | {} | {:.3} | {} | {} | {:.2} | {:.2} | {:.1} | {:.1} | {:.2} |",
        run.fov_deg,
        run.index_stars,
        run.mean_truth_stars,
        run.mean_detections,
        100.0 * rate(t.success, t.trials),
        100.0 * run.success_interval.0,
        100.0 * run.success_interval.1,
        t.false_positive,
        100.0 * run.false_positive_interval.1,
        t.no_solution,
        t.inaccurate,
        run.cross.median,
        run.cross.p95,
        run.roll.median,
        run.roll.p95,
        100.0 * rate(t.starved, t.trials),
    );
}

const TABLE_HEADER: &str = "| FOV (deg) | Index stars | Catalog stars / frame | Detections / frame | Success (%) | 95 % CI | False positives | FP upper bound (%) | No solution | Inaccurate | Cross median (\") | Cross p95 (\") | Roll median (\") | Roll p95 (\") | Frames with < 4 stars (%) |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|\n";

pub fn markdown(baseline: &RunSummary, sweep: &[RunSummary], seed: u64) -> String {
    let mut out = String::new();
    let t = &baseline.tally;
    let _ = writeln!(out, "# Asterope lost-in-space validation\n");
    let _ = writeln!(
        out,
        "Master seed {seed}, {} trials per configuration, {size}x{size} sensor. Each trial draws a \
         uniformly random attitude, hot-pixel fraction (1e-4 to 1e-3), 0-5 cosmic-ray streaks and, in \
         30 % of trials, a stray-light gradient.\n",
        t.trials,
        size = baseline.size
    );
    let _ = writeln!(
        out,
        "Classification: **success** = solved with cross-boresight error <= {SUCCESS_BORESIGHT_ARCSEC}\" \
         and roll error <= {SUCCESS_ROLL_ARCSEC}\"; **false positive** = an attitude reported more than \
         {FALSE_POSITIVE_DEG} deg from the truth; **inaccurate** = solved between those limits. No trial \
         is removed from the denominator. Intervals are exact Clopper-Pearson 95 %.\n"
    );

    let _ = writeln!(out, "## Baseline (FOV {:.1} deg)\n", baseline.fov_deg);
    let _ = writeln!(
        out,
        "| Criterion | Target | Measured | 95 % interval | Verdict |\n|---|---|---|---|---|"
    );
    let success = rate(t.success, t.trials);
    let false_positive = rate(t.false_positive, t.trials);
    let _ = writeln!(
        out,
        "| Success rate | > {:.0} % | {:.2} % | [{:.2}, {:.2}] % | {} |",
        100.0 * SUCCESS_TARGET,
        100.0 * success,
        100.0 * baseline.success_interval.0,
        100.0 * baseline.success_interval.1,
        if success > SUCCESS_TARGET {
            "pass"
        } else {
            "FAIL"
        }
    );
    let _ = writeln!(
        out,
        "| False-positive rate | < {:.1} % | {:.3} % ({} of {}) | [0, {:.3}] % | {} |",
        100.0 * FALSE_POSITIVE_TARGET,
        100.0 * false_positive,
        t.false_positive,
        t.trials,
        100.0 * baseline.false_positive_interval.1,
        if false_positive < FALSE_POSITIVE_TARGET {
            "pass"
        } else {
            "FAIL"
        }
    );
    let _ = writeln!(out);
    if baseline.false_positive_interval.1 >= FALSE_POSITIVE_TARGET {
        let _ = writeln!(
            out,
            "> With {} trials the 95 % upper bound on the false-positive rate is {:.3} %, which is \
             above the {:.1} % target. The point estimate passes, but this run is too small to \
             demonstrate the target; about 3700 trials with zero false positives are needed.\n",
            t.trials,
            100.0 * baseline.false_positive_interval.1,
            100.0 * FALSE_POSITIVE_TARGET
        );
    }
    if baseline.success_interval.0 <= SUCCESS_TARGET {
        let _ = writeln!(
            out,
            "> The lower 95 % bound on the success rate ({:.2} %) does not clear the {:.0} % target.\n",
            100.0 * baseline.success_interval.0,
            100.0 * SUCCESS_TARGET
        );
    }

    let _ = writeln!(
        out,
        "| Accuracy (solved trials) | Median | 95th percentile | Maximum |\n|---|---|---|---|"
    );
    let _ = writeln!(
        out,
        "| Cross-boresight (\") | {:.2} | {:.2} | {:.2} |",
        baseline.cross.median, baseline.cross.p95, baseline.cross.max
    );
    let _ = writeln!(
        out,
        "| Roll (\") | {:.1} | {:.1} | {:.1} |\n",
        baseline.roll.median, baseline.roll.p95, baseline.roll.max
    );

    let _ = writeln!(
        out,
        "Pattern index: {} stars (V <= {:.2}), {} pairs, built in {:.0} ms. Pixel scale {:.1}\"/px.\n",
        baseline.index_stars,
        baseline.index_mag_limit,
        baseline.index_pairs,
        baseline.index_build_ms,
        baseline.fov_deg * DEG / f64::from(baseline.size) / DEG * 3600.0
    );
    let _ = writeln!(
        out,
        "| Stage (native, single thread, median) | Time (ms) |\n|---|---|"
    );
    let _ = writeln!(out, "| Frame synthesis | {:.2} |", baseline.times.render_ms);
    let _ = writeln!(
        out,
        "| Detection and centroiding | {:.2} |",
        baseline.times.detect_ms
    );
    let _ = writeln!(
        out,
        "| Identification and attitude | {:.2} |\n",
        baseline.times.identify_ms
    );

    let _ = writeln!(out, "### Trials that did not succeed\n");
    if baseline.failures.is_empty() {
        let _ = writeln!(out, "None.\n");
    } else {
        let _ = writeln!(out, "| Trial | Class | Catalog stars in frame | Detections | Matches | Error (\") |\n|---|---|---|---|---|---|");
        for (trial, outcome) in baseline.failures.iter().take(200) {
            let _ = writeln!(
                out,
                "| {trial} | {} | {} | {} | {} | {:.1} |",
                class_name(outcome.class),
                outcome.truth_stars,
                outcome.detections,
                outcome.matches,
                outcome.total_arcsec
            );
        }
        let _ = writeln!(out);
    }

    if !sweep.is_empty() {
        let _ = writeln!(out, "## Field-of-view sweep\n");
        out.push_str(TABLE_HEADER);
        for run in sweep {
            table_row(&mut out, run);
        }
        let _ = writeln!(
            out,
            "\nThe success target applies to the baseline. Narrow fields are listed as measured: with a \
             V <= 6.5 catalog some pointings hold too few stars.\n"
        );
    }
    out
}

pub fn stress_markdown(tables: &[crate::validate::StressTable], trials: u64) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "## Stress sweeps\n");
    let _ = writeln!(
        out,
        "One sensor quantity is varied at a time with every other disturbance off, {trials} trials per \
         step, the same attitudes at every step. Body rate is a rotation about the sensor y axis \
         during the exposure.\n"
    );
    for table in tables {
        let _ = writeln!(
            out,
            "### {} ({})\n",
            table.parameter.label(),
            table.parameter.unit()
        );
        let _ = writeln!(
            out,
            "| Value | Success (%) | 95 % interval | No solution | Inaccurate | False positives | Cross median (\") |\n|---|---|---|---|---|---|---|"
        );
        for row in &table.rows {
            let t = &row.tally;
            let (lo, hi) = asterope_core::validation::clopper_pearson(t.success, t.trials);
            let _ = writeln!(
                out,
                "| {} | {:.1} | [{:.1}, {:.1}] | {} | {} | {} | {} |",
                row.value,
                100.0 * rate(t.success, t.trials),
                100.0 * lo,
                100.0 * hi,
                t.no_solution,
                t.inaccurate,
                t.false_positive,
                if row.cross_median.is_finite() {
                    format!("{:.2}", row.cross_median)
                } else {
                    "-".to_string()
                }
            );
        }
        let _ = writeln!(out);
    }
    out
}

pub fn profile_line(row: &crate::validate::ProfileRow) -> String {
    let t = &row.tally;
    format!(
        "{:<8} | index {:>5} stars V<={:.2} {:>6} pairs {:>7} B | catalog {:>4} stars | success {:.2} % | \
         false positive {} | cross {:.2}\" | identify {:.2} ms",
        row.profile.name,
        row.index_stars,
        row.index_mag_limit,
        row.index_pairs,
        row.index_bytes,
        row.catalog_stars,
        100.0 * rate(t.success, t.trials),
        t.false_positive,
        row.cross_median,
        row.identify_ms
    )
}

pub fn profiles_markdown(
    rows: &[crate::validate::ProfileRow],
    fov_deg: f64,
    trials: u64,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "## Onboard profiles\n");
    let _ = writeln!(
        out,
        "The baseline scenario at {fov_deg} degrees, {trials} trials per profile, with the index \
         serialized and searched in place and the solver limited to the onboard catalog. Frames \
         are rendered from the whole catalog. \"Stars per field\" sets the faintest indexed \
         magnitude; \"reach\" is the largest indexed separation as a fraction of the frame \
         diagonal.\n"
    );
    let _ = writeln!(
        out,
        "| Profile | Stars per field | Reach | Index stars | Index V limit | Pairs | Index (kB) | Catalog stars | Catalog (kB) | Success (%) | 95 % interval | No solution | False positives | Cross median (\") | Identify (ms) |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|"
    );
    for row in rows {
        let t = &row.tally;
        let (lo, hi) = asterope_core::validation::clopper_pearson(t.success, t.trials);
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {:.2} | {} | {:.0} | {} | {:.0} | {:.2} | [{:.2}, {:.2}] | {} | {} | {:.2} | {:.2} |",
            row.profile.name,
            row.profile.stars_per_field,
            row.profile.angle_fraction,
            row.index_stars,
            row.index_mag_limit,
            row.index_pairs,
            row.index_bytes as f64 / 1000.0,
            row.catalog_stars,
            row.catalog_file_bytes as f64 / 1000.0,
            100.0 * rate(t.success, t.trials),
            100.0 * lo,
            100.0 * hi,
            t.no_solution,
            t.false_positive,
            row.cross_median,
            row.identify_ms
        );
    }
    let _ = writeln!(out);
    out
}

fn json_number(value: f64) -> String {
    if value.is_finite() {
        format!("{value}")
    } else {
        "null".to_string()
    }
}

fn json_run(run: &RunSummary) -> String {
    let t = &run.tally;
    let failures: Vec<String> = run
        .failures
        .iter()
        .map(|(trial, o)| {
            format!(
                "{{\"trial\":{trial},\"class\":\"{}\",\"truthStars\":{},\"detections\":{},\"matches\":{},\"errorArcsec\":{}}}",
                class_name(o.class),
                o.truth_stars,
                o.detections,
                o.matches,
                json_number(o.total_arcsec)
            )
        })
        .collect();
    format!(
        "{{\"fovDeg\":{},\"size\":{},\"indexStars\":{},\"indexPairs\":{},\"trials\":{},\"success\":{},\
         \"inaccurate\":{},\"falsePositive\":{},\"noSolution\":{},\"starved\":{},\
         \"successInterval\":[{},{}],\"falsePositiveInterval\":[{},{}],\
         \"crossArcsec\":{{\"median\":{},\"p95\":{},\"max\":{}}},\
         \"rollArcsec\":{{\"median\":{},\"p95\":{},\"max\":{}}},\
         \"stageMs\":{{\"render\":{},\"detect\":{},\"identify\":{}}},\"failures\":[{}]}}",
        run.fov_deg,
        run.size,
        run.index_stars,
        run.index_pairs,
        t.trials,
        t.success,
        t.inaccurate,
        t.false_positive,
        t.no_solution,
        t.starved,
        run.success_interval.0,
        run.success_interval.1,
        run.false_positive_interval.0,
        run.false_positive_interval.1,
        json_number(run.cross.median),
        json_number(run.cross.p95),
        json_number(run.cross.max),
        json_number(run.roll.median),
        json_number(run.roll.p95),
        json_number(run.roll.max),
        json_number(run.times.render_ms),
        json_number(run.times.detect_ms),
        json_number(run.times.identify_ms),
        failures.join(",")
    )
}

pub fn json(baseline: &RunSummary, sweep: &[RunSummary], seed: u64) -> String {
    let sweep_json: Vec<String> = sweep.iter().map(json_run).collect();
    format!(
        "{{\"seed\":{seed},\"baseline\":{},\"sweep\":[{}]}}\n",
        json_run(baseline),
        sweep_json.join(",")
    )
}
