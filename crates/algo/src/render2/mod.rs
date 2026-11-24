//! Procedural renderers of Filter › Render: Flame, Picture Frame and Tree.
//!
//! Each renderer turns its options into a list of anti-aliased vector
//! primitives ([`Prim`]: tapered capsules and shaded ellipses, blended
//! normally or additively). [`composite`] rasterizes the list over a surface
//! tile by tile (in parallel on native targets), in straight sRGB converted to
//! and from the surface's own colour model and depth, mixed by the selection.
//!
//! The shapes are our own designs (L-system-like recursive branching for
//! trees, turbulent flame strands, ornaments walked along the frame edge),
//! built from observation of what the options do, not from any reference
//! implementation.

use rust_motion_geom::Rect;
use rust_motion_raster::{from_rgba_into, to_rgba, Surface};
use serde::{Deserialize, Serialize};

mod flame;
mod frame;
mod noise;
mod raster;
mod tree;
mod tree_kinds;
mod tree_table;
mod types;

pub use flame::*;
pub use frame::*;
pub use noise::*;
pub use raster::*;
pub use tree::*;
use tree_kinds::*;
pub use tree_table::*;
pub use types::*;

#[cfg(test)]
mod tests {
    use super::*;
    use rust_motion_color::{ColorMode, PixelFormat, SampleType};

    #[test]
    fn composite_draws_a_disc_and_respects_the_clip() {
        let mut s = Surface::new(PixelFormat::new(ColorMode::Rgb, SampleType::U8, true));
        let p = [Prim::disc((20.0, 20.0), 8.0, [1.0, 0.0, 0.0, 1.0])];
        let used = composite(&mut s, &p, Rect::new(0, 0, 20, 40), None);
        assert_eq!(used.x1, 20);
        let c = s.pixel(18, 20);
        assert!(c[0] > 0.99 && c[3] > 0.99);
        assert_eq!(s.pixel(22, 20)[3], 0.0, "clipped");
        assert_eq!(s.pixel(2, 2)[3], 0.0);
    }

    #[test]
    fn renderers_are_deterministic_and_seeded() {
        let canvas = Rect::new(0, 0, 400, 300);
        let a = tree(&TreeSpec::default(), canvas);
        assert_eq!(a, tree(&TreeSpec::default(), canvas));
        assert_ne!(
            a,
            tree(
                &TreeSpec {
                    seed: 7,
                    ..Default::default()
                },
                canvas
            )
        );
        for k in 1..=34 {
            assert!(
                !tree(
                    &TreeSpec {
                        base_tree_type: k,
                        ..Default::default()
                    },
                    canvas
                )
                .is_empty(),
                "tree {k}"
            );
        }
        for k in 1..=FRAME_STYLES.len() as u32 {
            assert!(
                !picture_frame(
                    &FrameSpec {
                        frame: k,
                        ..Default::default()
                    },
                    canvas
                )
                .is_empty(),
                "frame {k}"
            );
        }
        let path = default_flame_path(canvas);
        for t in [
            FlameType::OneFlameAlongPath,
            FlameType::MultipleFlamesAlongPath,
            FlameType::MultipleFlamesPathDirections,
            FlameType::MultipleFlamesVariousLength,
            FlameType::CandleLight,
            FlameType::MultipleFlamesOneDirection,
        ] {
            assert!(
                !flame(
                    &FlameSpec {
                        flame_type: t,
                        ..Default::default()
                    },
                    std::slice::from_ref(&path)
                )
                .is_empty(),
                "{t:?}"
            );
        }
    }
}
