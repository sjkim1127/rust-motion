//! Layer label colours.

use serde::{Deserialize, Serialize};

/// Photoshop layer colour labels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LabelColor {
    #[default]
    None,
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Violet,
    Gray,
    Seafoam,
    Indigo,
    Magenta,
    Fuchsia,
}

impl LabelColor {
    /// Display order, independent of the PSD sheet-colour indices.
    pub const ALL: [Self; 12] = [
        Self::None,
        Self::Red,
        Self::Orange,
        Self::Yellow,
        Self::Green,
        Self::Seafoam,
        Self::Blue,
        Self::Indigo,
        Self::Magenta,
        Self::Fuchsia,
        Self::Violet,
        Self::Gray,
    ];

    /// Stable, language-independent command and inspection value.
    pub fn id(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Red => "red",
            Self::Orange => "orange",
            Self::Yellow => "yellow",
            Self::Green => "green",
            Self::Seafoam => "seafoam",
            Self::Blue => "blue",
            Self::Indigo => "indigo",
            Self::Magenta => "magenta",
            Self::Fuchsia => "fuchsia",
            Self::Violet => "violet",
            Self::Gray => "gray",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "No Color",
            Self::Red => "Red",
            Self::Orange => "Orange",
            Self::Yellow => "Yellow",
            Self::Green => "Green",
            Self::Seafoam => "Seafoam",
            Self::Blue => "Blue",
            Self::Indigo => "Indigo",
            Self::Magenta => "Magenta",
            Self::Fuchsia => "Fuchsia",
            Self::Violet => "Violet",
            Self::Gray => "Gray",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.id() == id)
    }
}

#[cfg(test)]
mod label_color_tests {
    use super::LabelColor;

    #[test]
    fn layer_color_ids_and_existing_serialized_names_stay_stable() {
        for (name, color) in [
            ("None", LabelColor::None),
            ("Red", LabelColor::Red),
            ("Orange", LabelColor::Orange),
            ("Yellow", LabelColor::Yellow),
            ("Green", LabelColor::Green),
            ("Blue", LabelColor::Blue),
            ("Violet", LabelColor::Violet),
            ("Gray", LabelColor::Gray),
        ] {
            assert_eq!(serde_json::to_value(color).unwrap(), name);
            assert_eq!(
                serde_json::from_value::<LabelColor>(serde_json::json!(name)).unwrap(),
                color
            );
        }
        for c in LabelColor::ALL {
            assert_eq!(LabelColor::from_id(c.id()), Some(c));
            assert_eq!(
                serde_json::from_value::<LabelColor>(serde_json::to_value(c).unwrap()).unwrap(),
                c
            );
        }
        for id in ["", "Red", "0", "unknown", "🔴"] {
            assert_eq!(LabelColor::from_id(id), None);
        }
        assert!(serde_json::from_value::<LabelColor>(serde_json::json!("Unknown")).is_err());
    }
}
