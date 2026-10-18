//! Screenshots as RGBA pixels: PNG decoding and encoding, comparison with a reference, diff
//! pictures, cutting and scaling.

use std::io::Cursor;

use jevision_core::Rect;

#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Row-major, 4 bytes per pixel.
    pub rgba: Vec<u8>,
}

/// Result of comparing a screenshot with a reference.
#[derive(Debug, Clone)]
pub struct Comparison {
    /// Share of compared pixels that match, 0–1.
    pub similarity: f64,
    /// Number of compared pixels that differ.
    pub differing: u64,
    /// The screenshot faded, differing pixels in red, left-out areas tinted blue.
    pub diff: Image,
}

/// Screenshot and reference have different shapes, so they cannot be the same screen.
#[derive(Debug)]
pub struct ShapeMismatch;

/// Largest relative difference in aspect ratio still treated as the same window at another
/// scale (rounding of odd point sizes on Retina displays).
const ASPECT_TOLERANCE: f64 = 0.01;

impl Image {
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let mut decoder = png::Decoder::new(Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
        let size = reader
            .output_buffer_size()
            .ok_or_else(|| "image is too large".to_owned())?;
        let mut buf = vec![0; size];
        let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
        let pixels = info.width as usize * info.height as usize;
        let rgba = match info.color_type {
            png::ColorType::Rgba => {
                buf.truncate(pixels * 4);
                buf
            }
            png::ColorType::Rgb => expand(&buf, 3, pixels, |c| [c[0], c[1], c[2], 255]),
            png::ColorType::GrayscaleAlpha => expand(&buf, 2, pixels, |c| [c[0], c[0], c[0], c[1]]),
            png::ColorType::Grayscale => expand(&buf, 1, pixels, |c| [c[0], c[0], c[0], 255]),
            png::ColorType::Indexed => return Err("indexed colour was not expanded".to_owned()),
        };
        Ok(Self {
            width: info.width,
            height: info.height,
            rgba,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer
            .write_image_data(&self.rgba)
            .map_err(|e| e.to_string())?;
        writer.finish().map_err(|e| e.to_string())?;
        Ok(out)
    }

    /// Compares colours of `self` (the screenshot) with `reference` inside `area` (whole image
    /// when `None`), leaving out `skip`. A pixel differs when any channel is off by more than
    /// `tolerance`. Images of the same shape but another size (Retina and non-Retina) are
    /// compared at the smaller size.
    pub fn compare(
        &self,
        reference: &Image,
        area: Option<&Rect>,
        skip: &[Rect],
        tolerance: u8,
    ) -> Result<Comparison, ShapeMismatch> {
        let (actual, expected) = same_size(self, reference)?;
        let (w, h) = (actual.width, actual.height);
        let full = Rect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        };
        let (ax0, ay0, ax1, ay1) = actual.pixels_of(area.unwrap_or(&full));
        let mut left_out = vec![false; (w * h) as usize];
        for r in skip {
            let (x0, y0, x1, y1) = actual.pixels_of(r);
            for y in y0..y1 {
                left_out[(y * w + x0) as usize..(y * w + x1) as usize].fill(true);
            }
        }

        let mut diff = Vec::with_capacity(actual.rgba.len());
        let (mut compared, mut differing) = (0u64, 0u64);
        for y in 0..h {
            for x in 0..w {
                let p = actual.at(x, y);
                let (a, e) = (&actual.rgba[p..p + 3], &expected.rgba[p..p + 3]);
                let luma = (u32::from(a[0]) * 3 + u32::from(a[1]) * 6 + u32::from(a[2])) / 10;
                let faded = (165 + luma * 90 / 255) as u8;
                let inside = (ax0..ax1).contains(&x) && (ay0..ay1).contains(&y);
                if !inside || left_out[(y * w + x) as usize] {
                    diff.extend([faded / 2 + 40, faded / 2 + 70, 230, 255]);
                    continue;
                }
                compared += 1;
                if a.iter().zip(e).any(|(a, e)| a.abs_diff(*e) > tolerance) {
                    differing += 1;
                    diff.extend([230, 20, 60, 255]);
                } else {
                    diff.extend([faded, faded, faded, 255]);
                }
            }
        }
        let similarity = if compared == 0 {
            1.0
        } else {
            1.0 - differing as f64 / compared as f64
        };
        Ok(Comparison {
            similarity,
            differing,
            diff: Image {
                width: w,
                height: h,
                rgba: diff,
            },
        })
    }

    /// The part inside `area`, at least one pixel.
    pub fn crop(&self, area: &Rect) -> Image {
        let (x0, y0, x1, y1) = self.pixels_of(area);
        let (x0, y0) = (x0.min(self.width - 1), y0.min(self.height - 1));
        let (x1, y1) = (x1.max(x0 + 1), y1.max(y0 + 1));
        let mut rgba = Vec::with_capacity(((x1 - x0) * (y1 - y0) * 4) as usize);
        for y in y0..y1 {
            rgba.extend_from_slice(&self.rgba[self.at(x0, y)..self.at(x1, y)]);
        }
        Image {
            width: x1 - x0,
            height: y1 - y0,
            rgba,
        }
    }

    /// A copy no larger than `max` pixels on its longer side.
    pub fn fit(&self, max: u32) -> Image {
        let long = self.width.max(self.height);
        if long <= max {
            return self.clone();
        }
        let scale =
            |side: u32| ((u64::from(side) * u64::from(max) / u64::from(long)) as u32).max(1);
        self.resized(scale(self.width), scale(self.height))
    }

    /// Box-filtered copy at another size.
    pub(crate) fn resized(&self, width: u32, height: u32) -> Image {
        let span = |t: u32, target: u32, source: u32| {
            let from = u64::from(t) * u64::from(source) / u64::from(target);
            let to = (u64::from(t + 1) * u64::from(source) / u64::from(target)).max(from + 1);
            from as u32..to as u32
        };
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for ty in 0..height {
            let ys = span(ty, height, self.height);
            for tx in 0..width {
                let xs = span(tx, width, self.width);
                let mut sum = [0u32; 4];
                for y in ys.clone() {
                    for x in xs.clone() {
                        let p = self.at(x, y);
                        for (s, &v) in sum.iter_mut().zip(&self.rgba[p..p + 4]) {
                            *s += u32::from(v);
                        }
                    }
                }
                let n = xs.len() as u32 * ys.len() as u32;
                rgba.extend(sum.map(|s| (s / n) as u8));
            }
        }
        Image {
            width,
            height,
            rgba,
        }
    }

    /// Pixel bounds `(x0, y0, x1, y1)` of a fractional rectangle, clamped to the image.
    fn pixels_of(&self, r: &Rect) -> (u32, u32, u32, u32) {
        let edge = |v: f64, size: u32| (v.clamp(0.0, 1.0) * f64::from(size)).round() as u32;
        let (x0, y0) = (edge(r.x, self.width), edge(r.y, self.height));
        let (x1, y1) = (
            edge(r.x + r.width, self.width),
            edge(r.y + r.height, self.height),
        );
        (x0, y0, x1.max(x0), y1.max(y0))
    }

    fn at(&self, x: u32, y: u32) -> usize {
        (y as usize * self.width as usize + x as usize) * 4
    }
}

fn expand(
    buf: &[u8],
    channels: usize,
    pixels: usize,
    to_rgba: impl Fn(&[u8]) -> [u8; 4],
) -> Vec<u8> {
    buf.chunks_exact(channels)
        .take(pixels)
        .flat_map(to_rgba)
        .collect()
}

/// The two images at one size: the larger one scaled down to the smaller.
fn same_size<'a>(
    a: &'a Image,
    b: &'a Image,
) -> Result<(std::borrow::Cow<'a, Image>, std::borrow::Cow<'a, Image>), ShapeMismatch> {
    use std::borrow::Cow::{Borrowed, Owned};
    if (a.width, a.height) == (b.width, b.height) {
        return Ok((Borrowed(a), Borrowed(b)));
    }
    if a.width == 0 || a.height == 0 || b.width == 0 || b.height == 0 {
        return Err(ShapeMismatch);
    }
    let ratio = |i: &Image| f64::from(i.width) / f64::from(i.height);
    if (ratio(a) - ratio(b)).abs() / ratio(b) > ASPECT_TOLERANCE {
        return Err(ShapeMismatch);
    }
    if u64::from(a.width) * u64::from(a.height) > u64::from(b.width) * u64::from(b.height) {
        Ok((Owned(a.resized(b.width, b.height)), Borrowed(b)))
    } else {
        Ok((Borrowed(a), Owned(b.resized(a.width, a.height))))
    }
}

/// How two versions of a picture differ, pixel by pixel.
#[derive(Debug, Clone, PartialEq)]
pub struct PictureDiff {
    /// Share of pixels that are the same, 0–1.
    pub similarity: f64,
    pub differing: u64,
    /// PNG of the newer version faded, with the differing pixels in red.
    pub png: Vec<u8>,
}

/// Compares two versions of a reference picture exactly, as a check with no tolerance would.
/// `None` when either isn't a readable PNG or they have different shapes.
pub fn picture_diff(before: &[u8], after: &[u8]) -> Option<PictureDiff> {
    let (before, after) = (Image::decode(before).ok()?, Image::decode(after).ok()?);
    let comparison = after.compare(&before, None, &[], 0).ok()?;
    Some(PictureDiff {
        similarity: comparison.similarity,
        differing: comparison.differing,
        png: comparison.diff.encode().ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, rgb: [u8; 3]) -> Image {
        Image {
            width,
            height,
            rgba: [rgb[0], rgb[1], rgb[2], 255].repeat((width * height) as usize),
        }
    }

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn png_round_trip() {
        let mut img = solid(3, 2, [10, 20, 30]);
        img.rgba[4..8].copy_from_slice(&[1, 2, 3, 4]);
        assert_eq!(Image::decode(&img.encode().unwrap()).unwrap(), img);
        assert!(Image::decode(b"not a png").is_err());
    }

    #[test]
    fn masks_area_and_tolerance() {
        let reference = solid(10, 10, [200, 200, 200]);
        let mut shot = reference.clone();
        // Top-left 5×5 quarter changes.
        for y in 0..5 {
            for x in 0..5 {
                let p = shot.at(x, y);
                shot.rgba[p] = 0;
            }
        }
        let c = shot.compare(&reference, None, &[], 0).unwrap();
        assert_eq!(c.differing, 25);
        assert!((c.similarity - 0.75).abs() < 1e-9);
        assert_eq!(&c.diff.rgba[..4], &[230, 20, 60, 255]);

        let masked = shot
            .compare(&reference, None, &[rect(0.0, 0.0, 0.5, 0.5)], 0)
            .unwrap();
        assert_eq!(masked.differing, 0);
        let outside = shot
            .compare(&reference, Some(&rect(0.5, 0.5, 0.5, 0.5)), &[], 0)
            .unwrap();
        assert_eq!(outside.similarity, 1.0);
        assert_eq!(
            shot.compare(&reference, None, &[], 255).unwrap().differing,
            0
        );
    }

    #[test]
    fn retina_screenshot_matches_one_x_reference() {
        let reference = solid(4, 3, [50, 100, 150]);
        let shot = solid(8, 6, [50, 100, 150]);
        let c = shot.compare(&reference, None, &[], 0).unwrap();
        assert_eq!((c.diff.width, c.differing), (4, 0));
        assert!(
            solid(8, 3, [0, 0, 0])
                .compare(&reference, None, &[], 0)
                .is_err()
        );
    }

    #[test]
    fn crop_and_fit_keep_the_picture_readable() {
        let mut img = solid(8, 4, [0, 0, 0]);
        let p = img.at(6, 1);
        img.rgba[p..p + 3].copy_from_slice(&[255, 255, 255]);
        let right = img.crop(&rect(0.5, 0.0, 0.5, 1.0));
        assert_eq!((right.width, right.height), (4, 4));
        assert_eq!(
            &right.rgba[right.at(2, 1)..right.at(2, 1) + 3],
            &[255, 255, 255]
        );
        let sliver = img.crop(&rect(1.0, 1.0, 0.0, 0.0));
        assert_eq!((sliver.width, sliver.height), (1, 1));

        assert_eq!(img.fit(8).rgba, img.rgba);
        let small = img.fit(4);
        assert_eq!((small.width, small.height), (4, 2));
        assert_eq!(
            &small.rgba[small.at(3, 0)..small.at(3, 0) + 4],
            &[63, 63, 63, 255]
        );
        let line = solid(1000, 1, [9, 9, 9]).fit(10);
        assert_eq!((line.width, line.height), (10, 1));
    }

    #[test]
    fn picture_versions_show_their_differing_pixels() {
        let before = solid(4, 2, [10, 20, 30]);
        let mut after = before.clone();
        after.rgba[0] = 11;
        let diff = picture_diff(&before.encode().unwrap(), &after.encode().unwrap()).unwrap();
        assert_eq!(diff.differing, 1);
        assert_eq!(diff.similarity, 7.0 / 8.0);
        let shown = Image::decode(&diff.png).unwrap();
        assert_eq!((shown.width, shown.height), (4, 2));
        assert_eq!(shown.rgba[..4], [230, 20, 60, 255]);

        let tall = solid(2, 4, [10, 20, 30]).encode().unwrap();
        assert_eq!(picture_diff(&before.encode().unwrap(), &tall), None);
        assert_eq!(picture_diff(b"not a picture", &tall), None);
    }
}
