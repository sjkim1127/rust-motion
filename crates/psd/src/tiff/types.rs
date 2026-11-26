//! Byte order, header signatures and the [`ImageSourceData`] model.

use super::*;

/// The byte order of a TIFF container (and so of the Photoshop data inside it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ByteOrder {
    /// Motorola order (`MM`); the PSD byte order.
    #[default]
    Big,
    /// Intel order (`II`).
    Little,
}

impl ByteOrder {
    pub(super) fn is_little(self) -> bool {
        matches!(self, ByteOrder::Little)
    }
}

/// Header string of 37724 data with PSD-sized (4-byte) lengths.
pub const SIGNATURE: &[u8; 36] = b"Adobe Photoshop Document Data Block\0";
/// Header string of 37724 data with PSB-sized (8-byte) lengths for the long keys.
pub const SIGNATURE_PSB: &[u8; 36] = b"Adobe Photoshop Document Data V0002\0";

/// The layer-info block key Photoshop uses for a bit depth.
pub fn layer_info_key(depth: u16) -> [u8; 4] {
    match depth {
        16 => *b"Lr16",
        32 => *b"Lr32",
        _ => *b"Layr",
    }
}

/// The contents of an `ImageSourceData` tag, in the big-endian PSD model.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ImageSourceData {
    /// PSD (4-byte lengths) or PSB (8-byte lengths for the long keys).
    pub version: Version,
    /// The byte order the data was read in (what [`Self::to_bytes`] should normally write back).
    pub byte_order: ByteOrder,
    /// The layers, from the `Layr` / `Lr16` / `Lr32` block.
    pub layer_info: Option<LayerInfo>,
    /// Which key held the layer info (`Layr` when there was none).
    pub layer_info_key: [u8; 4],
    /// The `LMsk` block.
    pub global_layer_mask: Option<GlobalLayerMask>,
    /// Every other block, in stored order.
    pub global_blocks: Vec<TaggedBlock>,
    /// Bytes after the last block that do not form a block.
    pub trailing: Vec<u8>,
}

impl Default for ImageSourceData {
    fn default() -> Self {
        ImageSourceData {
            version: Version::Psd,
            byte_order: ByteOrder::Big,
            layer_info: None,
            layer_info_key: *b"Layr",
            global_layer_mask: None,
            global_blocks: Vec::new(),
            trailing: Vec::new(),
        }
    }
}
