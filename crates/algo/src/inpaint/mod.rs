//! Hole filling for Content-Aware Fill and Spot Healing: patch-based completion, proximity match
//! and texture synthesis.
//!
//! * [`complete`], content-aware fill: the multi-scale "space-time completion" EM scheme of
//!   Y. Wexler, E. Shechtman, M. Irani, *Space-Time Completion of Video*, IEEE TPAMI 2007, with the
//!   nearest-neighbour field computed by C. Barnes, E. Shechtman, A. Finkelstein, D. B. Goldman,
//!   *PatchMatch: A Randomized Correspondence Algorithm for Structural Image Editing*, SIGGRAPH 2009
//!   (random init, propagation, exponentially shrinking random search). Each EM iteration finds, for
//!   every patch overlapping the hole, its most similar source patch, then re-estimates hole pixels
//!   by a similarity-weighted vote of the overlapping matches. Coarse to fine over an image pyramid
//!   (down to where the hole is about four patches across): the coarsest level starts by filling
//!   the hole from its edge inwards, ring by ring, each pixel from the patch that best matches what
//!   is known around it (rather than from a smooth membrane, which draws the fill towards flat
//!   areas); every finer level starts from the coarser level's matches scaled up and voted with the
//!   finer level's own pixels, so the texture stays sharp. Where neighbouring patches agree on their
//!   source (the usual case), the vote copies the source pixels exactly; only along the seams
//!   between copied pieces does it blend.
//!
//!   A thin hole (a selected wire, string or hair: long, but only a few patches thick) is
//!   measured by its thickness, not its bounds (#1366): the pyramid stops where it is still about
//!   two patches thick. That level is big, so the onion peel starts it only where its exhaustive
//!   search stays affordable; otherwise a membrane across the hole does, which over so short a
//!   distance is the surroundings themselves. Shrunk to where its bounds are four patches across,
//!   a long thin hole swallows its surroundings, the matches found there copy whatever happens to
//!   look like the blur (a dark blotch, or a piece of another string), and the finer levels,
//!   which only refine those matches, keep them: the removed object's outline stays visible.
//!
//!   This is what Photoshop's Content-Aware Fill does, measured as a black box on Photoshop 25.4
//!   (see `content_aware`): with colour adaptation off its fill is pieces of the image copied pixel
//!   for pixel, typically 15–40 px across, joined by seams a few pixels wide; with colour
//!   adaptation the copies come with a gain and bias. That is the patch transform of S. Darabi,
//!   E. Shechtman, C. Barnes, D. B. Goldman, P. Sen, *Image Melding: Combining Inconsistent Images
//!   using Patch-based Synthesis*, SIGGRAPH 2012: each source patch may be scaled and offset per
//!   channel to match its target, within limits ([`Adapt`]), and is voted in adjusted.
//! * [`best_offset`], proximity match: the single displacement whose surrounding ring best matches the
//!   ring around the hole (sum of squared differences), i.e. an automatic Healing Brush source.
//! * [`synthesize`], create texture: patch-based texture synthesis in the style of A. Efros,
//!   W. Freeman, *Image Quilting for Texture Synthesis and Transfer*, SIGGRAPH 2001 (blocks picked from
//!   random candidates by overlap error, joined along minimum-error boundary cuts, so the texture is
//!   copied, never averaged).
//!
//! All functions take interleaved `w × h × ch` normalised floats (any colour model/depth) and a
//! `w × h` hole mask, and return a full buffer equal to the input outside the hole. They are
//! deterministic (fixed-seed hash RNG, results independent of thread count).

mod fill;
mod grid;
mod onion;
mod patchmatch;
mod proximity;
mod proximity_integral;
mod solver;
mod sources;
#[cfg(test)]
mod tests;
mod texture;
mod types;
mod votes;

pub use fill::*;
use grid::*;
use onion::*;
pub use proximity::*;
pub(crate) use solver::*;
use sources::*;
pub use texture::*;
pub use types::*;
use votes::*;
