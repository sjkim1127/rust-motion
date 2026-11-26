//! Panorama stitching for File › Automate › Photomerge and Edit › Auto-Align / Auto-Blend Layers.
//!
//! The pipeline follows M. Brown, D. G. Lowe, *Automatic Panoramic Image Stitching using
//! Invariant Features*, IJCV 2007, with classical parts throughout:
//!
//! 1. **Features**: Harris corners + steered BRIEF ([`crate::features`]).
//! 2. **Pair matching**: ratio-test matches, RANSAC homography, and Brown–Lowe's probabilistic
//!    verification (`inliers > 8 + 0.3·matches`).
//! 3. **Focal length** (for cylindrical / spherical layouts): from the pairwise homographies of
//!    a rotating camera (H. Shum, R. Szeliski, *Construction of Panoramic Image Mosaics with
//!    Global and Local Alignment*, IJCV 2000: `H = K R K⁻¹` makes the columns and rows of
//!    `K⁻¹HK` orthonormal), or from EXIF.
//! 4. **Projection + motion model** per layout: Perspective (plane, homography), Cylindrical
//!    and Spherical (keypoints mapped onto the cylinder / sphere, rigid motion there), Collage
//!    (similarity), Reposition (translation). Auto picks perspective unless the result is too
//!    stretched, then cylindrical or spherical (Photoshop's rule of thumb).
//! 5. **Chained alignment + bundle adjustment**: a maximum spanning tree over the match graph
//!    from the reference image gives initial transforms; Levenberg–Marquardt then minimises
//!    the distance between all matched points in the panorama (optionally refining one radial
//!    distortion coefficient, Photoshop's *Geometric Distortion Correction*, and the focal
//!    length of the cylinder / sphere).
//! 6. **Photometric**: gain compensation (Brown–Lowe §6) jointly with a radial vignetting model
//!    `log V(r) = a·r² + b·r⁴` shared by all images (D. Goldman, *Vignette and Exposure
//!    Calibration and Compensation*, PAMI 2010), solved by linear least squares in log space.
//! 7. **Seams**: per new image, a minimum-cost path (dynamic programming, as in seam carving:
//!    S. Avidan, A. Shamir, SIGGRAPH 2007) through its overlap with the composite on the colour
//!    difference, so seams avoid places where the images disagree (moving objects, parallax).
//! 8. **Multi-band blending**: Laplacian pyramids (P. Burt, E. Adelson, *A Multiresolution
//!    Spline with Application to Image Mosaics*, ACM TOG 1983) of each image (holes filled by
//!    push-pull so edges don't darken) weighted by the Gaussian pyramid of its seam mask.
//!
//! Everything is deterministic. Registration works on luminance in `0..=1` at a reduced scale;
//! [`Placement::scaled`] maps the result back to full resolution.

#![allow(clippy::needless_range_loop)] // index loops mirror the maths (pixels × channels)

mod blending;
mod exposure;
mod matching;
mod model;
mod registration;
mod seams;
#[cfg(test)]
mod tests;

pub use blending::*;
pub use exposure::*;
pub use matching::*;
pub use model::*;
pub use registration::*;
pub use seams::*;
