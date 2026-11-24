//! Warp styles: Photoshop's presets, their names and their menu order.

use super::*;

/// Warp style: none, a custom mesh, or one of Photoshop's presets (menu order).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WarpStyle {
    #[default]
    None,
    Custom,
    Arc,
    ArcLower,
    ArcUpper,
    Arch,
    Bulge,
    ShellLower,
    ShellUpper,
    Flag,
    Wave,
    Fish,
    Rise,
    Fisheye,
    Inflate,
    Squeeze,
    Twist,
}

impl WarpStyle {
    /// The fifteen presets in Photoshop's menu order.
    pub const PRESETS: [WarpStyle; 15] = [
        WarpStyle::Arc,
        WarpStyle::ArcLower,
        WarpStyle::ArcUpper,
        WarpStyle::Arch,
        WarpStyle::Bulge,
        WarpStyle::ShellLower,
        WarpStyle::ShellUpper,
        WarpStyle::Flag,
        WarpStyle::Wave,
        WarpStyle::Fish,
        WarpStyle::Rise,
        WarpStyle::Fisheye,
        WarpStyle::Inflate,
        WarpStyle::Squeeze,
        WarpStyle::Twist,
    ];

    /// Every style (none, custom, presets).
    pub fn all() -> impl Iterator<Item = WarpStyle> {
        [WarpStyle::None, WarpStyle::Custom]
            .into_iter()
            .chain(Self::PRESETS)
    }

    /// PSD `warpStyle` enum value (`warpArc`, `warpCustom`, `warpNone`…).
    pub fn psd_name(self) -> &'static str {
        match self {
            WarpStyle::None => "warpNone",
            WarpStyle::Custom => "warpCustom",
            WarpStyle::Arc => "warpArc",
            WarpStyle::ArcLower => "warpArcLower",
            WarpStyle::ArcUpper => "warpArcUpper",
            WarpStyle::Arch => "warpArch",
            WarpStyle::Bulge => "warpBulge",
            WarpStyle::ShellLower => "warpShellLower",
            WarpStyle::ShellUpper => "warpShellUpper",
            WarpStyle::Flag => "warpFlag",
            WarpStyle::Wave => "warpWave",
            WarpStyle::Fish => "warpFish",
            WarpStyle::Rise => "warpRise",
            WarpStyle::Fisheye => "warpFisheye",
            WarpStyle::Inflate => "warpInflate",
            WarpStyle::Squeeze => "warpSqueeze",
            WarpStyle::Twist => "warpTwist",
        }
    }

    /// Short id used by commands (`arc`, `arcLower`, `custom`, `none`).
    pub fn id(self) -> &'static str {
        let p = self.psd_name().trim_start_matches("warp");
        // Lower-case first letter without allocating: every id is listed.
        match p {
            "None" => "none",
            "Custom" => "custom",
            "Arc" => "arc",
            "ArcLower" => "arcLower",
            "ArcUpper" => "arcUpper",
            "Arch" => "arch",
            "Bulge" => "bulge",
            "ShellLower" => "shellLower",
            "ShellUpper" => "shellUpper",
            "Flag" => "flag",
            "Wave" => "wave",
            "Fish" => "fish",
            "Rise" => "rise",
            "Fisheye" => "fisheye",
            "Inflate" => "inflate",
            "Squeeze" => "squeeze",
            _ => "twist",
        }
    }

    /// Menu label (`Arc Lower`).
    pub fn label(self) -> &'static str {
        match self {
            WarpStyle::None => "None",
            WarpStyle::Custom => "Custom",
            WarpStyle::Arc => "Arc",
            WarpStyle::ArcLower => "Arc Lower",
            WarpStyle::ArcUpper => "Arc Upper",
            WarpStyle::Arch => "Arch",
            WarpStyle::Bulge => "Bulge",
            WarpStyle::ShellLower => "Shell Lower",
            WarpStyle::ShellUpper => "Shell Upper",
            WarpStyle::Flag => "Flag",
            WarpStyle::Wave => "Wave",
            WarpStyle::Fish => "Fish",
            WarpStyle::Rise => "Rise",
            WarpStyle::Fisheye => "Fisheye",
            WarpStyle::Inflate => "Inflate",
            WarpStyle::Squeeze => "Squeeze",
            WarpStyle::Twist => "Twist",
        }
    }

    /// Parses a short id, a PSD name or a label (case-insensitive, spaces ignored).
    pub fn parse(s: &str) -> Option<WarpStyle> {
        let k: String = s
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        Self::all()
            .find(|w| w.id().to_ascii_lowercase() == k || w.psd_name().to_ascii_lowercase() == k)
    }

    pub fn is_preset(self) -> bool {
        !matches!(self, WarpStyle::None | WarpStyle::Custom)
    }
}
