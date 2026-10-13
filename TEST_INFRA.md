# E2E Test Infra: Rust Motion Remotion Native Porting

## Test Philosophy
- Opaque-box, requirement-driven. Zero dependency on `vendor/` or implementation internals.
- Methodology: Category-Partition + Boundary Value Analysis (BVA) + Pairwise Interaction Testing + Real-World Workload Testing.

## Feature Inventory Test Matrix
| # | Feature | Source (Requirement) | Tier 1 (Coverage ≥5) | Tier 2 (Boundary ≥5) | Tier 3 (Pairwise) | Tier 4 (Real-World) |
|---|---------|----------------------|:-------------------:|:--------------------:|:-----------------:|:-------------------:|
| 1 | Simplex 2D/3D/4D | ORIGINAL_REQUEST §R1 | 5 | 5 | ✓ | ✓ |
| 2 | Mulberry32 PRNG & Seeding | ORIGINAL_REQUEST §R1 | 5 | 5 | ✓ | ✓ |
| 3 | Fractal Brownian Motion (fBm) | ORIGINAL_REQUEST §R1 | 5 | 5 | ✓ | ✓ |
| 4 | Turbulence & Domain Warping | ORIGINAL_REQUEST §R1 | 5 | 5 | ✓ | ✓ |
| 5 | `<NoiseBackground />` | ORIGINAL_REQUEST §R1 | 5 | 5 | ✓ | ✓ |
| 6 | Chromatic Aberration Filter | ORIGINAL_REQUEST §R2 | 5 | 5 | ✓ | ✓ |
| 7 | Vignette Filter | ORIGINAL_REQUEST §R2 | 5 | 5 | ✓ | ✓ |
| 8 | Color Grading Suite | ORIGINAL_REQUEST §R2 | 5 | 5 | ✓ | ✓ |
| 9 | ClockWipe Transition | ORIGINAL_REQUEST §R2 | 5 | 5 | ✓ | ✓ |
| 10 | LinearWipe / Polygon Transitions | ORIGINAL_REQUEST §R2 | 5 | 5 | ✓ | ✓ |
| 11 | Flip & Zoom Transitions | ORIGINAL_REQUEST §R2 | 5 | 5 | ✓ | ✓ |
| 12 | Easing Curves & Timing | ORIGINAL_REQUEST §R2 | 5 | 5 | ✓ | ✓ |
| 13 | SceneTransitionSeries Integration | ORIGINAL_REQUEST §R2 | 5 | 5 | ✓ | ✓ |
| 14 | `fit_text_on_n_lines` | ORIGINAL_REQUEST §R3 | 5 | 5 | ✓ | ✓ |
| 15 | `fill_text_box` | ORIGINAL_REQUEST §R3 | 5 | 5 | ✓ | ✓ |
| 16 | `create_rounded_text_box` | ORIGINAL_REQUEST §R3 | 5 | 5 | ✓ | ✓ |
| 17 | Unified Public APIs | ORIGINAL_REQUEST §R3 | 5 | 5 | ✓ | ✓ |

## Test Architecture
- Test Runner: `cargo test --locked --workspace --all-features` (and targeted test modules).
- Target Test Directories:
  - Unit / Mathematical parity tests: `crates/noise/tests/`, `crates/rasterizer/tests/`, `crates/transitions/tests/`
  - Integration & E2E tests: `tests/e2e/` (or integration test files in root/crates)
- Pass/Fail Semantics:
  - Exit code 0
  - Zero panics, zero unexpected NaN / Inf, exact deterministic reproduction on seeds, zero memory leaks.

## Real-World Application Scenarios (Tier 4)
| # | Scenario | Features Exercised | Complexity |
|---|----------|--------------------|------------|
| 1 | Procedural Cyberpunk Title Card | Simplex noise background + Vignette + Chromatic Aberration + Multi-line fitted text | High |
| 2 | Dynamic Presentation Deck | ClockWipe & LinearWipe transitions + Easing + Multi-corner rounded text badge badges | High |
| 3 | Cinematic Color Grading Video Reel | Contrast + Saturation + Tint + 3D Flip transition + SceneTransitionSeries overlap | High |
| 4 | Organic Flow Graphic Animation | 4D Simplex noise + Domain Warping + Procedural SVG path deformation | High |
| 5 | Streaming Word-by-Word Caption Box | `fill_text_box` + `create_rounded_text_box` + dynamic padding & corner radius | High |

## Coverage Thresholds
- Tier 1: ≥5 per feature (Total ≥85 test cases)
- Tier 2: ≥5 per feature boundary/corner (Total ≥85 test cases)
- Tier 3: Pairwise feature combination tests (Total ≥20 test cases)
- Tier 4: ≥5 realistic end-to-end video application scenarios
- Tier 5: Adversarial white-box stress & edge-case hardening
