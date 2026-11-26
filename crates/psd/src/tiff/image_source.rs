//! Reading and writing the 37724 `ImageSourceData` tag.

use super::*;

pub(super) fn block_order(rest: &[u8]) -> Result<ByteOrder> {
    match rest.get(..4) {
        None | Some(b"8BIM") | Some(b"8B64") => Ok(ByteOrder::Big),
        Some(b"MIB8") | Some(b"46B8") => Ok(ByteOrder::Little),
        Some(other) => Err(PsdError::InvalidSignature {
            expected: "8BIM or MIB8 (Photoshop image source data block)",
            found: [other[0], other[1], other[2], other[3]],
        }),
    }
}

impl ImageSourceData {
    /// Parses a 37724 tag. Returns the data together with warnings about blocks that were
    /// dropped because they could not be converted from the file's byte order.
    pub fn from_bytes(data: &[u8]) -> Result<(Self, Vec<String>)> {
        let (version, rest) = match data.get(..36) {
            Some(s) if s == SIGNATURE => (Version::Psd, &data[36..]),
            Some(s) if s == SIGNATURE_PSB => (Version::Psb, &data[36..]),
            _ => return Err(PsdError::invalid(
                "not Photoshop image source data (missing \"Adobe Photoshop Document Data Block\")",
            )),
        };
        let byte_order = block_order(rest)?;
        let mut warnings = Vec::new();
        let big;
        let rest = if byte_order.is_little() {
            let (b, w) =
                transcode::transcode_blocks(rest, version, ByteOrder::Little, ByteOrder::Big)?;
            warnings = w;
            big = b;
            &big[..]
        } else {
            rest
        };
        let (mut blocks, trailing) = read_blocks(&mut Reader::new(rest), version)?;
        // Blocks in a TIFF are padded to 4 (`to_bytes` re-pads them), so zero padding is the
        // canonical default here, as even-length padding is in a PSD.
        for b in &mut blocks {
            if b.padding
                .as_ref()
                .is_some_and(|p| p.len() <= 3 && p.iter().all(|&x| x == 0))
            {
                b.padding = None;
            }
        }
        let mut layer_info = None;
        let mut layer_info_key = *b"Layr";
        if let Some(i) = blocks
            .iter()
            .position(|b| matches!(&b.key, b"Layr" | b"Lr16" | b"Lr32"))
        {
            let b = blocks.remove(i);
            layer_info = Some(LayerInfo::read_body(&b.data, version)?);
            layer_info_key = b.key;
        }
        let global_layer_mask =
            blocks
                .iter()
                .position(|b| &b.key == b"LMsk")
                .map(|i| GlobalLayerMask {
                    data: blocks.remove(i).data,
                });
        Ok((
            ImageSourceData {
                version,
                byte_order,
                layer_info,
                layer_info_key,
                global_layer_mask,
                global_blocks: blocks,
                trailing,
            },
            warnings,
        ))
    }

    /// Serializes the data in `order`. Returns the bytes and warnings about blocks that were
    /// dropped because they could not be converted to a little-endian layout.
    pub fn to_bytes(&self, order: ByteOrder) -> Result<(Vec<u8>, Vec<String>)> {
        let version = self.version;
        let mut body = Vec::new();
        if let Some(info) = &self.layer_info {
            // Pad to 4 without cloning the layers (and all their pixel data) just to set `padding`.
            let pad = info.pad_len(version, 4)?;
            let mut data = Vec::new();
            data.try_reserve(info.channel_data_len().saturating_add(pad))
                .map_err(|_| PsdError::LimitExceeded("not enough memory for the layer data"))?;
            info.write_unpadded(&mut data, version)?;
            data.resize(data.len() + pad, 0);
            padded_block(self.layer_info_key, data).write(&mut body, version, 4)?;
        }
        // Blocks without explicit padding are padded to 4 as they are written (alignment 4), so
        // the global mask and blocks are written from the model, without a copy of their data.
        // The signature stays (a foreign-order marker included).
        if let Some(m) = &self.global_layer_mask {
            TaggedBlock::write_parts(&mut body, *b"8BIM", *b"LMsk", &m.data, None, version, 4)?;
        }
        for b in &self.global_blocks {
            b.write(&mut body, version, 4)?;
        }
        body.put(&self.trailing);
        let (body, warnings) = if order.is_little() {
            transcode::transcode_blocks(&body, version, ByteOrder::Big, ByteOrder::Little)?
        } else {
            (body, Vec::new())
        };
        let mut out = Vec::with_capacity(36 + body.len());
        out.put(if version.is_psb() {
            SIGNATURE_PSB
        } else {
            SIGNATURE
        });
        out.put(&body);
        Ok((out, warnings))
    }

    /// The layers, global mask and global blocks of a PSD as image source data (byte order Big).
    pub fn from_psd(file: &PsdFile) -> Self {
        let key = match &file.layer_info_placement {
            LayerInfoPlacement::GlobalBlock { key, .. } => *key,
            LayerInfoPlacement::Section => layer_info_key(file.header.depth),
        };
        ImageSourceData {
            version: file.header.version,
            byte_order: ByteOrder::Big,
            layer_info: file.layer_info.clone(),
            layer_info_key: key,
            global_layer_mask: file.global_layer_mask.clone(),
            global_blocks: file.global_blocks.clone(),
            trailing: file.layer_mask_trailing.clone(),
        }
    }

    /// Assembles a PSD model from this data plus the parts a TIFF stores natively: the header
    /// (dimensions, depth, colour mode, channel count), the image resources (tag 34377) and the
    /// merged image (the TIFF's pixels).
    pub fn into_psd(
        self,
        header: Header,
        resources: Vec<ImageResource>,
        image_data: ImageData,
    ) -> PsdFile {
        let placement =
            if matches!(&self.layer_info_key, b"Lr16" | b"Lr32") && self.layer_info.is_some() {
                LayerInfoPlacement::GlobalBlock {
                    index: 0,
                    signature: *b"8BIM",
                    key: self.layer_info_key,
                    padding: None,
                }
            } else {
                LayerInfoPlacement::Section
            };
        PsdFile {
            header,
            color_mode_data: Vec::new(),
            resources,
            layer_info: self.layer_info,
            layer_info_placement: placement,
            global_layer_mask: self.global_layer_mask,
            global_blocks: self.global_blocks,
            layer_mask_trailing: self.trailing,
            image_data,
        }
    }
}

/// A block padded to a multiple of 4, as Photoshop writes them in TIFFs.
pub(super) fn padded_block(key: [u8; 4], data: Vec<u8>) -> TaggedBlock {
    let pad = (4 - data.len() % 4) % 4;
    TaggedBlock {
        signature: *b"8BIM",
        key,
        data,
        padding: Some(vec![0; pad]),
    }
}
