//! Field-by-field byte-order conversion of Photoshop structures.
//!
//! Every function reads one structure in the source order and writes it in the target order,
//! so output lengths equal input lengths and every length field stays valid. Layouts follow the
//! Adobe PSD specification; the walker only needs to know where the integers, doubles and

use super::*;

mod adjustments;
mod block_bodies;
mod blocks;
mod descriptors;
mod layers;
mod resources;
mod walker;

use walker::Tx;

/// Converts the tagged blocks after the 37724 header string from `from` to `to`.
pub fn transcode_blocks(
    data: &[u8],
    version: Version,
    from: ByteOrder,
    to: ByteOrder,
) -> Result<(Vec<u8>, Vec<String>)> {
    let mut t = Tx::new(data, version, from, to);
    t.blocks()?;
    Ok(t.finish())
}

/// Converts a layer-info body (the data of a `Layr` / `Lr16` / `Lr32` block) between orders.
pub fn transcode_layer_info(
    data: &[u8],
    version: Version,
    from: ByteOrder,
    to: ByteOrder,
) -> Result<(Vec<u8>, Vec<String>)> {
    let mut t = Tx::new(data, version, from, to);
    t.layer_info()?;
    Ok(t.finish())
}

/// Converts image-resource blocks (a 34377 tag) between orders.
pub fn transcode_resources(
    data: &[u8],
    from: ByteOrder,
    to: ByteOrder,
) -> Result<(Vec<u8>, Vec<String>)> {
    let mut t = Tx::new(data, Version::Psd, from, to);
    t.resources()?;
    Ok(t.finish())
}

/// A block signature as stored in `little` or big-endian data. Big-endian data (the model)
/// also carries the `MIB8` / `46B8` markers of blocks kept from a little-endian file.
fn is_sig(b: &[u8], little: bool) -> bool {
    if little {
        b.starts_with(b"MIB8") || b.starts_with(b"46B8")
    } else {
        b.starts_with(b"8BIM")
            || b.starts_with(b"8B64")
            || TaggedBlock::is_foreign_order_signature(b)
    }
}

fn height(top: i32, bottom: i32) -> usize {
    usize::try_from(bottom.saturating_sub(top)).unwrap_or(0)
}

/// Per layer record: the channel (id, length) list and the heights of the layer, mask and
/// real-mask rects.
type RecordShape = (Vec<(i16, u64)>, [usize; 3]);
