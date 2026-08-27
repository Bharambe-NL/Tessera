//! The sketch raster path. Doc 12 phase 9.
//!
//! Ink is stored as strokes: a colour, a width and a list of points. Doc 07
//! section A2 has the Reader read an Image row, and doc 07 section A4's packet
//! carries a `blob_ref` and a mime type. So something has to turn the strokes a
//! person drew into a picture a vision model can look at, and this is it.
//!
//! Deliberately plain. No anti aliasing beyond a round brush, no curve fitting,
//! no smoothing: the strokes are already the path the hand took, and every
//! transformation between the hand and the model is a chance for the model to
//! read something the person did not draw.
//!
//! Doc 07 section A6's preprocessing is "downscale to the vision alias limit,
//! contrast normalise for sketches". The first is here as a bound on the output
//! size. The second is a no op on ink, which is drawn at full contrast on white
//! by construction; it belongs with the scanned page path, which needs a real
//! image pipeline.

use serde::{Deserialize, Serialize};

/// The longest edge a rasterised sketch is written at.
///
/// Doc 07 section A6 downscales to the vision alias limit. 1568 is Anthropic's
/// long edge before its own downscale, so a larger image costs tokens to have
/// the provider throw the detail away again.
pub const MAX_EDGE: u32 = 1568;

/// Padding around the drawn bounds, so a stroke at the edge is not clipped and
/// a model is not asked to judge whether a line continues off the page.
const MARGIN: f64 = 24.0;

/// One drawn stroke. Mirrors the generator's shape in `gen/boards.py`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stroke {
    #[serde(default)]
    pub colour: String,
    #[serde(default = "default_width")]
    pub width: f64,
    pub points: Vec<(f64, f64)>,
}

fn default_width() -> f64 {
    3.0
}

/// A rasterised sketch: the png bytes and the size they were written at.
pub struct Raster {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum RasterError {
    #[error("nothing was drawn")]
    Empty,
    #[error("png: {0}")]
    Encode(String),
}

/// Draw the strokes onto white and encode a png.
///
/// The image is cropped to what was drawn rather than to a fixed canvas: a
/// person who sketched a small table in the corner of a large board should not
/// hand a vision model a page that is nine tenths empty.
pub fn rasterise(strokes: &[Stroke]) -> Result<Raster, RasterError> {
    let points: Vec<(f64, f64)> = strokes.iter().flat_map(|s| s.points.iter().copied()).collect();
    if points.is_empty() {
        return Err(RasterError::Empty);
    }

    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (x, y) in &points {
        min_x = min_x.min(*x);
        min_y = min_y.min(*y);
        max_x = max_x.max(*x);
        max_y = max_y.max(*y);
    }

    let drawn_w = (max_x - min_x).max(1.0) + MARGIN * 2.0;
    let drawn_h = (max_y - min_y).max(1.0) + MARGIN * 2.0;
    // One scale for both axes, because a sketch stretched on one of them is a
    // different drawing.
    let scale = (MAX_EDGE as f64 / drawn_w.max(drawn_h)).min(1.0);

    let width = ((drawn_w * scale).round() as u32).max(1);
    let height = ((drawn_h * scale).round() as u32).max(1);

    // Greyscale, because ink is one colour and a vision model gains nothing from
    // three channels of the same number. It is also a third of the bytes.
    let mut pixels = vec![255u8; (width as usize) * (height as usize)];
    let place = |x: f64, y: f64| ((x - min_x + MARGIN) * scale, (y - min_y + MARGIN) * scale);

    for stroke in strokes {
        let radius = (stroke.width * scale / 2.0).max(0.6);
        let ink = ink_level(&stroke.colour);
        for pair in stroke.points.windows(2) {
            let (a, b) = (place(pair[0].0, pair[0].1), place(pair[1].0, pair[1].1));
            line(&mut pixels, width, height, a, b, radius, ink);
        }
        // A stroke of one point is a dot, and a dot is something someone drew.
        if stroke.points.len() == 1 {
            let a = place(stroke.points[0].0, stroke.points[0].1);
            line(&mut pixels, width, height, a, a, radius, ink);
        }
    }

    encode(&pixels, width, height).map(|bytes| Raster { bytes, width, height })
}

/// How dark a stroke draws. Colours are OKLCH strings from the token set and
/// parsing them to judge ink weight would be a colour pipeline nobody needs:
/// what matters to a reader is that a line is there.
fn ink_level(colour: &str) -> u8 {
    if colour.contains("0.7") || colour.contains("0.8") {
        90
    } else {
        20
    }
}

/// A round brush along a segment, sampled at half a pixel.
fn line(pixels: &mut [u8], width: u32, height: u32, a: (f64, f64), b: (f64, f64), radius: f64, ink: u8) {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let length = (dx * dx + dy * dy).sqrt();
    let steps = (length * 2.0).ceil().max(1.0) as usize;

    for step in 0..=steps {
        let t = step as f64 / steps as f64;
        dot(pixels, width, height, a.0 + dx * t, a.1 + dy * t, radius, ink);
    }
}

fn dot(pixels: &mut [u8], width: u32, height: u32, cx: f64, cy: f64, radius: f64, ink: u8) {
    let r = radius.ceil() as i64;
    let (cx_i, cy_i) = (cx.round() as i64, cy.round() as i64);

    for y in (cy_i - r)..=(cy_i + r) {
        for x in (cx_i - r)..=(cx_i + r) {
            if x < 0 || y < 0 || x >= width as i64 || y >= height as i64 {
                continue;
            }
            let d = (((x as f64 - cx).powi(2)) + ((y as f64 - cy).powi(2))).sqrt();
            if d > radius + 0.5 {
                continue;
            }
            // A soft edge over the outer half pixel, which is the whole of the
            // anti aliasing: enough that a diagonal does not read as stairs.
            let coverage = (radius + 0.5 - d).clamp(0.0, 1.0);
            let index = (y as usize) * (width as usize) + (x as usize);
            let existing = pixels[index] as f64;
            let painted = existing * (1.0 - coverage) + (ink as f64) * coverage;
            pixels[index] = painted.round().clamp(0.0, 255.0) as u8;
        }
    }
}

fn encode(pixels: &[u8], width: u32, height: u32) -> Result<Vec<u8>, RasterError> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| RasterError::Encode(e.to_string()))?;
        writer
            .write_image_data(pixels)
            .map_err(|e| RasterError::Encode(e.to_string()))?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(points: &[(f64, f64)]) -> Stroke {
        Stroke {
            colour: "oklch(0.24 0.01 80)".into(),
            width: 3.0,
            points: points.to_vec(),
        }
    }

    #[test]
    fn nothing_drawn_is_an_error_rather_than_a_blank_page() {
        // A blank png handed to a vision model bills for a call that can only
        // answer "unrecognised".
        assert!(matches!(rasterise(&[]), Err(RasterError::Empty)));
        assert!(matches!(rasterise(&[stroke(&[])]), Err(RasterError::Empty)));
    }

    #[test]
    fn the_image_is_cropped_to_what_was_drawn() {
        // A small sketch in the corner of a large board should not become a page
        // that is nine tenths empty.
        let r = rasterise(&[stroke(&[(1000.0, 1000.0), (1040.0, 1000.0)])]).expect("raster");
        assert!(r.width < 120, "width {}", r.width);
        assert!(r.height < 80, "height {}", r.height);
        assert_eq!(&r.bytes[1..4], b"PNG");
    }

    #[test]
    fn a_long_edge_is_bounded_and_the_aspect_survives() {
        // Doc 07 section A6 downscales to the vision alias limit, and one scale
        // for both axes because a sketch stretched on one is a different
        // drawing.
        let r = rasterise(&[stroke(&[(0.0, 0.0), (6000.0, 3000.0)])]).expect("raster");
        assert!(r.width <= MAX_EDGE, "width {}", r.width);
        assert!(r.height <= MAX_EDGE);
        let ratio = r.width as f64 / r.height as f64;
        assert!((1.9..2.1).contains(&ratio), "aspect {ratio}");
    }

    #[test]
    fn the_ink_actually_lands_on_the_page() {
        // The failure this guards is a rasteriser that encodes a valid, empty
        // png: every check above would still pass.
        let r = rasterise(&[stroke(&[(0.0, 0.0), (100.0, 0.0)])]).expect("raster");
        let decoder = png::Decoder::new(std::io::Cursor::new(&r.bytes));
        let mut reader = decoder.read_info().expect("read");
        let mut buf = vec![0; reader.output_buffer_size().expect("size")];
        let info = reader.next_frame(&mut buf).expect("frame");
        let dark = buf[..info.buffer_size()].iter().filter(|p| **p < 128).count();
        assert!(dark > 50, "only {dark} dark pixels, so nothing was drawn");
    }

    #[test]
    fn two_rasters_of_one_sketch_are_identical() {
        // Doc 02 section 10.1's reproducibility, applied here: a sketch that
        // rasterises differently twice would make a Reader score irreproducible
        // for a reason that has nothing to do with the Reader.
        let ink = [stroke(&[(0.0, 0.0), (50.0, 20.0), (90.0, 5.0)])];
        assert_eq!(
            rasterise(&ink).expect("a").bytes,
            rasterise(&ink).expect("b").bytes
        );
    }
}
