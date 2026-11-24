//! Pixel work of the Filter Gallery ([`crate::artistic`]).
//!
//! Every effect runs on an sRGB working copy of the tile's input window (straight colour; the
//! source alpha is kept) and may read up to [`effect_reach`] pixels around each output pixel,
//! so a stack of effects reads the sum of their reaches. Noise, strokes and textures are
//! functions of document coordinates, so results do not depend on the tiling.

// Effects walk several per-pixel planes in lockstep; index loops read clearest.
#![allow(clippy::needless_range_loop)]

mod apply;
mod extent;
mod filters;
mod surface;
mod win;

pub(crate) use apply::*;
pub(crate) use extent::*;
use filters::*;
use surface::*;
pub(crate) use win::*;

#[cfg(test)]
mod tests {
    use rust_motion_color::{ColorMode, PixelFormat, SampleType};
    use rust_motion_geom::Rect;
    use rust_motion_raster::Surface;

    use crate::artistic::GalleryEffect;
    use crate::{apply_tiled, FilterParams};

    fn pattern(st: SampleType) -> Surface {
        let mut s = Surface::new(PixelFormat::new(ColorMode::Rgb, st, true));
        for y in 0..40 {
            for x in 0..56 {
                let v = ((x * 7 + y * 3) % 23) as f32 / 22.0;
                let px = if (x / 9 + y / 7) % 2 == 0 {
                    [v, 0.3, 1.0 - v, 1.0]
                } else {
                    [0.9, v * 0.5, 0.2, 1.0]
                };
                s.write_pixel(x, y, &px);
            }
        }
        s
    }

    #[test]
    fn every_effect_changes_pixels_and_is_tile_independent() {
        let b = Rect::new(0, 0, 56, 40);
        for st in [SampleType::U8, SampleType::U16, SampleType::F32] {
            let s = pattern(st);
            let before = s.read_region(b);
            for f in crate::GalleryFilter::ALL {
                let fp = FilterParams::FilterGallery {
                    effects: vec![GalleryEffect::new(*f)],
                };
                let a = apply_tiled(&s, &fp, b, b, None, 256, Some(b)).read_region(b);
                assert_ne!(a, before, "{} @{st:?} changed nothing", f.key());
                assert!(a.iter().all(|v| v.is_finite()), "{}", f.key());
                if st == SampleType::F32 {
                    let t = apply_tiled(&s, &fp, b, b, None, 16, Some(b)).read_region(b);
                    let worst = a
                        .iter()
                        .zip(&t)
                        .map(|(x, y)| (x - y).abs())
                        .fold(0.0, f32::max);
                    assert!(
                        worst < 1e-3,
                        "{} differs across tilings by {worst}",
                        f.key()
                    );
                }
            }
        }
    }

    #[test]
    fn rough_pastels_and_angled_strokes_respond_to_every_slider() {
        // #879: these smeared or drowned the image in texture, and some sliders did nothing.
        use crate::GalleryFilter as G;
        let b = Rect::new(0, 0, 56, 40);
        let s = pattern(SampleType::F32);
        let run = |e: &GalleryEffect, tile: i32| {
            let fp = FilterParams::FilterGallery {
                effects: vec![e.clone()],
            };
            apply_tiled(&s, &fp, b, b, None, tile, Some(b)).read_region(b)
        };
        for f in [G::RoughPastels, G::AngledStrokes] {
            let base = run(&GalleryEffect::new(f), 256);
            for prm in f.params() {
                let crate::GalleryParamKind::Range { min, max, .. } = prm.kind else {
                    continue;
                };
                for v in [min, max] {
                    let mut e = GalleryEffect::new(f);
                    e.set(prm.key, v);
                    let a = run(&e, 256);
                    assert_ne!(a, base, "{} {} = {v} changed nothing", f.key(), prm.key);
                    let t = run(&e, 16);
                    let worst = a
                        .iter()
                        .zip(&t)
                        .map(|(x, y)| (x - y).abs())
                        .fold(0.0, f32::max);
                    assert!(
                        worst < 1e-3,
                        "{} {} = {v} differs across tilings by {worst}",
                        f.key(),
                        prm.key
                    );
                }
            }
        }
    }

    #[test]
    fn params_parse_and_clamp() {
        for f in crate::GalleryFilter::ALL {
            assert!(!f.params().is_empty(), "{}", f.key());
            assert_eq!(crate::GalleryFilter::from_key(f.key()), Some(*f));
            assert_eq!(crate::GalleryFilter::from_key(f.command_id()), Some(*f));
        }
        assert_eq!(crate::GalleryFilter::ALL.len(), 47);
        let mut e = GalleryEffect::new(crate::GalleryFilter::Texturizer);
        assert_eq!(e.choice("texture"), "canvas");
        assert_eq!(e.choice("light"), "top");
        assert!(e.set_choice("texture", "brick"));
        assert_eq!(e.choice("texture"), "brick");
        e.set("relief", 500.0);
        assert_eq!(e.get("relief"), 50.0);
    }

    /// Release timing on 24 MP: `cargo test -p rust-motion-algo --release bench_gallery -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn bench_gallery() {
        let (w, h) = (6000, 4000);
        let mut s = Surface::new(PixelFormat::new(ColorMode::Rgb, SampleType::U8, true));
        let b = Rect::new(0, 0, w, h);
        s.fill_rect(b, &[0.5, 0.4, 0.3, 1.0]);
        for y in (0..h).step_by(9) {
            s.fill_rect(Rect::new(0, y, w, y + 3), &[0.9, 0.8, 0.2, 1.0]);
        }
        let t = std::time::Instant::now();
        std::hint::black_box(crate::apply_in(
            &s,
            &FilterParams::AddNoise {
                amount: 10.0,
                distribution: crate::Distribution::Uniform,
                monochromatic: true,
                seed: 1,
            },
            b,
            b,
            None,
            b,
        ));
        println!(
            "{:>18}: {:>7.0} ms",
            "(addNoise baseline)",
            t.elapsed().as_secs_f64() * 1000.0
        );
        let mut total = 0.0;
        for f in crate::GalleryFilter::ALL {
            let fp = FilterParams::FilterGallery {
                effects: vec![GalleryEffect::new(*f)],
            };
            let t = std::time::Instant::now();
            std::hint::black_box(crate::apply_in(&s, &fp, b, b, None, b));
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            total += ms;
            println!("{:>18}: {ms:>7.0} ms", f.key());
        }
        println!("{:>18}: {total:>7.0} ms", "total");
    }

    /// Visual check: `GALLERY_RAW=src.raw GALLERY_W=480 GALLERY_H=322 cargo test -p rust-motion-algo --release gallery_sheet -- --ignored`
    /// writes `<key>.raw` (RGB8) beside the input for every filter.
    #[test]
    #[ignore]
    fn gallery_sheet() {
        let Ok(path) = std::env::var("GALLERY_RAW") else {
            return;
        };
        let w: i32 = std::env::var("GALLERY_W").unwrap().parse().unwrap();
        let h: i32 = std::env::var("GALLERY_H").unwrap().parse().unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mut s = Surface::new(PixelFormat::new(ColorMode::Rgb, SampleType::U8, true));
        let data: Vec<f32> = bytes
            .chunks(3)
            .flat_map(|c| {
                [
                    c[0] as f32 / 255.0,
                    c[1] as f32 / 255.0,
                    c[2] as f32 / 255.0,
                    1.0,
                ]
            })
            .collect();
        let b = Rect::new(0, 0, w, h);
        s.write_region(b, &data);
        let dir = std::path::Path::new(&path).parent().unwrap().to_path_buf();
        for f in crate::GalleryFilter::ALL {
            let fp = FilterParams::FilterGallery {
                effects: vec![GalleryEffect::new(*f)],
            };
            let out = crate::apply_in(&s, &fp, b, b, None, b).read_region(b);
            let raw: Vec<u8> = out
                .chunks(4)
                .flat_map(|p| [p[0], p[1], p[2]].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8))
                .collect();
            std::fs::write(dir.join(format!("{}.raw", f.key())), raw).unwrap();
        }
    }
}
