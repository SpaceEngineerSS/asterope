//! Minimal FITS reader: a 2-D image or a 3-D cube whose planes are averaged
//! (RGB stacks), from the primary unit or from a named image extension.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

const BLOCK: usize = 2880;
const CARD: usize = 80;
const MAX_AXIS: usize = 16_384;
const MAX_PIXELS: usize = 64 << 20;
const MAX_UNITS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FitsError {
    NotFits,
    Truncated,
    MissingKeyword(&'static str),
    UnsupportedBitpix,
    UnsupportedShape,
    MissingPlane,
}

impl fmt::Display for FitsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FitsError::NotFits => f.write_str("not a FITS file"),
            FitsError::Truncated => f.write_str("FITS file is truncated"),
            FitsError::MissingKeyword(k) => write!(f, "FITS header lacks {k}"),
            FitsError::UnsupportedBitpix => f.write_str("unsupported BITPIX"),
            FitsError::MissingPlane => f.write_str("FITS file has no image plane of that name"),
            FitsError::UnsupportedShape => {
                f.write_str("only 2-D images and 3-D cubes are supported")
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FitsImage {
    pub width: u32,
    pub height: u32,
    /// Physical values (`BZERO + BSCALE * raw`), top row first.
    pub data: Vec<f32>,
}

impl FitsImage {
    /// Maps the image onto 16 bits: values already in `[0, 65535]` are kept,
    /// anything else is stretched linearly between its extremes.
    pub fn to_u16(&self) -> Vec<u16> {
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for v in self.data.iter().filter(|v| v.is_finite()) {
            lo = lo.min(*v);
            hi = hi.max(*v);
        }
        if !(hi > lo) {
            return vec![0; self.data.len()];
        }
        let direct = lo >= 0.0 && hi <= 65_535.0 && hi > 255.0;
        let (offset, scale) = if direct {
            (0.0, 1.0)
        } else {
            (lo, 65_535.0 / (hi - lo))
        };
        self.data
            .iter()
            .map(|v| {
                let scaled = (v - offset) * scale + 0.5;
                if scaled.is_finite() {
                    scaled.clamp(0.0, 65_535.0) as u16
                } else {
                    0
                }
            })
            .collect()
    }
}

#[derive(Default)]
struct Header {
    simple: bool,
    image_extension: bool,
    name: String,
    bitpix: Option<i64>,
    naxis: Option<i64>,
    axes: [Option<i64>; 3],
    pcount: usize,
    bzero: f64,
    bscale: f64,
    /// `NFRAMES` is present: a Global Meteor Network four-frame file.
    four_frame: bool,
}

fn card_value(card: &[u8]) -> Option<&str> {
    if card.get(8..10) != Some(b"= ") {
        return None;
    }
    let text = core::str::from_utf8(card.get(10..)?).ok()?;
    let value = match text.find('/') {
        Some(slash) if !text.trim_start().starts_with('\'') => text.get(..slash)?,
        _ => text,
    };
    Some(value.trim())
}

fn parse_float(text: &str) -> Option<f64> {
    let normalized: String = text
        .chars()
        .map(|c| if c == 'D' || c == 'd' { 'E' } else { c })
        .collect();
    normalized.parse().ok()
}

/// Text of a quoted FITS string value, without quotes and padding.
fn parse_string(value: &str) -> String {
    value
        .trim_start_matches('\'')
        .split('\'')
        .next()
        .unwrap_or("")
        .trim()
        .into()
}

/// Reads one header; returns it with the offset of the data that follows.
fn parse_header(bytes: &[u8], primary: bool) -> Result<(Header, usize), FitsError> {
    let mut header = Header {
        bscale: 1.0,
        ..Header::default()
    };
    for (index, card) in bytes.as_chunks::<CARD>().0.iter().enumerate() {
        let keyword = core::str::from_utf8(card.get(..8).unwrap_or(&[]))
            .unwrap_or("")
            .trim_end();
        if index == 0 && keyword != if primary { "SIMPLE" } else { "XTENSION" } {
            return Err(FitsError::NotFits);
        }
        if keyword == "END" {
            let header_len = (index + 1) * CARD;
            return Ok((header, header_len.div_ceil(BLOCK) * BLOCK));
        }
        let Some(value) = card_value(card) else {
            continue;
        };
        match keyword {
            "SIMPLE" => header.simple = value == "T",
            "XTENSION" => header.image_extension = parse_string(value) == "IMAGE",
            "EXTNAME" => header.name = parse_string(value),
            "BITPIX" => header.bitpix = value.parse().ok(),
            "NAXIS" => header.naxis = value.parse().ok(),
            "NAXIS1" => header.axes[0] = value.parse().ok(),
            "NAXIS2" => header.axes[1] = value.parse().ok(),
            "NAXIS3" => header.axes[2] = value.parse().ok(),
            "PCOUNT" => header.pcount = value.parse().unwrap_or(0),
            "BZERO" => header.bzero = parse_float(value).unwrap_or(0.0),
            "BSCALE" => header.bscale = parse_float(value).unwrap_or(1.0),
            "NFRAMES" => header.four_frame = true,
            _ => {}
        }
    }
    Err(FitsError::Truncated)
}

fn axis(value: Option<i64>, name: &'static str) -> Result<usize, FitsError> {
    let v = value.ok_or(FitsError::MissingKeyword(name))?;
    if v < 1 || v as usize > MAX_AXIS {
        return Err(FitsError::UnsupportedShape);
    }
    Ok(v as usize)
}

fn decode(bitpix: i64, bytes: &[u8]) -> Option<f64> {
    Some(match bitpix {
        8 => f64::from(*bytes.first()?),
        16 => f64::from(i16::from_be_bytes(*bytes.first_chunk()?)),
        32 => f64::from(i32::from_be_bytes(*bytes.first_chunk()?)),
        -32 => f64::from(f32::from_be_bytes(*bytes.first_chunk()?)),
        -64 => f64::from_be_bytes(*bytes.first_chunk()?),
        _ => return None,
    })
}

/// Width, height, planes and bytes per sample of a header's image, or
/// `None` when the unit holds no data.
fn shape(header: &Header) -> Result<Option<(usize, usize, usize, usize)>, FitsError> {
    let naxis = header.naxis.ok_or(FitsError::MissingKeyword("NAXIS"))?;
    if naxis == 0 {
        return Ok(None);
    }
    let sample = match header.bitpix.ok_or(FitsError::MissingKeyword("BITPIX"))? {
        8 => 1,
        16 => 2,
        32 | -32 => 4,
        -64 => 8,
        _ => return Err(FitsError::UnsupportedBitpix),
    };
    let width = axis(header.axes[0], "NAXIS1")?;
    let height = axis(header.axes[1], "NAXIS2")?;
    let planes = match naxis {
        2 => 1,
        3 => axis(header.axes[2], "NAXIS3")?,
        _ => return Err(FitsError::UnsupportedShape),
    };
    if width * height > MAX_PIXELS {
        return Err(FitsError::UnsupportedShape);
    }
    Ok(Some((width, height, planes, sample)))
}

/// Reads the first image of a FITS file: the primary unit, or the first
/// image extension when the primary unit holds no data. For a Global Meteor
/// Network four-frame file that is the `AVEPIXEL` plane, the mean of the
/// 256 video frames.
pub fn parse_fits(bytes: &[u8]) -> Result<FitsImage, FitsError> {
    parse_fits_plane(bytes, None)
}

/// Reads the image extension called `name` (`EXTNAME`, case-insensitive),
/// or with `None` the image `parse_fits` would pick.
pub fn parse_fits_plane(bytes: &[u8], name: Option<&str>) -> Result<FitsImage, FitsError> {
    if bytes.len() < CARD {
        return Err(FitsError::NotFits);
    }
    let mut offset = 0usize;
    let mut four_frame = false;
    for unit in 0..MAX_UNITS {
        let rest = bytes.get(offset..).unwrap_or(&[]);
        if rest.is_empty() {
            break;
        }
        let (header, data_start) = parse_header(rest, unit == 0)?;
        if unit == 0 {
            if !header.simple {
                return Err(FitsError::NotFits);
            }
            four_frame = header.four_frame;
        }
        let image = if unit == 0 || header.image_extension {
            shape(&header)?
        } else {
            None
        };
        let wanted = match name {
            Some(name) => header.name.eq_ignore_ascii_case(name),
            None if four_frame => header.name == "AVEPIXEL",
            None => true,
        };
        if let (Some((width, height, planes, sample)), true) = (image, wanted) {
            return read_image(
                &header,
                rest.get(data_start..).unwrap_or(&[]),
                (width, height, planes.min(4), sample),
                four_frame,
            );
        }
        let data_len = match image {
            Some((width, height, planes, sample)) => width * height * planes * sample,
            None => 0,
        } + header.pcount;
        offset += data_start + data_len.div_ceil(BLOCK) * BLOCK;
    }
    Err(if name.is_some() || four_frame {
        FitsError::MissingPlane
    } else {
        FitsError::UnsupportedShape
    })
}

/// FITS stores the bottom row first, and the rows are flipped so that row 0
/// is the top of the displayed image and the sky keeps its handedness.
/// Four-frame meteor files are the exception: they are written straight
/// from the camera with the top row first.
fn read_image(
    header: &Header,
    data: &[u8],
    (width, height, planes, sample): (usize, usize, usize, usize),
    top_row_first: bool,
) -> Result<FitsImage, FitsError> {
    let bitpix = header.bitpix.ok_or(FitsError::MissingKeyword("BITPIX"))?;
    let plane_len = width * height * sample;
    let data = data.get(..plane_len * planes).ok_or(FitsError::Truncated)?;
    let mut out = vec![0.0f32; width * height];
    for plane in data.chunks_exact(plane_len) {
        for (index, row) in plane.chunks_exact(width * sample).enumerate() {
            let line = if top_row_first {
                index
            } else {
                height - 1 - index
            };
            let Some(target) = out.get_mut(line * width..(line + 1) * width) else {
                continue;
            };
            for (raw, pixel) in row.chunks_exact(sample).zip(target.iter_mut()) {
                let value = decode(bitpix, raw).unwrap_or(0.0);
                *pixel += ((header.bzero + header.bscale * value) / planes as f64) as f32;
            }
        }
    }
    Ok(FitsImage {
        width: width as u32,
        height: height as u32,
        data: out,
    })
}
/// Encodes 16-bit pixels (top row first) as a single-HDU FITS file with
/// `BITPIX = 16` and `BZERO = 32768`, the convention for unsigned data.
/// Returns `None` when the dimensions do not match the pixel count.
pub fn encode_fits_u16(width: u32, height: u32, pixels: &[u16]) -> Option<Vec<u8>> {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || w.checked_mul(h) != Some(pixels.len()) {
        return None;
    }
    let cards = [
        format!("{:<8}= {:>20}", "SIMPLE", "T"),
        format!("{:<8}= {:>20}", "BITPIX", 16),
        format!("{:<8}= {:>20}", "NAXIS", 2),
        format!("{:<8}= {:>20}", "NAXIS1", width),
        format!("{:<8}= {:>20}", "NAXIS2", height),
        format!("{:<8}= {:>20}", "BZERO", 32768),
        format!("{:<8}= {:>20}", "BSCALE", 1),
        "END".into(),
    ];
    let mut out = Vec::with_capacity(BLOCK + (2 * pixels.len()).div_ceil(BLOCK) * BLOCK);
    for card in &cards {
        out.extend(card.bytes().chain(core::iter::repeat(b' ')).take(CARD));
    }
    out.resize(out.len().div_ceil(BLOCK) * BLOCK, b' ');
    for row in pixels.chunks_exact(w).rev() {
        for pixel in row {
            out.extend_from_slice(&((i32::from(*pixel) - 32_768) as i16).to_be_bytes());
        }
    }
    out.resize(out.len().div_ceil(BLOCK) * BLOCK, 0);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(text: &str) -> Vec<u8> {
        let mut bytes = text.as_bytes().to_vec();
        bytes.resize(CARD, b' ');
        bytes
    }

    fn file(cards: &[&str], data: &[u8]) -> Vec<u8> {
        let mut bytes: Vec<u8> = cards.iter().flat_map(|c| card(c)).collect();
        bytes.extend(card("END"));
        bytes.resize(bytes.len().div_ceil(BLOCK) * BLOCK, b' ');
        bytes.extend_from_slice(data);
        bytes
    }

    fn header_16(width: usize, height: usize) -> Vec<String> {
        vec![
            "SIMPLE  =                    T / conforms".to_string(),
            "BITPIX  =                   16".to_string(),
            "NAXIS   =                    2".to_string(),
            format!("NAXIS1  = {width:>20}"),
            format!("NAXIS2  = {height:>20}"),
            "BZERO   =                32768 / unsigned".to_string(),
            "BSCALE  =                    1".to_string(),
        ]
    }

    #[test]
    fn reads_unsigned_16_bit_and_flips_rows() {
        let (width, height) = (16usize, 16usize);
        let mut data = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let value = (y * 1000 + x) as i32 - 32768;
                data.extend_from_slice(&(value as i16).to_be_bytes());
            }
        }
        let cards = header_16(width, height);
        let refs: Vec<&str> = cards.iter().map(String::as_str).collect();
        let image = parse_fits(&file(&refs, &data)).unwrap();
        assert_eq!((image.width, image.height), (16, 16));
        assert_eq!(image.data[0], 15_000.0, "top-left is the last stored row");
        assert_eq!(
            image.data[15 * 16 + 3],
            3.0,
            "bottom row is the first stored row"
        );
        assert_eq!(image.to_u16()[1], 15_001);
    }

    #[test]
    fn reads_float_images_and_stretches_them() {
        let mut data = Vec::new();
        for i in 0..256 {
            data.extend_from_slice(&(i as f32 * 0.001 - 0.05).to_be_bytes());
        }
        let cards = [
            "SIMPLE  =                    T",
            "BITPIX  =                  -32",
            "NAXIS   =                    2",
            "NAXIS1  =                   16",
            "NAXIS2  =                   16",
        ];
        let image = parse_fits(&file(&cards, &data)).unwrap();
        let scaled = image.to_u16();
        assert_eq!(scaled.iter().min(), Some(&0));
        assert_eq!(scaled.iter().max(), Some(&65_535));
    }

    /// Small deterministic generator; the crate has no dependencies.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }
    }

    #[test]
    fn survives_corrupted_and_random_files() {
        let cards = header_16(16, 16);
        let refs: Vec<&str> = cards.iter().map(String::as_str).collect();
        let valid = file(&refs, &[7u8; 512]);
        let keywords = [
            "BITPIX", "NAXIS ", "NAXIS1", "NAXIS2", "NAXIS3", "BZERO ", "BSCALE",
        ];
        let values = [
            "-64",
            "0",
            "3",
            "99999999999999999999",
            "-1",
            "1E308",
            "NaN",
            "T",
            "'x'",
            "",
        ];
        let mut rng = Lcg(42);
        let mut parsed = 0u32;
        for case in 0..100_000u32 {
            let mut bytes = valid.clone();
            match case % 4 {
                0 => {
                    for _ in 0..1 + rng.next() % 8 {
                        let at = (rng.next() % bytes.len() as u64) as usize;
                        bytes[at] = rng.next() as u8;
                    }
                }
                1 => bytes.truncate((rng.next() % (bytes.len() as u64 + 1)) as usize),
                2 => {
                    let slot = 1 + (rng.next() % 6) as usize;
                    let keyword = keywords[(rng.next() % keywords.len() as u64) as usize];
                    let value = values[(rng.next() % values.len() as u64) as usize];
                    let text = format!("{keyword:<8}= {value:>20}");
                    bytes[slot * CARD..(slot + 1) * CARD].copy_from_slice(&card(&text));
                }
                _ => bytes.iter_mut().for_each(|b| *b = rng.next() as u8),
            }
            if let Ok(image) = parse_fits(&bytes) {
                parsed += 1;
                assert_eq!(image.data.len(), (image.width * image.height) as usize);
                assert_eq!(image.to_u16().len(), image.data.len());
            }
        }
        assert!(
            parsed > 1000,
            "the generator should also produce readable files: {parsed}"
        );
    }

    #[test]
    fn rejects_malformed_input() {
        assert_eq!(parse_fits(b"not fits").unwrap_err(), FitsError::NotFits);
        assert_eq!(parse_fits(&[b' '; 5760]).unwrap_err(), FitsError::NotFits);
        let cards = header_16(16, 16);
        let refs: Vec<&str> = cards.iter().map(String::as_str).collect();
        assert_eq!(
            parse_fits(&file(&refs, &[0u8; 100])).unwrap_err(),
            FitsError::Truncated
        );
        let bad = [
            "SIMPLE  =                    T",
            "BITPIX  =                   64",
            "NAXIS   =                    2",
            "NAXIS1  =                   16",
            "NAXIS2  =                   16",
        ];
        assert_eq!(
            parse_fits(&file(&bad, &[0u8; 4096])).unwrap_err(),
            FitsError::UnsupportedBitpix
        );
        let huge = [
            "SIMPLE  =                    T",
            "BITPIX  =                   16",
            "NAXIS   =                    2",
            "NAXIS1  =             99999999",
            "NAXIS2  =             99999999",
        ];
        assert_eq!(
            parse_fits(&file(&huge, &[])).unwrap_err(),
            FitsError::UnsupportedShape
        );
    }

    #[test]
    fn encoded_frame_reads_back_unchanged() {
        let pixels: Vec<u16> = (0..12u16).map(|i| i * 5957).collect();
        let bytes = encode_fits_u16(4, 3, &pixels).unwrap();
        assert_eq!(bytes.len() % BLOCK, 0);
        let image = parse_fits(&bytes).unwrap();
        assert_eq!((image.width, image.height), (4, 3));
        assert_eq!(image.to_u16(), pixels);
        assert!(encode_fits_u16(4, 4, &pixels).is_none());
    }

    /// One header-plus-data unit, padded to FITS blocks.
    fn unit(cards: &[&str], data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for card in cards.iter().chain(&["END"]) {
            out.extend(card.bytes().chain(core::iter::repeat(b' ')).take(CARD));
        }
        out.resize(out.len().div_ceil(BLOCK) * BLOCK, b' ');
        out.extend_from_slice(data);
        out.resize(out.len().div_ceil(BLOCK) * BLOCK, 0);
        out
    }

    fn four_frame_file() -> Vec<u8> {
        let plane = |name: &str, data: &[u8]| {
            unit(
                &[
                    "XTENSION= 'IMAGE   '",
                    "BITPIX  =                    8",
                    "NAXIS   =                    2",
                    "NAXIS1  =                    3",
                    "NAXIS2  =                    2",
                    "PCOUNT  =                    0",
                    "GCOUNT  =                    1",
                    &format!("EXTNAME = '{name:<8}'"),
                ],
                data,
            )
        };
        let mut file = unit(
            &[
                "SIMPLE  =                    T",
                "BITPIX  =                    8",
                "NAXIS   =                    0",
                "EXTEND  =                    T",
                "NFRAMES =                  256",
            ],
            &[],
        );
        file.extend(plane("MAXPIXEL", &[9, 9, 9, 9, 9, 9]));
        file.extend(plane("AVEPIXEL", &[10, 20, 30, 40, 50, 250]));
        file
    }

    #[test]
    fn four_frame_files_give_the_mean_plane_with_the_top_row_first() {
        let file = four_frame_file();
        let mean = parse_fits(&file).unwrap();
        assert_eq!((mean.width, mean.height), (3, 2));
        assert_eq!(mean.data, [10.0, 20.0, 30.0, 40.0, 50.0, 250.0]);
        let peak = parse_fits_plane(&file, Some("maxpixel")).unwrap();
        assert!(peak.data.iter().all(|v| *v == 9.0));
        assert_eq!(
            parse_fits_plane(&file, Some("STDPIXEL")).unwrap_err(),
            FitsError::MissingPlane
        );
        assert_eq!(
            parse_fits(&file[..file.len() - 2 * BLOCK]).unwrap_err(),
            FitsError::MissingPlane
        );
        assert_eq!(
            parse_fits(&file[..file.len() - BLOCK]).unwrap_err(),
            FitsError::Truncated
        );
        let mut cut = file.clone();
        cut.truncate(2 * BLOCK + BLOCK + 40);
        assert!(parse_fits(&cut).is_err());
    }
}
