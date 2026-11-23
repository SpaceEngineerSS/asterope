//! Reading and writing frames as FITS, raw 16-bit, PNG or JPEG.

use std::error::Error;
use std::fs;
use std::path::Path;

use image::{DynamicImage, ImageBuffer, Luma};

/// Longest side kept; larger photographs are block-averaged, as in the browser.
const MAX_SIDE: u32 = 4096;

pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Row-major, top row first.
    pub pixels: Vec<u16>,
    /// The file is a photograph (PNG or JPEG) rather than a sensor frame.
    pub photograph: bool,
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn srgb_to_linear(value: u8) -> f64 {
    let c = f64::from(value) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn bin(width: u32, height: u32, values: &[f64], full_scale: f64) -> Image {
    let factor = width.max(height).div_ceil(MAX_SIDE).max(1);
    let (out_width, out_height) = (width / factor, height / factor);
    let scale = full_scale / f64::from(factor * factor);
    let mut pixels = Vec::with_capacity((out_width * out_height) as usize);
    for oy in 0..out_height {
        for ox in 0..out_width {
            let mut sum = 0.0;
            for by in 0..factor {
                let row = ((oy * factor + by) * width + ox * factor) as usize;
                sum += values[row..row + factor as usize].iter().sum::<f64>();
            }
            pixels.push((sum * scale).round().clamp(0.0, 65_535.0) as u16);
        }
    }
    Image {
        width: out_width,
        height: out_height,
        pixels,
        photograph: true,
    }
}

fn from_raster(decoded: DynamicImage) -> Image {
    let (width, height) = (decoded.width(), decoded.height());
    match decoded {
        DynamicImage::ImageLuma16(gray) => bin(
            width,
            height,
            &gray.iter().map(|v| f64::from(*v)).collect::<Vec<_>>(),
            1.0,
        ),
        other => {
            let lut: Vec<f64> = (0..=255u8).map(srgb_to_linear).collect();
            let luminance: Vec<f64> = other
                .to_rgb8()
                .pixels()
                .map(|p| {
                    0.2126 * lut[usize::from(p[0])]
                        + 0.7152 * lut[usize::from(p[1])]
                        + 0.0722 * lut[usize::from(p[2])]
                })
                .collect();
            bin(width, height, &luminance, 65_535.0)
        }
    }
}

/// Loads a frame. `raw_size` gives width and height for headerless `.raw`
/// files of little-endian 16-bit pixels; `plane` names the image extension
/// of a FITS file to read instead of the default one.
pub fn load(
    path: &Path,
    raw_size: Option<(u32, u32)>,
    plane: Option<&str>,
) -> Result<Image, Box<dyn Error>> {
    match extension(path).as_str() {
        "fits" | "fit" | "fts" => {
            let fits = asterope_io::parse_fits_plane(&fs::read(path)?, plane)
                .map_err(|e| e.to_string())?;
            Ok(Image {
                width: fits.width,
                height: fits.height,
                pixels: fits.to_u16(),
                photograph: false,
            })
        }
        "raw" | "bin" => {
            let (width, height) = raw_size.ok_or("a raw frame needs --width and --height")?;
            let bytes = fs::read(path)?;
            if bytes.len() != 2 * (width as usize) * (height as usize) {
                return Err(format!(
                    "{} holds {} bytes, expected {} for {width} x {height}",
                    path.display(),
                    bytes.len(),
                    2 * (width as usize) * (height as usize)
                )
                .into());
            }
            Ok(Image {
                width,
                height,
                pixels: bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_le_bytes(*b))
                    .collect(),
                photograph: false,
            })
        }
        _ => Ok(from_raster(image::open(path)?)),
    }
}

/// Saves a frame as FITS, raw little-endian 16-bit, or 16-bit PNG, chosen
/// by the file extension. `full_scale` is the brightest representable value
/// and only affects PNG, which is stretched to the full 16-bit range.
pub fn save(
    path: &Path,
    width: u32,
    height: u32,
    pixels: &[u16],
    full_scale: u16,
) -> Result<(), Box<dyn Error>> {
    match extension(path).as_str() {
        "fits" | "fit" | "fts" => {
            let bytes = asterope_io::encode_fits_u16(width, height, pixels)
                .ok_or("frame dimensions do not match the pixel count")?;
            fs::write(path, bytes)?;
        }
        "raw" | "bin" => {
            let bytes: Vec<u8> = pixels.iter().flat_map(|p| p.to_le_bytes()).collect();
            fs::write(path, bytes)?;
        }
        "png" => {
            let scale = 65_535.0 / f64::from(full_scale.max(1));
            let stretched: Vec<u16> = pixels
                .iter()
                .map(|p| (f64::from(*p) * scale).round().min(65_535.0) as u16)
                .collect();
            ImageBuffer::<Luma<u16>, _>::from_raw(width, height, stretched)
                .ok_or("frame dimensions do not match the pixel count")?
                .save(path)?;
        }
        other => return Err(format!("cannot write .{other}; use .fits, .raw or .png").into()),
    }
    Ok(())
}
