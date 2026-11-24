//! Tests for the TIFF-embedded Photoshop data.

use super::transcode::*;
use super::*;
use crate::compression::Compression;
use crate::descriptor::{Descriptor, Id, UnicodeString, Value, VersionedDescriptor};
use crate::header::ColorMode;
use crate::testgen;

mod blocks;
mod byte_order;
mod container;
mod effects;

/// Keys the generator plants that the transcoder (rightly) does not know.
pub(super) const UNKNOWN: [&[u8; 4]; 3] = [b"zOdd", b"zNop", b"Txt2"];

/// `d` without the blocks a little-endian round trip drops, and without the layer-info
/// padding (which follows the body length, so it changes when blocks are dropped).
pub(super) fn known(d: &ImageSourceData) -> ImageSourceData {
    let mut d = d.clone();
    d.byte_order = ByteOrder::Big;
    d.global_blocks.retain(|b| !UNKNOWN.contains(&&b.key));
    if d.layer_info.is_none() {
        d.layer_info_key = *b"Layr";
    }
    if let Some(info) = &mut d.layer_info {
        info.padding = None;
        for l in &mut info.layers {
            l.blocks.retain(|b| !UNKNOWN.contains(&&b.key));
        }
    }
    d
}

pub(super) fn roundtrip(d: &ImageSourceData) -> ImageSourceData {
    let (big, w) = d.to_bytes(ByteOrder::Big).unwrap();
    assert!(w.is_empty(), "{w:?}");
    let (little, w) = d.to_bytes(ByteOrder::Little).unwrap();
    assert!(
        w.iter().all(|w| UNKNOWN
            .iter()
            .any(|k| w.starts_with(std::str::from_utf8(*k).unwrap()))),
        "{w:?}"
    );
    if w.is_empty() {
        assert_eq!(big.len(), little.len());
    }
    if big.len() > 36 {
        assert_ne!(big, little);
        assert!(little[36..].starts_with(b"MIB8"));
    }
    let (back_big, w) = ImageSourceData::from_bytes(&big).unwrap();
    assert!(w.is_empty(), "{w:?}");
    let (back_little, w) = ImageSourceData::from_bytes(&little).unwrap();
    assert!(w.is_empty(), "{w:?}");
    if little.len() > 36 {
        assert_eq!(back_little.byte_order, ByteOrder::Little);
    }
    assert_eq!(known(&back_little), known(&back_big));
    // The little-endian bytes are a fixed point (from the second generation on when blocks
    // were dropped, since the layer-info padding then follows the shorter body).
    let (little2, _) = back_little.to_bytes(ByteOrder::Little).unwrap();
    let little3 = ImageSourceData::from_bytes(&little2)
        .unwrap()
        .0
        .to_bytes(ByteOrder::Little)
        .unwrap()
        .0;
    assert_eq!(little3, little2);
    back_big
}

pub(super) fn sample_layers() -> ImageSourceData {
    let file = testgen::layered(Version::Psd, ColorMode::Rgb, 8, Compression::Rle);
    ImageSourceData::from_psd(&file)
}

pub(super) fn block_rt(key: [u8; 4], data: Vec<u8>) -> (Vec<u8>, Vec<u8>) {
    let d = ImageSourceData {
        global_blocks: vec![TaggedBlock::new(key, data.clone())],
        ..Default::default()
    };
    let (big, _) = d.to_bytes(ByteOrder::Big).unwrap();
    let (little, w) = d.to_bytes(ByteOrder::Little).unwrap();
    assert!(w.is_empty(), "{w:?}");
    let (back, w) = ImageSourceData::from_bytes(&little).unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(back.global_blocks[0].data, data);
    assert_eq!(back.to_bytes(ByteOrder::Big).unwrap().0, big);
    (big, little)
}
