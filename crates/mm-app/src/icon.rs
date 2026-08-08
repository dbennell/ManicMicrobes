// The window icon, drawn rather than decoded.
//
// **Not a `//!` comment in this file, and that is load-bearing.** `build.rs` pulls this module in
// with `include!` to generate the Windows `.ico`, because a build script cannot depend on the
// crate it is building. An inner doc comment is illegal in a macro expansion — E0753 — so the
// module's documentation lives on `pub mod icon;` in `lib.rs` and the reasoning lives here as
// ordinary comments. Put a `//!` back and only the Windows build breaks, eight minutes into a
// release.
//
// The same mark the website uses: a membrane, the bright arc across it, a nucleus and three
// organelles. `icon.svg` in the site repository is the original, and the numbers below are its
// numbers — a 32-unit square, so a coordinate here can be read straight off that file.
//
// Arithmetic rather than a PNG, because there is no decoder here to decode one. `mm-app` takes
// Bevy with `default-features = false` and does not enable `png`, so the dependency graph contains
// no image codec at all; pulling one in to unpack six circles would cost more than drawing them.
// It also means the icon renders at whatever size the platform asks for instead of at whatever
// size somebody exported, and it cannot go missing — the same reason `cell.wgsl` is embedded
// rather than loaded from an `assets/` directory.
//
// The same argument runs the other way for the two icons the operating system wants as *files*
// rather than as pixels at runtime — the `.ico` compiled into the Windows executable and the
// `.icns` inside a macOS bundle. Both are written from `rgba` rather than committed, by `png` and
// `ico` below, so there is one mark and nothing to keep in step with it. That is the whole reason
// there is a PNG encoder in a simulator: not to be one, but so that no part of the build has a
// picture of the icon that could disagree with this file.

/// `#5fd39b`. The producer green, and the one the site draws the mark in.
const MARK: [f32; 3] = [95.0 / 255.0, 211.0 / 255.0, 155.0 / 255.0];

/// `#06080a`. Near-black, and the tile the mark sits on.
const TILE: [f32; 3] = [6.0 / 255.0, 8.0 / 255.0, 10.0 / 255.0];

/// Corner radius of the tile, in the 32-unit space.
const TILE_RADIUS: f32 = 7.0;

/// Membrane: centre, radius and half the stroke width.
const MEMBRANE: (f32, f32, f32, f32) = (16.0, 16.0, 13.0, 0.8);

/// The lit arc, in degrees, measured from the positive x axis and increasing downwards — which
/// is the direction an SVG arc goes, so `rotate(-40)` and a 20-unit dash out of a 81.68-unit
/// circumference land here as a start and a sweep.
const ARC_START_DEG: f32 = -40.0;
const ARC_SWEEP_DEG: f32 = 360.0 * 20.0 / (2.0 * std::f32::consts::PI * 13.0);

/// Nucleus first, then the three organelles: centre, radius, opacity.
const DISCS: [(f32, f32, f32, f32); 4] = [
    (13.5, 14.0, 4.0, 0.90),
    (21.0, 12.5, 1.9, 0.50),
    (20.0, 20.5, 2.6, 0.68),
    (12.0, 21.5, 1.4, 0.40),
];

/// Samples per pixel per axis. The arc is a thin curve and the tile has a rounded corner; four
/// is enough that neither shows a staircase at 32 pixels, which is the smallest size anything
/// asks for.
const SUPERSAMPLE: u32 = 4;

/// The mark as straight-alpha RGBA rows, top to bottom — what `winit::window::Icon` wants.
pub fn rgba(size: u32) -> Vec<u8> {
    render(size, true)
}

/// The same, without the tile behind it.
///
/// For drawing the mark *inside* the application, where it sits on a panel that already has a
/// ground. The tile is what makes an icon read as an icon on somebody else's taskbar; on our own
/// menu bar it would be a near-black rounded square sitting on a near-black strip, which is a
/// block with a cell in it rather than a mark.
pub fn rgba_untiled(size: u32) -> Vec<u8> {
    render(size, false)
}

fn render(size: u32, tile: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    let scale = 32.0 / size as f32;
    let step = scale / SUPERSAMPLE as f32;
    let samples = (SUPERSAMPLE * SUPERSAMPLE) as f32;

    for py in 0..size {
        for px in 0..size {
            // Straight alpha, accumulated over the samples rather than composited per sample,
            // so a half-covered edge averages its colour instead of its coverage twice.
            let (mut r, mut g, mut b, mut a) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);

            for sy in 0..SUPERSAMPLE {
                for sx in 0..SUPERSAMPLE {
                    let x = (px as f32 * SUPERSAMPLE as f32 + sx as f32 + 0.5) * step;
                    let y = (py as f32 * SUPERSAMPLE as f32 + sy as f32 + 0.5) * step;
                    let (sr, sg, sb, sa) = sample(x, y, tile);
                    r += sr * sa;
                    g += sg * sa;
                    b += sb * sa;
                    a += sa;
                }
            }

            // Un-premultiply: the accumulation above weighted colour by coverage, which is what
            // makes a partly covered pixel the right colour rather than a dark fringe.
            let (r, g, b) = if a > 0.0 {
                (r / a, g / a, b / a)
            } else {
                (0.0, 0.0, 0.0)
            };
            let a = a / samples;

            out.push(to_byte(r));
            out.push(to_byte(g));
            out.push(to_byte(b));
            out.push(to_byte(a));
        }
    }

    out
}

/// One sample of the mark, in the 32-unit space. Returns straight-alpha colour.
fn sample(x: f32, y: f32, tile: bool) -> (f32, f32, f32, f32) {
    let mut colour = [0.0f32; 3];
    let mut alpha = 0.0f32;

    if tile && in_tile(x, y) {
        over(&mut colour, &mut alpha, TILE, 1.0);
    }

    let (cx, cy, radius, half) = MEMBRANE;
    let dx = x - cx;
    let dy = y - cy;
    let distance = (dx * dx + dy * dy).sqrt();

    // The membrane is the whole ring at low opacity; the arc is a stretch of the same ring at
    // full. Drawn in that order so the bright part sits on the faint one rather than beside it.
    if (distance - radius).abs() <= half {
        over(&mut colour, &mut alpha, MARK, 0.45);

        let mut angle = dy.atan2(dx).to_degrees() - ARC_START_DEG;
        while angle < 0.0 {
            angle += 360.0;
        }
        if angle <= ARC_SWEEP_DEG {
            over(&mut colour, &mut alpha, MARK, 1.0);
        }
    }

    // Round caps, as discs at the two ends. `stroke-linecap="round"` in one line of trigonometry
    // rather than a special case inside the sweep test above.
    for end in [ARC_START_DEG, ARC_START_DEG + ARC_SWEEP_DEG] {
        let (sin, cos) = end.to_radians().sin_cos();
        let ex = cx + cos * radius;
        let ey = cy + sin * radius;
        if ((x - ex).powi(2) + (y - ey).powi(2)).sqrt() <= half {
            over(&mut colour, &mut alpha, MARK, 1.0);
        }
    }

    for (dcx, dcy, r, opacity) in DISCS {
        if ((x - dcx).powi(2) + (y - dcy).powi(2)).sqrt() <= r {
            over(&mut colour, &mut alpha, MARK, opacity);
        }
    }

    (colour[0], colour[1], colour[2], alpha)
}

/// Inside the rounded tile? A rounded rectangle is the box inset by its radius, grown back out
/// by that radius in every direction — so the test is a distance to the inset box.
fn in_tile(x: f32, y: f32) -> bool {
    let dx = (TILE_RADIUS - x).max(x - (32.0 - TILE_RADIUS)).max(0.0);
    let dy = (TILE_RADIUS - y).max(y - (32.0 - TILE_RADIUS)).max(0.0);
    (dx * dx + dy * dy).sqrt() <= TILE_RADIUS
}

/// Source-over, on straight alpha.
fn over(dst: &mut [f32; 3], dst_a: &mut f32, src: [f32; 3], src_a: f32) {
    let out_a = src_a + *dst_a * (1.0 - src_a);
    if out_a <= 0.0 {
        *dst = [0.0; 3];
        *dst_a = 0.0;
        return;
    }
    for i in 0..3 {
        dst[i] = (src[i] * src_a + dst[i] * *dst_a * (1.0 - src_a)) / out_a;
    }
    *dst_a = out_a;
}

fn to_byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/* -------------------------------------------------------------------------------------------- */
/* Container formats                                                                              */
/* -------------------------------------------------------------------------------------------- */

/// The mark as a PNG.
///
/// Deflate's *stored* mode — every block copied verbatim, no compression at all. A real encoder
/// would be a dependency and several hundred lines to save a few tens of kilobytes on a file that
/// is already a rounding error beside an 80 MB binary. The output is a valid PNG either way,
/// because "stored" is a legal deflate block and every decoder handles it.
pub fn png(size: u32) -> Vec<u8> {
    let pixels = rgba(size);

    // The filter byte in front of each row. Zero — "no filter" — because filtering exists to make
    // the compression that is not happening here work better.
    let mut raw = Vec::with_capacity((size * (size * 4 + 1)) as usize);
    for y in 0..size {
        raw.push(0);
        let row = (y * size * 4) as usize;
        raw.extend_from_slice(&pixels[row..row + (size * 4) as usize]);
    }

    let mut out = Vec::new();
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8 bits a channel, RGBA, no interlace.
    chunk(&mut out, b"IHDR", &ihdr);

    chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

/// The mark as a Windows `.ico`, holding one PNG per size.
///
/// PNG-compressed entries rather than the older bitmap-and-mask ones: supported since Vista, and
/// the microscope wants a GPU from the last decade, so nothing that can run this cannot read it.
pub fn ico(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes.iter().map(|&s| png(s)).collect();

    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // 1 = icon, 2 = cursor
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());

    // Directory first, so the offsets have to be worked out before any image is written.
    let mut offset = 6 + 16 * images.len() as u32;
    for (&size, image) in sizes.iter().zip(&images) {
        // 256 is written as 0. The field is one byte, and 256 does not fit in one byte.
        let dimension = if size >= 256 { 0u8 } else { size as u8 };
        out.push(dimension);
        out.push(dimension);
        out.push(0); // palette size; 0 for truecolour
        out.push(0); // reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // colour planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits a pixel
        out.extend_from_slice(&(image.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += image.len() as u32;
    }

    for image in images {
        out.extend_from_slice(&image);
    }
    out
}

/// A PNG chunk: length, type, payload, and a CRC over the type and payload.
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);

    let mut crc = Crc::new();
    crc.eat(kind);
    crc.eat(data);
    out.extend_from_slice(&crc.finish().to_be_bytes());
}

/// A zlib stream of stored deflate blocks.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01]; // deflate, 32K window, no preset dictionary

    // A stored block's length is a u16, so anything longer is several blocks.
    const MAX: usize = 65535;
    let mut rest = data;
    loop {
        let take = rest.len().min(MAX);
        let (block, remainder) = rest.split_at(take);
        let last = remainder.is_empty();

        out.push(if last { 1 } else { 0 }); // BFINAL, and BTYPE 00 for stored
        out.extend_from_slice(&(take as u16).to_le_bytes());
        out.extend_from_slice(&(!(take as u16)).to_le_bytes()); // the ones' complement, as spec'd
        out.extend_from_slice(block);

        if last {
            break;
        }
        rest = remainder;
    }

    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

/// Adler-32, which is the checksum zlib ends a stream with.
fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// CRC-32, which is what a PNG chunk ends with. The table is built on first use rather than
/// written out, because a 256-entry constant is harder to check than the loop that makes it.
struct Crc(u32);

impl Crc {
    fn new() -> Self {
        Crc(0xffff_ffff)
    }

    fn eat(&mut self, data: &[u8]) {
        for &byte in data {
            let mut c = (self.0 ^ byte as u32) & 0xff;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            self.0 = c ^ (self.0 >> 8);
        }
    }

    fn finish(self) -> u32 {
        self.0 ^ 0xffff_ffff
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(buf: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }

    #[test]
    fn the_buffer_is_the_size_it_was_asked_for() {
        for size in [16u32, 32, 64, 256] {
            assert_eq!(rgba(size).len(), (size * size * 4) as usize);
        }
    }

    #[test]
    fn the_corner_is_transparent_and_the_middle_is_not() {
        let size = 64;
        let buf = rgba(size);
        assert_eq!(pixel(&buf, size, 0, 0)[3], 0, "the tile is rounded");
        assert_eq!(pixel(&buf, size, size / 2, size / 2)[3], 255);
    }

    /// The nucleus is the largest thing on the tile and the reason the mark reads as a cell at
    /// 16 pixels. If it stops being drawn in the producer green, the icon has silently become a
    /// dark square with a ring on it.
    #[test]
    fn the_nucleus_is_the_mark_colour() {
        let size = 64;
        let buf = rgba(size);
        // (13.5, 14) of 32, in pixels.
        let [r, g, b, a] = pixel(&buf, size, 27, 28);
        assert_eq!(a, 255);
        assert!(g > r && g > b, "green channel dominates: {r} {g} {b}");
        assert!(g > 150, "and it is the light green, not the dark: {g}");
    }

    /// Every pixel of the tile is opaque, so the icon never shows a hole where a compositor
    /// expected a background.
    #[test]
    fn the_tile_is_opaque_wherever_it_is_drawn() {
        let size = 64;
        let buf = rgba(size);
        let opaque = (0..size * size)
            .filter(|i| buf[(i * 4 + 3) as usize] == 255)
            .count();
        // A rounded 32×32 tile with r=7 covers a shade over 95% of its box.
        assert!(
            opaque > (size * size) as usize * 9 / 10,
            "only {opaque} opaque of {}",
            size * size
        );
    }
}
