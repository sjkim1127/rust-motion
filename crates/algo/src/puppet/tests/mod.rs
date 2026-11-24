//! Tests for Puppet Warp.

use super::*;
use rust_motion_color::{ColorMode, PixelFormat, SampleType};

mod mesh;
mod warp;

pub(super) fn blob(st: SampleType) -> Surface {
    let mut s = Surface::new(PixelFormat::new(ColorMode::Rgb, st, true));
    // An L shape: a vertical bar and a foot.
    for y in 10..70 {
        for x in 20..36 {
            s.write_pixel(
                x,
                y,
                &[((x + y) % 7) as f32 / 6.0, 0.5, (y as f32) / 80.0, 1.0],
            );
        }
    }
    for y in 56..70 {
        for x in 36..70 {
            s.write_pixel(x, y, &[0.2, ((x * 3) % 5) as f32 / 4.0, 0.8, 1.0]);
        }
    }
    s
}
