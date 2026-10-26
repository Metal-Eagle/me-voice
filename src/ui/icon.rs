//! The app icon (rounded dark tile with a blue, a yellow-to-orange and a white
//! bar, plus a coloured status dot), drawn in code so there are no image files
//! to ship. Mirrors `assets/icon.svg`.

const TILE_COLOUR: [u8; 3] = [0x14, 0x16, 0x1F];
const BLUE: [u8; 3] = [0x1E, 0x9B, 0xFF];
/// Middle bar gradient, top to bottom.
const MIDDLE_STOPS: [[u8; 3]; 3] = [[0xFF, 0xF8, 0xA6], [0xFF, 0xD0, 0x00], [0xFF, 0x8A, 0x00]];
const WHITE: [u8; 3] = [0xFF, 0xFF, 0xFF];
pub const ORANGE: [u8; 3] = [0xFF, 0x80, 0x00];

/// Rounded tile in the 256×256 design space: (x, y, size, corner radius).
const TILE: (f32, f32, f32, f32) = (8.0, 8.0, 240.0, 60.0);
/// Bars in the 256×256 design space: (x, y, width, height). Fully rounded ends.
const BARS: [(f32, f32, f32, f32); 3] = [
    (38.0, 96.0, 36.0, 66.0),
    (88.0, 48.0, 36.0, 162.0),
    (138.0, 76.0, 36.0, 106.0),
];
const DOT: (f32, f32, f32) = (206.0, 160.0, 22.0);

/// Render the icon as RGBA at `size`×`size`, with the dot in `dot` colour.
/// Outside the rounded tile is transparent.
pub fn rgba(size: u32, dot: [u8; 3]) -> Vec<u8> {
    const SS: u32 = 4; // supersamples per axis, for anti-aliasing
    let scale = 256.0 / (size * SS) as f32;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let mut sum = [0u32; 3];
            let mut hits = 0u32;
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = ((px * SS + sx) as f32 + 0.5) * scale;
                    let y = ((py * SS + sy) as f32 + 0.5) * scale;
                    if let Some(c) = colour_at(x, y, dot) {
                        for i in 0..3 {
                            sum[i] += c[i] as u32;
                        }
                        hits += 1;
                    }
                }
            }
            if hits == 0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                let alpha = hits * 255 / (SS * SS);
                out.extend_from_slice(&[(sum[0] / hits) as u8, (sum[1] / hits) as u8, (sum[2] / hits) as u8, alpha as u8]);
            }
        }
    }
    out
}

fn colour_at(x: f32, y: f32, dot: [u8; 3]) -> Option<[u8; 3]> {
    let (tx, ty, ts, tr) = TILE;
    if !in_rounded_rect(x, y, tx, ty, ts, ts, tr) {
        return None;
    }
    let (cx, cy, r) = DOT;
    if (x - cx).powi(2) + (y - cy).powi(2) <= r * r {
        return Some(dot);
    }
    for (i, &(bx, by, w, h)) in BARS.iter().enumerate() {
        if in_rounded_rect(x, y, bx, by, w, h, w / 2.0) {
            return Some(match i {
                0 => BLUE,
                1 => gradient(&MIDDLE_STOPS, (y - by) / h),
                _ => WHITE,
            });
        }
    }
    Some(TILE_COLOUR)
}

fn in_rounded_rect(x: f32, y: f32, rx: f32, ry: f32, w: f32, h: f32, r: f32) -> bool {
    if x < rx || x >= rx + w || y < ry || y >= ry + h {
        return false;
    }
    let cx = x.clamp(rx + r, rx + w - r);
    let cy = y.clamp(ry + r, ry + h - r);
    (x - cx).powi(2) + (y - cy).powi(2) <= r * r
}

/// Colour at `t` (0..=1) along evenly spaced gradient stops.
fn gradient(stops: &[[u8; 3]], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
    let i = (t as usize).min(stops.len() - 2);
    let f = t - i as f32;
    let (a, b) = (stops[i], stops[i + 1]);
    [0, 1, 2].map(|k| (a[k] as f32 + (b[k] as f32 - a[k] as f32) * f).round() as u8)
}

/// Writes the app icon as a Windows `.ico` (16–256 px, 32-bit), used by the installer.
pub fn write_ico(path: &std::path::Path) -> std::io::Result<()> {
    const SIZES: [u32; 5] = [16, 32, 48, 64, 256];
    let images: Vec<Vec<u8>> = SIZES.iter().map(|&s| bmp_entry(s)).collect();

    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]); // reserved, type = icon
    out.extend_from_slice(&(SIZES.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * SIZES.len() as u32;
    for (&size, image) in SIZES.iter().zip(&images) {
        let dim = if size >= 256 { 0 } else { size as u8 }; // 0 means 256
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // colour planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(image.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += image.len() as u32;
    }
    for image in images {
        out.extend_from_slice(&image);
    }
    std::fs::write(path, out)
}

/// One icon image as a bottom-up BGRA bitmap with an (empty) AND mask.
fn bmp_entry(size: u32) -> Vec<u8> {
    let rgba = rgba(size, ORANGE);
    let mask_row = ((size + 31) / 32) * 4;
    let mut out = Vec::new();
    out.extend_from_slice(&40u32.to_le_bytes()); // header size
    out.extend_from_slice(&(size as i32).to_le_bytes());
    out.extend_from_slice(&((size * 2) as i32).to_le_bytes()); // height incl. mask
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 24]); // no compression, sizes/colours unused
    for y in (0..size).rev() {
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            out.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
        }
    }
    out.extend(std::iter::repeat(0u8).take((mask_row * size) as usize));
    out
}
