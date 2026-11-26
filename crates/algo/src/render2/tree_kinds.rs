//! Tree species description types and the bark and leaf colours.

// ---------------------------------------------------------------------------
// Tree
// ---------------------------------------------------------------------------

/// Leaf form of a tree type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Leaf {
    None,
    Round,
    Oval,
    Needle,
    Heart,
    Blossom,
    Long,
    Frond,
}

/// Branch architecture of a tree type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Habit {
    /// Recursive branching (deciduous trees).
    Branching,
    /// A dominant stem with whorls of side branches (conifers).
    Conifer,
    /// A tall trunk crowned with fronds.
    Palm,
    /// Several straight jointed stalks.
    Bamboo,
}

/// One base tree type.
pub(super) struct TreeKind {
    pub(super) name: &'static str,
    pub(super) habit: Habit,
    pub(super) levels: u32,
    pub(super) children: u32,
    /// Branch spread (degrees from the parent).
    pub(super) spread: f32,
    /// Child / parent length ratio.
    pub(super) ratio: f32,
    /// Trunk share of the total height.
    pub(super) trunk: f32,
    /// Downward bend per unit length at deep levels (willow > 0, upright < 0).
    pub(super) droop: f32,
    /// Random bend.
    pub(super) wiggle: f32,
    /// Trunk radius as a fraction of the height.
    pub(super) thick: f32,
    pub(super) leaf: Leaf,
    pub(super) leaf_col: [f32; 3],
    pub(super) bark: [f32; 3],
    /// Fruit or blossom accent dots.
    pub(super) accent: Option<[f32; 3]>,
}

#[allow(clippy::too_many_arguments)]
pub(super) const fn tk(
    name: &'static str,
    habit: Habit,
    levels: u32,
    children: u32,
    spread: f32,
    ratio: f32,
    trunk: f32,
    droop: f32,
    wiggle: f32,
    thick: f32,
    leaf: Leaf,
    leaf_col: [f32; 3],
    bark: [f32; 3],
    accent: Option<[f32; 3]>,
) -> TreeKind {
    TreeKind {
        name,
        habit,
        levels,
        children,
        spread,
        ratio,
        trunk,
        droop,
        wiggle,
        thick,
        leaf,
        leaf_col,
        bark,
        accent,
    }
}

pub(super) const BROWN: [f32; 3] = [0.36, 0.25, 0.16];
pub(super) const GREY_BARK: [f32; 3] = [0.45, 0.42, 0.38];
pub(super) const GREEN: [f32; 3] = [0.24, 0.52, 0.16];
pub(super) const DARK_GREEN: [f32; 3] = [0.12, 0.36, 0.14];
