//! Image fitting and sizing utilities.

/// How to fit the image within its container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImageFit {
    /// Scale to fill, preserving aspect ratio (may crop). Default.
    #[default]
    Cover,
    /// Scale to fit, preserving aspect ratio (may letterbox).
    Contain,
    /// Stretch to fill exactly.
    Fill,
    /// Natural size.
    None,
    /// Scale down if larger than container.
    ScaleDown,
}

impl ImageFit {
    pub fn as_css(&self) -> &'static str {
        match self {
            ImageFit::Cover => "cover",
            ImageFit::Contain => "contain",
            ImageFit::Fill => "fill",
            ImageFit::None => "none",
            ImageFit::ScaleDown => "scale-down",
        }
    }
}
