//! Reading and writing the 34377 image-resources tag.

use super::*;

/// Parses a 34377 tag (image resources). Little-endian resource blocks (`MIB8`) are converted;
/// resource payloads other than the typed numeric ones are kept as stored.
pub fn resources_from_bytes(data: &[u8]) -> Result<(Vec<ImageResource>, Vec<String>)> {
    let order = block_order(data)?;
    let mut warnings = Vec::new();
    let big;
    let data = if order.is_little() {
        let (b, w) = transcode::transcode_resources(data, ByteOrder::Little, ByteOrder::Big)?;
        warnings = w;
        big = b;
        &big[..]
    } else {
        data
    };
    let mut r = Reader::new(data);
    let mut out = Vec::new();
    while !r.is_empty() {
        // Photoshop pads the tag to an even length; a lone trailing byte is not a resource.
        if r.remaining() < 12 && r.peek_rest().iter().all(|&b| b == 0) {
            break;
        }
        out.push(ImageResource::read(&mut r)?);
    }
    Ok((out, warnings))
}

/// Serializes image resources for a 34377 tag (big-endian, which every reader expects).
pub fn resources_to_bytes(resources: &[ImageResource]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for r in resources {
        r.write(&mut out)?;
    }
    Ok(out)
}
