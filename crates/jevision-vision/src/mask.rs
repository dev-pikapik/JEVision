//! Covering private parts of a screenshot before it leaves JEVision.
//!
//! [`MaskedImage`] is the only kind of picture a [`Question`](crate::Question) carries, and its
//! constructors are the only way to make one: each paints solid blocks over the areas it is
//! given first. So no picture can reach an AI model without going through them.
//!
//! ```compile_fail
//! // A masked picture can't be made from raw bytes, so unmasked pixels can't be sent.
//! let picture = jevision_vision::MaskedImage { png: vec![] };
//! ```
//!
//! ```compile_fail
//! // A question takes a masked picture, not PNG bytes.
//! let question = jevision_vision::Question {
//!     expect: "",
//!     screenshot: &b"\x89PNG"[..],
//!     reference: None,
//!     model: "",
//! };
//! ```

use jevision_core::Rect;

use crate::image::Image;

/// Colour painted over private areas: flat and the same everywhere, so nothing of what was
/// underneath, not even its average colour, is left.
pub const MASK_COLOUR: [u8; 4] = [128, 128, 128, 255];

/// A PNG picture whose private areas have been painted over.
#[derive(Clone, PartialEq, Eq)]
pub struct MaskedImage {
    png: Vec<u8>,
    masked: bool,
}

impl std::fmt::Debug for MaskedImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MaskedImage")
            .field("bytes", &self.png.len())
            .field("masked", &self.masked)
            .finish()
    }
}

impl MaskedImage {
    /// `image` with `areas` (fractions of the picture, from its top-left corner) painted over.
    pub fn new(image: &Image, areas: &[Rect]) -> Result<Self, String> {
        Self::view(image, areas, None, u32::MAX)
    }

    /// `image` with `areas` painted over, then cut to `crop` and scaled down to at most
    /// `max_side` pixels on its longer side. Painting comes first, so neither cutting nor
    /// scaling can bring back a pixel of a covered area.
    pub fn view(
        image: &Image,
        areas: &[Rect],
        crop: Option<&Rect>,
        max_side: u32,
    ) -> Result<Self, String> {
        let mut image = image.clone();
        let masked = paint(&mut image, areas);
        if let Some(crop) = crop {
            image = image.crop(crop);
        }
        Ok(Self {
            png: image.fit(max_side).encode()?,
            masked,
        })
    }

    /// The PNG picture `png` with `areas` painted over. Without areas it is passed on as it is.
    pub fn from_png(png: &[u8], areas: &[Rect]) -> Result<Self, String> {
        if areas.is_empty() {
            return Ok(Self {
                png: png.to_vec(),
                masked: false,
            });
        }
        Self::new(&Image::decode(png)?, areas)
    }

    pub fn png(&self) -> &[u8] {
        &self.png
    }

    /// Some part of the picture was painted over.
    pub fn is_masked(&self) -> bool {
        self.masked
    }
}

/// Paints [`MASK_COLOUR`] over every area, widened to whole pixels and clipped to the picture.
/// Returns whether any pixel was painted.
fn paint(image: &mut Image, areas: &[Rect]) -> bool {
    let (width, height) = (image.width, image.height);
    let mut painted = false;
    for area in areas {
        let (Some((x0, x1)), Some((y0, y1))) = (
            span(area.x, area.width, width),
            span(area.y, area.height, height),
        ) else {
            continue;
        };
        let (x0, x1) = (x0 as usize, x1 as usize);
        for y in y0 as usize..y1 as usize {
            let row = y * width as usize;
            for pixel in image.rgba[(row + x0) * 4..(row + x1) * 4].chunks_exact_mut(4) {
                pixel.copy_from_slice(&MASK_COLOUR);
            }
        }
        painted = true;
    }
    painted
}

/// The whole pixels from `start` to `start + length` (fractions of `size` pixels), widened
/// outwards and clipped to `0..size`; `None` when nothing is left.
fn span(start: f64, length: f64, size: u32) -> Option<(u32, u32)> {
    let size = f64::from(size);
    let from = (start * size).floor().clamp(0.0, size);
    let to = ((start + length) * size).ceil().clamp(0.0, size);
    // NaN fails every comparison and is left out.
    (from < to).then_some((from as u32, to as u32))
}

/// A window of the app in a video frame and the parts of it to cover.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoWindow {
    /// Where the window is, in the same points as the parts of the screen a frame shows.
    pub bounds: Rect,
    /// Areas to cover, in fractions of the window from its top-left corner, like the areas a
    /// screenshot of the window has covered.
    pub hide: Vec<Rect>,
}

/// A block of a video frame, in whole pixels from its top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelArea {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// What to paint over one video frame, and [`MASK_COLOUR`] in the frame's own encoding:
/// video-range BT.709 Y′CbCr, as H.264 stores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoMask {
    pub areas: Vec<PixelArea>,
    pub luma: u8,
    pub chroma_blue: u8,
    pub chroma_red: u8,
}

/// The blocks to paint over a `width`×`height` frame that shows `shown`, a part of the screen in
/// points, scaled to fill the frame. Give several parts while the frame moves from one to
/// another: whichever the frame shows, its private areas are covered.
///
/// Blocks are widened to even pixels: a frame keeps one colour sample for every 2×2 pixels, so a
/// block that ended mid-pair would leave a fringe of the covered colour.
pub fn video_mask(width: u32, height: u32, shown: &[Rect], windows: &[VideoWindow]) -> VideoMask {
    let mut areas = Vec::new();
    for view in shown.iter().filter(|v| v.width > 0.0 && v.height > 0.0) {
        for window in windows {
            let b = &window.bounds;
            for hide in &window.hide {
                let x = (b.x + hide.x * b.width - view.x) / view.width;
                let y = (b.y + hide.y * b.height - view.y) / view.height;
                let (Some((x0, x1)), Some((y0, y1))) = (
                    span(x, hide.width * b.width / view.width, width),
                    span(y, hide.height * b.height / view.height, height),
                ) else {
                    continue;
                };
                let even = |from: u32, to: u32, size: u32| (from & !1, (to + (to & 1)).min(size));
                let ((x0, x1), (y0, y1)) = (even(x0, x1, width), even(y0, y1, height));
                areas.push(PixelArea {
                    x: x0,
                    y: y0,
                    width: x1 - x0,
                    height: y1 - y0,
                });
            }
        }
    }
    let [luma, chroma_blue, chroma_red] = video_colour(MASK_COLOUR);
    VideoMask {
        areas,
        luma,
        chroma_blue,
        chroma_red,
    }
}

/// `rgba` as video-range BT.709 Y′CbCr.
fn video_colour([r, g, b, _]: [u8; 4]) -> [u8; 3] {
    let (r, g, b) = (f64::from(r), f64::from(g), f64::from(b));
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let scale = |v: f64, range: f64| v * range / 255.0;
    [
        (16.0 + scale(y, 219.0)).round() as u8,
        (128.0 + scale((b - y) / 1.8556, 224.0)).round() as u8,
        (128.0 + scale((r - y) / 1.5748, 224.0)).round() as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// A picture whose every pixel has its own colour, so any change shows.
    fn gradient(width: u32, height: u32) -> Image {
        let mut rgba = Vec::new();
        for y in 0..height {
            for x in 0..width {
                rgba.extend([x as u8, y as u8, 7, 255]);
            }
        }
        Image {
            width,
            height,
            rgba,
        }
    }

    fn decoded(m: &MaskedImage) -> Image {
        Image::decode(m.png()).unwrap()
    }

    /// Pixels that differ between `a` and `b`, as `(x, y)`.
    fn changed(a: &Image, b: &Image) -> Vec<(u32, u32)> {
        assert_eq!((a.width, a.height), (b.width, b.height));
        let mut out = Vec::new();
        for y in 0..a.height {
            for x in 0..a.width {
                let p = ((y * a.width + x) * 4) as usize;
                if a.rgba[p..p + 4] != b.rgba[p..p + 4] {
                    out.push((x, y));
                }
            }
        }
        out
    }

    #[test]
    fn paints_only_inside_the_areas() {
        let original = gradient(10, 8);
        let masked = MaskedImage::new(&original, &[rect(0.2, 0.25, 0.3, 0.5)]).unwrap();
        assert!(masked.is_masked());
        let after = decoded(&masked);
        let mut expected = Vec::new();
        for y in 2..6 {
            for x in 2..5 {
                expected.push((x, y));
            }
        }
        assert_eq!(changed(&original, &after), expected);
        let p = ((3 * 10 + 3) * 4) as usize;
        assert_eq!(after.rgba[p..p + 4], MASK_COLOUR);
    }

    #[test]
    fn partly_covered_pixels_are_covered_whole() {
        let original = gradient(10, 10);
        // From x 1.5 px to 2.5 px: pixels 1 and 2 are both touched.
        let after = decoded(&MaskedImage::new(&original, &[rect(0.15, 0.0, 0.1, 0.1)]).unwrap());
        assert_eq!(changed(&original, &after), [(1, 0), (2, 0)]);
    }

    #[test]
    fn areas_past_the_edges_are_clipped() {
        let original = gradient(6, 4);
        let areas = [
            rect(-0.5, -0.5, 0.75, 0.75),
            rect(0.9, 0.9, 1.0, 1.0),
            rect(2.0, 2.0, 0.5, 0.5),
            rect(f64::NAN, 0.0, 0.5, 0.5),
            rect(0.5, 0.5, 0.0, 0.5),
        ];
        let after = decoded(&MaskedImage::new(&original, &areas).unwrap());
        // Top-left: x 0..2, y 0..1. Bottom-right: x 5, y 3.
        assert_eq!(changed(&original, &after), [(0, 0), (1, 0), (5, 3)]);

        let outside = MaskedImage::new(&original, &areas[2..]).unwrap();
        assert!(!outside.is_masked());
        assert_eq!(decoded(&outside), original);
    }

    #[test]
    fn areas_follow_the_picture_at_any_scale() {
        // The same window at 1× and on a Retina display: the area covers the same part.
        let area = [rect(0.25, 0.5, 0.5, 0.25)];
        for (scale, width, height) in [(1, 8, 8), (2, 16, 16), (3, 24, 24)] {
            let original = gradient(width, height);
            let after = decoded(&MaskedImage::new(&original, &area).unwrap());
            let changed = changed(&original, &after);
            assert_eq!(changed.len(), (4 * 2 * scale * scale) as usize);
            assert_eq!(changed[0], (2 * scale, 4 * scale));
            assert_eq!(*changed.last().unwrap(), (6 * scale - 1, 6 * scale - 1));
        }
    }

    #[test]
    fn cutting_and_scaling_come_after_painting() {
        // White picture with a black secret in the middle, covered entirely.
        let mut original = Image {
            width: 40,
            height: 40,
            rgba: vec![255; 40 * 40 * 4],
        };
        for y in 10..30 {
            for x in 10..30 {
                let p = (y * 40 + x) * 4;
                original.rgba[p..p + 3].copy_from_slice(&[0, 0, 0]);
            }
        }
        let secret = [rect(0.25, 0.25, 0.5, 0.5)];
        let view =
            MaskedImage::view(&original, &secret, Some(&rect(0.0, 0.0, 0.5, 0.5)), 5).unwrap();
        let after = decoded(&view);
        assert_eq!((after.width, after.height), (5, 5));
        // No pixel, however scaled, is darker than the mask colour: the black never shows.
        assert!(
            after
                .rgba
                .chunks_exact(4)
                .all(|p| p[0] >= MASK_COLOUR[0] && p[1] >= MASK_COLOUR[1])
        );
    }

    #[test]
    fn png_without_areas_passes_unchanged() {
        let png = gradient(3, 3).encode().unwrap();
        let passed = MaskedImage::from_png(&png, &[]).unwrap();
        assert_eq!(passed.png(), png);
        assert!(!passed.is_masked());
        let masked = MaskedImage::from_png(&png, &[rect(0.0, 0.0, 1.0, 1.0)]).unwrap();
        assert!(
            decoded(&masked)
                .rgba
                .chunks_exact(4)
                .all(|p| p == MASK_COLOUR)
        );
        assert!(MaskedImage::from_png(b"not a picture", &[rect(0.0, 0.0, 1.0, 1.0)]).is_err());
    }

    fn window(bounds: Rect, hide: &[Rect]) -> VideoWindow {
        VideoWindow {
            bounds,
            hide: hide.to_vec(),
        }
    }

    fn area(x: u32, y: u32, width: u32, height: u32) -> PixelArea {
        PixelArea {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn video_mask_follows_the_window_into_the_frame() {
        // A 200×100 window at (100, 50) on a 400×200 part of the screen, filmed at 800×400.
        let shown = [rect(0.0, 0.0, 400.0, 200.0)];
        let windows = [window(
            rect(100.0, 50.0, 200.0, 100.0),
            &[rect(0.5, 0.5, 0.25, 0.25)],
        )];
        let mask = video_mask(800, 400, &shown, &windows);
        assert_eq!(mask.areas, [area(400, 200, 100, 50)]);
        // The frame closed in on the window: the same area, larger.
        let closer = video_mask(800, 400, &[rect(100.0, 50.0, 200.0, 100.0)], &windows);
        assert_eq!(closer.areas, [area(400, 200, 200, 100)]);
    }

    #[test]
    fn video_mask_widens_to_even_pixels_and_clips() {
        let shown = [rect(0.0, 0.0, 10.0, 10.0)];
        let whole = rect(0.0, 0.0, 10.0, 10.0);
        let mask = video_mask(
            10,
            10,
            &shown,
            &[window(whole, &[rect(0.15, 0.15, 0.1, 0.1)])],
        );
        assert_eq!(mask.areas, [area(0, 0, 4, 4)]);
        let edge = video_mask(
            10,
            10,
            &shown,
            &[window(whole, &[rect(0.9, 0.9, 0.5, 0.5)])],
        );
        assert_eq!(edge.areas, [area(8, 8, 2, 2)]);
    }

    #[test]
    fn video_mask_covers_every_shown_part_and_skips_what_is_off_frame() {
        let windows = [window(
            rect(0.0, 0.0, 100.0, 100.0),
            &[rect(0.0, 0.0, 0.5, 0.5), rect(f64::NAN, 0.0, 0.1, 0.1)],
        )];
        let moving = [rect(0.0, 0.0, 100.0, 100.0), rect(50.0, 50.0, 100.0, 100.0)];
        let mask = video_mask(100, 100, &moving, &windows);
        assert_eq!(mask.areas, [area(0, 0, 50, 50)]);
        assert!(
            video_mask(100, 100, &[rect(0.0, 0.0, 0.0, 100.0)], &windows)
                .areas
                .is_empty()
        );
        let away = [window(rect(500.0, 0.0, 100.0, 100.0), &windows[0].hide)];
        assert!(video_mask(100, 100, &moving[..1], &away).areas.is_empty());
    }

    #[test]
    fn video_mask_paints_the_mask_colour_in_video_range() {
        let mask = video_mask(2, 2, &[], &[]);
        assert_eq!(
            (mask.luma, mask.chroma_blue, mask.chroma_red),
            (126, 128, 128)
        );
        assert_eq!(video_colour([255, 255, 255, 255]), [235, 128, 128]);
        assert_eq!(video_colour([0, 0, 0, 255]), [16, 128, 128]);
        assert_eq!(video_colour([255, 0, 0, 255]), [63, 102, 240]);
    }
}
