//! Degenerate 3D lookup tables must leave the colour alone.

use super::lut3d_sample;

#[test]
fn degenerate_or_short_tables_leave_the_colour_alone() {
    let c = [0.2, 0.4, 0.6];
    for tetra in [false, true] {
        assert_eq!(lut3d_sample(&[], 0, c, tetra), c);
        assert_eq!(lut3d_sample(&[0.0; 3], 1, c, tetra), c);
        assert_eq!(lut3d_sample(&[0.0; 3], 2, c, tetra), c);
        assert_eq!(lut3d_sample(&[0.0; 24], usize::MAX, c, tetra), c);
    }
}
