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
            .ok_or_else(|| jevision_l10n::tr!("vision.image.tooLarge"))?;
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

    /// Where a scaled copy of `part` shows in this picture, in fractions of it: the screen of a
    /// simulated device in the window that shows it, with the frame and title bar around it.
    /// `None` when no place looks like `part`, or `part` is too plain to tell where it is.
    pub fn find_scaled(&self, part: &Image) -> Option<Rect> {
        let aspect = f64::from(part.height) / f64::from(part.width);
        let fine = Grid::of(part, 48, aspect);
        if fine.spread() < MIN_SPREAD {
            return None;
        }
        let coarse = Grid::of(part, 12, aspect);
        let sums = Sums::of(self);
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        let fits = |x: f64, y: f64, width: f64| {
            x >= 0.0 && y >= 0.0 && width >= 1.0 && x + width <= w && y + width * aspect <= h
        };
        // Every place a fiftieth of the window apart, from the widest copy down to a third of
        // the window: the screen fills most of a simulator window.
        let step = w / 50.0;
        let mut best: Option<(f64, f64, f64, f64)> = None;
        let mut width = w.min(h / aspect);
        while width >= w / 3.0 {
            let mut y = 0.0;
            while fits(0.0, y, width) {
                let mut x = 0.0;
                while fits(x, y, width) {
                    let score = sums.difference(&coarse, x, y, width);
                    if best.is_none_or(|(s, ..)| score < s) {
                        best = Some((score, x, y, width));
                    }
                    x += step;
                }
                y += step;
            }
            width -= step;
        }
        // Then closer, in ever smaller steps, down to a quarter of a pixel.
        let (_, mut x, mut y, mut width) = best?;
        let mut score = sums.difference(&fine, x, y, width);
        let mut step = step;
        while step >= 0.25 {
            let mut moved = false;
            for dw in [-step, 0.0, step] {
                for dx in [-step, 0.0, step] {
                    for dy in [-step, 0.0, step] {
                        let (cx, cy, cw) = (x + dx, y + dy, width + dw);
                        if !fits(cx, cy, cw) {
                            continue;
                        }
                        let s = sums.difference(&fine, cx, cy, cw);
                        if s < score {
                            (score, x, y, width, moved) = (s, cx, cy, cw, true);
                        }
                    }
                }
            }
            if !moved {
                step /= 2.0;
            }
        }
        (score <= MAX_DIFFERENCE).then(|| Rect {
            x: x / w,
            y: y / h,
            width: width / w,
            height: width * aspect / h,
        })
    }

    /// Box-filtered copy at another size.
    pub fn resized(&self, width: u32, height: u32) -> Image {
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

/// Least spread of brightness, on a 0–255 scale, of a picture [`Image::find_scaled`] can place:
/// a plain one, such as a screen still black while the device starts, looks alike anywhere.
const MIN_SPREAD: f64 = 6.0;

/// Largest mean difference in brightness, on a 0–255 scale, between a picture and the place
/// [`Image::find_scaled`] puts it: scaling softens edges, the rest is the same.
const MAX_DIFFERENCE: f64 = 12.0;

fn brightness(rgba: &[u8]) -> f64 {
    0.299 * f64::from(rgba[0]) + 0.587 * f64::from(rgba[1]) + 0.114 * f64::from(rgba[2])
}

/// A picture shrunk to a few cells across, as brightness.
struct Grid {
    columns: usize,
    rows: usize,
    /// Height over width of the picture.
    aspect: f64,
    cells: Vec<f64>,
}

impl Grid {
    fn of(image: &Image, columns: u32, aspect: f64) -> Self {
        let rows = ((f64::from(columns) * aspect).round() as u32).max(1);
        let small = image.resized(columns, rows);
        Self {
            columns: columns as usize,
            rows: rows as usize,
            aspect,
            cells: small
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| brightness(p))
                .collect(),
        }
    }

    /// Mean distance of the cells from their mean.
    fn spread(&self) -> f64 {
        let n = self.cells.len() as f64;
        let mean = self.cells.iter().sum::<f64>() / n;
        self.cells.iter().map(|c| (c - mean).abs()).sum::<f64>() / n
    }
}

/// Running sums of brightness, for the mean of any box in constant time.
struct Sums {
    width: usize,
    height: usize,
    sums: Vec<f64>,
}

impl Sums {
    fn of(image: &Image) -> Self {
        let (width, height) = (image.width as usize, image.height as usize);
        let mut sums = vec![0.0; (width + 1) * (height + 1)];
        for y in 0..height {
            let mut row = 0.0;
            for x in 0..width {
                row += brightness(&image.rgba[(y * width + x) * 4..]);
                sums[(y + 1) * (width + 1) + x + 1] = sums[y * (width + 1) + x + 1] + row;
            }
        }
        Self {
            width,
            height,
            sums,
        }
    }

    fn mean(&self, x0: f64, y0: f64, x1: f64, y1: f64) -> f64 {
        let edge = |v: f64, size: usize| (v.round().max(0.0) as usize).min(size);
        let (x0, y0) = (edge(x0, self.width), edge(y0, self.height));
        let x1 = edge(x1, self.width).max(x0 + 1).min(self.width);
        let y1 = edge(y1, self.height).max(y0 + 1).min(self.height);
        let (x0, y0) = (x0.min(x1 - 1), y0.min(y1 - 1));
        let at = |x: usize, y: usize| self.sums[y * (self.width + 1) + x];
        (at(x1, y1) - at(x0, y1) - at(x1, y0) + at(x0, y0)) / ((x1 - x0) * (y1 - y0)) as f64
    }

    /// Mean difference between `grid` and the box `width` wide at `x`, `y` cut into as many
    /// cells.
    fn difference(&self, grid: &Grid, x: f64, y: f64, width: f64) -> f64 {
        let cell_width = width / grid.columns as f64;
        let cell_height = width * grid.aspect / grid.rows as f64;
        let mut total = 0.0;
        for row in 0..grid.rows {
            let top = y + row as f64 * cell_height;
            for column in 0..grid.columns {
                let left = x + column as f64 * cell_width;
                let seen = self.mean(left, top, left + cell_width, top + cell_height);
                total += (seen - grid.cells[row * grid.columns + column]).abs();
            }
        }
        total / grid.cells.len() as f64
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

    /// A screen of coloured tiles, the same every time.
    fn tiles(width: u32, height: u32, seed: u32) -> Image {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let tile = (x / 12).wrapping_mul(31) ^ (y / 12).wrapping_mul(17) ^ seed;
                let v = tile.wrapping_mul(2_654_435_761) >> 8;
                rgba.extend([v as u8, (v >> 8) as u8, (v >> 16) as u8, 255]);
            }
        }
        Image {
            width,
            height,
            rgba,
        }
    }

    /// A simulator window: a title bar, a dark frame and `screen` at `width` pixels wide
    /// placed at `x`, `y`.
    fn window(screen: &Image, x: u32, y: u32, width: u32) -> Image {
        let mut w = solid(width + 2 * x, 2 * y + width * 22 / 10, [28, 28, 30]);
        for p in 0..(w.width * 30) as usize {
            w.rgba[p * 4..p * 4 + 3].copy_from_slice(&[236, 236, 236]);
        }
        let height = width * screen.height / screen.width;
        let scaled = screen.resized(width, height);
        for row in 0..height {
            let from = (row * width * 4) as usize;
            let to = w.at(x, y + row);
            w.rgba[to..to + (width * 4) as usize]
                .copy_from_slice(&scaled.rgba[from..from + (width * 4) as usize]);
        }
        w
    }

    #[test]
    fn a_device_screen_is_found_in_its_window_at_any_zoom() {
        let screen = tiles(390, 844, 7);
        for (x, y, width) in [(24, 64, 300), (40, 90, 195), (12, 40, 390)] {
            let w = window(&screen, x, y, width);
            let found = w.find_scaled(&screen).expect("the screen is in the window");
            let (ww, wh) = (f64::from(w.width), f64::from(w.height));
            let off = |a: f64, b: u32| (a - f64::from(b)).abs();
            assert!(
                off(found.x * ww, x) <= 1.0,
                "x of {found:?} at zoom {width}"
            );
            assert!(
                off(found.y * wh, y) <= 1.0,
                "y of {found:?} at zoom {width}"
            );
            assert!(off(found.width * ww, width) <= 1.5, "width of {found:?}");
        }
    }

    #[test]
    fn plain_or_other_screens_are_not_placed() {
        let w = window(&tiles(390, 844, 7), 24, 64, 300);
        assert_eq!(w.find_scaled(&solid(390, 844, [0, 0, 0])), None);
        assert_eq!(w.find_scaled(&tiles(390, 844, 99)), None);
    }
}
