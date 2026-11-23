#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented,
    clippy::indexing_slicing
)]
// `!(a > b)` is used on purpose: it is also true when either side is NaN.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

pub mod engine;

use wasm_bindgen::prelude::*;

fn js(result: Result<Vec<f64>, String>) -> Result<Vec<f64>, JsError> {
    result.map_err(|message| JsError::new(&message))
}

#[wasm_bindgen]
pub struct Engine {
    inner: engine::Engine,
}

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new(epoch_year: f64) -> Result<Engine, JsError> {
        engine::Engine::new(epoch_year)
            .map(|inner| Engine { inner })
            .map_err(|message| JsError::new(&message))
    }

    #[wasm_bindgen(js_name = catalogSize)]
    pub fn catalog_size(&self) -> u32 {
        self.inner.catalog_size()
    }

    pub fn configure(
        &mut self,
        width: u32,
        height: u32,
        fov_deg: f64,
        distortion: &[f64],
        scale_span: f64,
        current_epoch: bool,
    ) -> Result<Vec<f64>, JsError> {
        js(self.inner.configure(
            width,
            height,
            fov_deg,
            distortion,
            scale_span,
            current_epoch,
        ))
    }

    pub fn render(
        &mut self,
        ra_deg: f64,
        dec_deg: f64,
        roll_deg: f64,
        sensor: &[f64],
        seed: f64,
    ) -> Result<(), JsError> {
        self.inner
            .render(ra_deg, dec_deg, roll_deg, sensor, seed)
            .map_err(|message| JsError::new(&message))
    }

    #[wasm_bindgen(js_name = renderTrial)]
    pub fn render_trial(
        &mut self,
        master_seed: f64,
        trial: f64,
        sensor: &[f64],
        disturbances: bool,
    ) -> Result<Vec<f64>, JsError> {
        js(self
            .inner
            .render_trial(master_seed, trial, sensor, disturbances))
    }

    #[wasm_bindgen(js_name = loadFrame)]
    pub fn load_frame(&mut self, pixels: &[u16]) -> Result<(), JsError> {
        self.inner
            .load_frame(pixels)
            .map_err(|message| JsError::new(&message))
    }

    pub fn frame(&self) -> Result<Vec<u16>, JsError> {
        self.inner
            .frame()
            .map(<[u16]>::to_vec)
            .map_err(|message| JsError::new(&message))
    }

    pub fn truth(&self) -> Result<Vec<f64>, JsError> {
        js(self.inner.truth())
    }

    pub fn detect(&mut self, solver: &[f64]) -> Result<Vec<f64>, JsError> {
        js(self.inner.detect(solver))
    }

    pub fn identify(&mut self, solver: &[f64]) -> Result<Vec<f64>, JsError> {
        js(self.inner.identify(solver))
    }

    #[wasm_bindgen(js_name = beginSearch)]
    pub fn begin_search(&mut self, solver: &[f64], unknown_fov: bool) -> Result<f64, JsError> {
        self.inner
            .begin_search(solver, unknown_fov)
            .map_err(|message| JsError::new(&message))
    }

    #[wasm_bindgen(js_name = stepSearch)]
    pub fn step_search(&mut self, max_scales: u32) -> Result<Vec<f64>, JsError> {
        js(self.inner.step_search(max_scales))
    }

    pub fn design(&self, spec: &[f64]) -> Result<Vec<f64>, JsError> {
        js(self.inner.design(spec))
    }

    pub fn mission(&self, spec: &[f64]) -> Result<Vec<f64>, JsError> {
        js(self.inner.mission(spec))
    }

    #[wasm_bindgen(js_name = stackReset)]
    pub fn stack_reset(&mut self) {
        self.inner.stack_reset();
    }

    #[wasm_bindgen(js_name = stackFrame)]
    pub fn stack_frame(
        &mut self,
        pixels: &[u16],
        depth: u32,
        solver: &[f64],
    ) -> Result<Vec<f64>, JsError> {
        js(self.inner.stack_frame(pixels, depth, solver))
    }

    #[wasm_bindgen(js_name = trackReset)]
    pub fn track_reset(&mut self) {
        self.inner.track_reset();
    }

    #[wasm_bindgen(js_name = trackStep)]
    pub fn track_step(
        &mut self,
        solver: &[f64],
        dt: f64,
        max_misses: u32,
    ) -> Result<Vec<f64>, JsError> {
        js(self.inner.track_step(solver, dt, max_misses))
    }

    #[wasm_bindgen(js_name = searchResult)]
    pub fn search_result(&mut self) -> Result<Vec<f64>, JsError> {
        js(self.inner.search_result())
    }

    pub fn field(
        &self,
        q: &[f64],
        intrinsics: &[f64],
        mag_limit: f64,
    ) -> Result<Vec<f64>, JsError> {
        js(self.inner.field(q, intrinsics, mag_limit))
    }

    #[wasm_bindgen(js_name = monteCarlo)]
    pub fn monte_carlo(
        &self,
        master_seed: f64,
        first_trial: f64,
        count: u32,
        sensor: &[f64],
        solver: &[f64],
        disturbances: bool,
    ) -> Result<Vec<f64>, JsError> {
        js(self.inner.monte_carlo(
            master_seed,
            first_trial,
            count,
            sensor,
            solver,
            disturbances,
        ))
    }
}

/// Decodes a FITS file into `[width, height]` followed by 16-bit pixels.
#[wasm_bindgen(js_name = decodeFits)]
pub fn decode_fits(bytes: &[u8]) -> Result<Vec<u16>, JsError> {
    let image = asterope_io::parse_fits(bytes).map_err(|e| JsError::new(&e.to_string()))?;
    if image.width > u32::from(u16::MAX) || image.height > u32::from(u16::MAX) {
        return Err(JsError::new("FITS image is too large"));
    }
    let mut out = Vec::with_capacity(image.data.len() + 2);
    out.push(image.width as u16);
    out.push(image.height as u16);
    out.extend(image.to_u16());
    Ok(out)
}

#[wasm_bindgen(js_name = centroidValidation)]
pub fn centroid_validation(
    psf_sigma: f64,
    peak_snr: f64,
    phases: u32,
    seeds: u32,
    method: u32,
    seed: f64,
) -> Result<Vec<f64>, JsError> {
    js(engine::centroid_validation(
        psf_sigma, peak_snr, phases, seeds, method, seed,
    ))
}

#[wasm_bindgen(js_name = wahbaValidation)]
pub fn wahba_validation(cases: u32, seed: f64) -> Vec<f64> {
    engine::wahba_validation(cases, seed)
}

#[wasm_bindgen(js_name = pointingQuaternion)]
pub fn pointing_quaternion(ra_deg: f64, dec_deg: f64, roll_deg: f64) -> Vec<f64> {
    engine::pointing_quaternion(ra_deg, dec_deg, roll_deg)
}

#[wasm_bindgen(js_name = attitudeErrorArcsec)]
pub fn attitude_error_arcsec(estimate: &[f64], truth: &[f64]) -> Result<Vec<f64>, JsError> {
    js(engine::attitude_error_arcsec(estimate, truth))
}

#[wasm_bindgen(js_name = stressValues)]
pub fn stress_values(index: u32) -> Result<Vec<f64>, JsError> {
    js(engine::stress_values(index))
}

#[wasm_bindgen(js_name = stressSensor)]
pub fn stress_sensor(sensor: &[f64], index: u32, value: f64) -> Result<Vec<f64>, JsError> {
    js(engine::stress_sensor(sensor, index, value))
}

#[wasm_bindgen(js_name = advancePointing)]
pub fn advance_pointing(pointing: &[f64], rate: &[f64], dt: f64) -> Result<Vec<f64>, JsError> {
    js(engine::advance_pointing(pointing, rate, dt))
}

#[wasm_bindgen(js_name = parityDigest)]
pub fn parity_digest() -> Result<String, JsError> {
    engine::parity_digest().map_err(|message| JsError::new(&message))
}

#[wasm_bindgen(js_name = defaultSensor)]
pub fn default_sensor() -> Vec<f64> {
    engine::default_sensor()
}

#[wasm_bindgen(js_name = defaultSolver)]
pub fn default_solver() -> Vec<f64> {
    engine::default_solver()
}
