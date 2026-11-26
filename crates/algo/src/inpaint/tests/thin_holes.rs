//! Tests for thin holes and thread-count independence.

use super::*;

#[test]
fn a_hole_is_as_thick_as_it_is_through_not_along() {
    let (w, h) = (240, 120);
    let all = Bounds {
        x0: 0,
        y0: 0,
        x1: w,
        y1: h,
    };
    let thickness = |hole: &[bool]| {
        let px: Vec<Px> = hole
            .iter()
            .map(|&o| if o { Px::Hole } else { Px::Source })
            .collect();
        hole_thickness(w, hole_bounds(w, h, &px).unwrap_or(all), &px)
    };
    // A band 200 px long and 9 px thick; a disc of radius 20; the band touching the edge
    // counts the outside of the image as not hole.
    let band: Vec<bool> = (0..w * h)
        .map(|i| (20..220).contains(&(i % w)) && (50..59).contains(&(i / w)))
        .collect();
    assert!((9..=11).contains(&thickness(&band)), "{}", thickness(&band));
    assert!((39..=42).contains(&thickness(&disc(w, h, 120.0, 60.0, 20.0))));
    let edge: Vec<bool> = (0..w * h).map(|i| i / w < 9).collect();
    assert!((9..=11).contains(&thickness(&edge)), "{}", thickness(&edge));
}

/// A long thin hole goes the thin way (a shallow pyramid that starts from a membrane): it is
/// filled, only it changes, and the result does not depend on the thread count either.
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn thin_holes_are_filled_the_same_on_any_number_of_threads() {
    let (w, h) = (300, 200);
    let img = busy(w, h, 3, 5);
    // A wavy line 6 px thick across the image.
    let hole: Vec<bool> = (0..w * h)
        .map(|i| (((i % w) as f32 * 0.05).sin() * 30.0 + 100.0 - (i / w) as f32).abs() < 3.0)
        .collect();
    let px = roles(w, h, &hole);
    let run = |threads: usize| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        pool.install(|| {
            complete_masked(
                w,
                h,
                3,
                &img,
                &px,
                &CompleteParams::default(),
                &rust_motion_raster::Interrupt::NONE,
            )
        })
        .unwrap()
        .unwrap()
    };
    let one = run(1);
    assert_eq!(bits(&one), bits(&run(7)));
    assert!((0..w * h)
        .all(|i| hole[i] || (0..3).all(|c| one[i * 3 + c].to_bits() == img[i * 3 + c].to_bits())));
    assert!((0..w * h)
        .filter(|&i| hole[i])
        .all(|i| (0..3).all(|c| one[i * 3 + c].is_finite())));
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn results_do_not_depend_on_the_thread_count() {
    // Big enough for the parallel passes, and for PatchMatch to sweep in wavefronts where
    // there are fewer bands than threads (never with one thread).
    let (w, h) = (300, 260);
    for (ch, adapt) in [
        (3, Adapt::NONE),
        (
            4,
            Adapt {
                gain: (0.9, 1.1),
                bias: 0.04,
            },
        ),
    ] {
        let img = busy(w, h, ch, 3);
        let hole = disc(w, h, 140.0, 130.0, 40.0);
        let px = roles(w, h, &hole);
        let p = CompleteParams {
            adapt,
            ..Default::default()
        };
        let run = |threads: usize| {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap();
            bits(
                &pool
                    .install(|| {
                        complete_masked(
                            w,
                            h,
                            ch,
                            &img,
                            &px,
                            &p,
                            &rust_motion_raster::Interrupt::NONE,
                        )
                    })
                    .unwrap()
                    .unwrap(),
            )
        };
        let one = run(1);
        assert_eq!(one, run(3), "{ch} channels, 3 threads");
        assert_eq!(one, run(16), "{ch} channels, 16 threads");
    }
}
