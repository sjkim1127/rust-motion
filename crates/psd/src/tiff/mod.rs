//! Photoshop layer data embedded in TIFF files (layered TIFF).
//!
//! Photoshop keeps a layered TIFF's layers in two private tags, described by Adobe's
//! *Photoshop TIFF Technical Notes* (March 2002) together with the PSD specification:
//!
//! * **37724 `ImageSourceData`**: the string `"Adobe Photoshop Document Data Block\0"`
//!   (`"Adobe Photoshop Document Data V0002\0"` for PSB-sized data, whose long keys carry
//!   8-byte lengths) followed by tagged blocks in the usual `8BIM` + key + length + data form,
//!   each padded to a multiple of 4. `Layr` (8-bit), `Lr16` and `Lr32` hold the layer info, the
//!   same structure as a PSD's layer info section; `LMsk` holds the global layer mask; every other
//!   block is a global additional-layer-information block (`Patt`, `Anno`, `FMsk`, `Txt2`, …).
//! * **34377**: the PSD image resources (`8BIM` resource blocks, without the section length).
//!
//! **Byte order.** Adobe's note is silent, but Photoshop files show (and open-source readers such as psdtags
//! implement) that the block structure follows the TIFF's byte order: in an Intel-order (`II`)
//! file every integer and double is little-endian and every four-character signature and key is
//! stored reversed (`MIB8`, `ryaL`), while the pixel samples inside channel data stay big-endian
//! as in a PSD, and RLE row-count tables follow the container. [`ImageSourceData::from_bytes`]
//! normalizes either order to the big-endian PSD model, and [`ImageSourceData::to_bytes`] writes
//! either order, by transcoding every structure it knows field by field ([`transcode`]).
//! A block the transcoder does not know is never dropped on the way in: its bytes are kept
//! verbatim with the signature `MIB8` (`46B8` for `8B64`) marking that they are in little-endian
//! layout ([`TaggedBlock::is_foreign_order`]), so a later save to a little-endian TIFF writes it
//! back exactly. Only a save to the *other* byte order has to drop such a block, with a warning:
//! a byte-swapped block passed through unchanged would be read as garbage by every other
//! application (and would end the block list for readers that stop at a bad signature).

use crate::descriptor::MAX_DEPTH;
use crate::error::{PsdError, Result};
use crate::file::{GlobalLayerMask, LayerInfoPlacement, PsdFile};
use crate::header::{Header, Version};
use crate::image_data::ImageData;
use crate::io::{Reader, WriteExt};
use crate::layer::{mask_parameter_bytes, LayerInfo, LayerMask};
use crate::resources::ImageResource;
use crate::tagged::{read_blocks, uses_long_length, TaggedBlock};

mod image_source;
mod resource_tag;
#[cfg(test)]
mod tests;
pub mod transcode;
mod types;

use image_source::*;
pub use resource_tag::*;
pub use types::*;
