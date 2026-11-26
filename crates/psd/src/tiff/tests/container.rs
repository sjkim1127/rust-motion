//! Round trips through both byte orders, signatures, padding, resources and the PSD model.

use super::*;

#[test]
fn generated_files_survive_both_orders() {
    for case in testgen::all_cases() {
        let d = ImageSourceData::from_psd(&case.file);
        let back = roundtrip(&d);
        assert_eq!(back.version, d.version, "{}", case.name);
        assert_eq!(known(&back), known(&d), "{}", case.name);
        // Nothing but the planted unknown blocks is lost on the little-endian path.
        let (little, _) = d.to_bytes(ByteOrder::Little).unwrap();
        let (back, _) = ImageSourceData::from_bytes(&little).unwrap();
        assert_eq!(known(&back), known(&d), "{}", case.name);
    }
}

#[test]
fn same_order_transcoding_is_the_identity() {
    for case in testgen::all_cases() {
        let d = ImageSourceData::from_psd(&case.file);
        let (big, _) = d.to_bytes(ByteOrder::Big).unwrap();
        let (same, w) =
            transcode_blocks(&big[36..], d.version, ByteOrder::Big, ByteOrder::Big).unwrap();
        assert!(w.is_empty());
        assert_eq!(same, big[36..].to_vec(), "{}", case.name);
    }
}

#[test]
fn signature_selects_the_version() {
    let mut d = sample_layers();
    let (b, _) = d.to_bytes(ByteOrder::Big).unwrap();
    assert!(b.starts_with(SIGNATURE));
    d.version = Version::Psb;
    let (b, _) = d.to_bytes(ByteOrder::Big).unwrap();
    assert!(b.starts_with(SIGNATURE_PSB));
    assert_eq!(
        ImageSourceData::from_bytes(&b).unwrap().0.version,
        Version::Psb
    );
    roundtrip(&d);
}

#[test]
fn layer_key_follows_depth() {
    assert_eq!(layer_info_key(8), *b"Layr");
    assert_eq!(layer_info_key(16), *b"Lr16");
    assert_eq!(layer_info_key(32), *b"Lr32");
    for (depth, comp) in [
        (16, Compression::ZipPrediction),
        (32, Compression::ZipPrediction),
        (16, Compression::Rle),
    ] {
        let file = testgen::layered(Version::Psd, ColorMode::Rgb, depth, comp);
        let d = ImageSourceData::from_psd(&file);
        assert_eq!(d.layer_info_key, layer_info_key(depth));
        let back = roundtrip(&d);
        assert_eq!(back.layer_info_key, d.layer_info_key);
        // The 16/32-bit model goes back into a PSD as a global block, like Photoshop's.
        let psd = back.into_psd(
            file.header.clone(),
            file.resources.clone(),
            file.image_data.clone(),
        );
        assert!(matches!(
            psd.layer_info_placement,
            LayerInfoPlacement::GlobalBlock { .. }
        ));
        let again = PsdFile::from_bytes(&psd.to_bytes().unwrap()).unwrap();
        assert_eq!(
            known(&ImageSourceData::from_psd(&again)).layer_info,
            known(&d).layer_info
        );
        let (little, _) = d.to_bytes(ByteOrder::Little).unwrap();
        let (back, _) = ImageSourceData::from_bytes(&little).unwrap();
        assert_eq!(
            known(&back).layer_info,
            known(&d).layer_info,
            "depth {depth}"
        );
    }
}

#[test]
fn blocks_are_padded_to_four() {
    let d = sample_layers();
    let (b, _) = d.to_bytes(ByteOrder::Big).unwrap();
    let mut at = 36;
    while at < b.len() {
        assert_eq!(&b[at..at + 4], b"8BIM");
        let len = u32::from_be_bytes(b[at + 8..at + 12].try_into().unwrap()) as usize;
        at += 12 + len;
        at += (4 - at % 4) % 4;
    }
    assert_eq!(at, b.len());
}

#[test]
fn psb_sized_data_uses_long_lengths_and_counts() {
    let file = testgen::layered(Version::Psb, ColorMode::Cmyk, 8, Compression::Rle);
    let d = ImageSourceData::from_psd(&file);
    assert_eq!(d.version, Version::Psb);
    let back = roundtrip(&d);
    assert_eq!(back.layer_info, d.layer_info);
    let (little, _) = d.to_bytes(ByteOrder::Little).unwrap();
    assert!(little[36..].starts_with(b"MIB8"));
    let (back, _) = ImageSourceData::from_bytes(&little).unwrap();
    assert_eq!(known(&back).layer_info, known(&d).layer_info);
}

#[test]
fn resources_both_orders() {
    let res = vec![
        ImageResource::new(1005, vec![0, 72, 0, 0, 0, 1, 0, 1, 0, 72, 0, 0, 0, 1, 0, 1]),
        ImageResource::new(1037, vec![0, 0, 0, 120]),
        ImageResource::new(1039, vec![1, 2, 3]),
        ImageResource {
            signature: *b"8BIM",
            id: 1026,
            name: b"grp".to_vec(),
            data: vec![0, 1, 0, 1, 0, 0],
        },
    ];
    let big = resources_to_bytes(&res).unwrap();
    let (back, w) = resources_from_bytes(&big).unwrap();
    assert!(w.is_empty());
    assert_eq!(back, res);
    let (little, w) = transcode_resources(&big, ByteOrder::Big, ByteOrder::Little).unwrap();
    assert!(w.is_empty());
    assert!(little.starts_with(b"MIB8"));
    assert_eq!(little.len(), big.len());
    assert_eq!(&little[4..6], &[0xed, 3]);
    let (back, w) = resources_from_bytes(&little).unwrap();
    assert!(w.is_empty());
    assert_eq!(back, res);
    // The ICC blob is untouched by the swap.
    assert!(little.windows(3).any(|w| w == [1, 2, 3]));
    // An odd trailing pad byte is tolerated.
    let mut padded = big.clone();
    padded.push(0);
    assert_eq!(resources_from_bytes(&padded).unwrap().0, res);
}

#[test]
fn malformed_input_errors_without_panicking() {
    assert!(ImageSourceData::from_bytes(b"").is_err());
    assert!(ImageSourceData::from_bytes(b"Adobe Photoshop Document Data Block").is_err());
    let mut bad = SIGNATURE.to_vec();
    bad.extend_from_slice(b"XXXXLayr");
    assert!(ImageSourceData::from_bytes(&bad).is_err());
    let d = sample_layers();
    for order in [ByteOrder::Big, ByteOrder::Little] {
        let (bytes, _) = d.to_bytes(order).unwrap();
        for cut in (0..bytes.len())
            .step_by(7)
            .chain(bytes.len().saturating_sub(8)..bytes.len())
        {
            let _ = ImageSourceData::from_bytes(&bytes[..cut]);
        }
        // Bit flips in the structure (not the signature).
        for i in (36..bytes.len()).step_by(13) {
            let mut b = bytes.clone();
            b[i] ^= 0x5a;
            let _ = ImageSourceData::from_bytes(&b);
        }
    }
    // A block that claims more data than exists.
    let mut huge = SIGNATURE.to_vec();
    huge.extend_from_slice(b"8BIMluni");
    huge.extend_from_slice(&u32::MAX.to_be_bytes());
    assert!(ImageSourceData::from_bytes(&huge).is_err());
    assert!(resources_from_bytes(b"8BIM\x03\xed\x00").is_err());
}

#[test]
fn into_psd_roundtrips_through_the_psd_writer() {
    let file = testgen::layered(Version::Psd, ColorMode::Grayscale, 8, Compression::Zip);
    let d = ImageSourceData::from_psd(&file);
    let (little, _) = d.to_bytes(ByteOrder::Little).unwrap();
    let (back, _) = ImageSourceData::from_bytes(&little).unwrap();
    let psd = back.into_psd(
        file.header.clone(),
        file.resources.clone(),
        file.image_data.clone(),
    );
    let bytes = psd.to_bytes().unwrap();
    let again = PsdFile::from_bytes(&bytes).unwrap();
    let expected = known(&ImageSourceData::from_psd(&file));
    assert_eq!(
        known(&ImageSourceData::from_psd(&again)).layer_info,
        expected.layer_info
    );
    assert_eq!(again.global_blocks, expected.global_blocks);
    assert_eq!(again.layer_tree().len(), file.layer_tree().len());
}

/// Global blocks are padded as they are written, not on a copy: the bytes are those of the
/// former path (each block cloned with explicit padding to 4, written with `write_blocks`).
#[test]
fn global_blocks_written_without_copies_keep_their_bytes() {
    for version in [Version::Psd, Version::Psb] {
        let mut d = ImageSourceData {
            version,
            global_layer_mask: Some(GlobalLayerMask {
                data: vec![0, 1, 0, 2, 0, 3, 0, 4, 9],
            }),
            ..Default::default()
        };
        for n in 0..8u8 {
            d.global_blocks
                .push(TaggedBlock::new(*b"FMsk", (0..n).collect()));
        }
        let mut explicit = TaggedBlock::new(*b"FMsk", vec![1, 2, 3]);
        explicit.padding = Some(vec![]);
        d.global_blocks.push(explicit);
        let mut odd = TaggedBlock::new(*b"FMsk", vec![1, 2]);
        odd.padding = Some(vec![0, 0, 0, 0, 0]);
        d.global_blocks.push(odd);
        let mut copies = vec![padded_block(
            *b"LMsk",
            d.global_layer_mask.as_ref().unwrap().data.clone(),
        )];
        for b in &d.global_blocks {
            let mut b = b.clone();
            if b.padding.is_none() {
                b.padding = Some(vec![0; (4 - b.data.len() % 4) % 4]);
            }
            copies.push(b);
        }
        let mut body = Vec::new();
        crate::tagged::write_blocks(&mut body, &copies, version, 2).unwrap();
        let signature = if version.is_psb() {
            SIGNATURE_PSB
        } else {
            SIGNATURE
        };
        assert_eq!(
            d.to_bytes(ByteOrder::Big).unwrap().0,
            [signature, &body[..]].concat()
        );
        let (little, _) =
            transcode_blocks(&body, version, ByteOrder::Big, ByteOrder::Little).unwrap();
        assert_eq!(
            d.to_bytes(ByteOrder::Little).unwrap().0,
            [signature, &little[..]].concat()
        );
    }
}

/// Nested structures are transcoded into one shared buffer. A block whose body fails
/// part-way leaves no partial bytes: dropped on the way out (with the enclosing lengths
/// patched as if it had never been there), or kept exactly as stored on the way in.
#[test]
fn a_nested_block_that_fails_part_way_leaves_nothing_behind() {
    let clean = sample_layers();
    let mut fx = Vec::new();
    fx.put_u32(0);
    fx.extend_from_slice(
        &VersionedDescriptor::new(Descriptor::new("null").with("Opct", Value::Double(1.0)))
            .to_bytes(),
    );
    // A whole number of 4-byte units, so dropping it cannot change any padding.
    fx.resize(fx.len().next_multiple_of(4), 0);
    let mut d = clean.clone();
    d.layer_info.as_mut().unwrap().layers[0]
        .blocks
        .insert(0, TaggedBlock::new(*b"lfx2", fx.clone()));
    let (mut little, w) = d.to_bytes(ByteOrder::Little).unwrap();
    assert!(!w.iter().any(|w| w.starts_with("lfx2")), "{w:?}");
    // An unknown value type after the descriptor's header: parsing fails part-way.
    let at = little.windows(4).position(|w| w == b"buod").unwrap();
    little[at..at + 4].copy_from_slice(b"????");
    let (back, w) = ImageSourceData::from_bytes(&little).unwrap();
    assert_eq!(
        w.iter().filter(|w| w.starts_with("lfx2 block")).count(),
        1,
        "{w:?}"
    );
    let kept = &back.layer_info.as_ref().unwrap().layers[0].blocks[0];
    assert!(kept.is_foreign_order());
    let stored = little.windows(4).position(|w| w == b"2xfl").unwrap() + 8;
    assert_eq!(kept.data, little[stored..stored + fx.len()]);
    // Big-endian data that fails part-way cannot go into a little-endian file: dropped.
    let mut big_bad = d.clone();
    let mut broken = fx.clone();
    let at = broken.windows(4).position(|w| w == b"doub").unwrap();
    broken[at..at + 4].copy_from_slice(b"????");
    big_bad.layer_info.as_mut().unwrap().layers[0].blocks[0].data = broken;
    let (little, w) = big_bad.to_bytes(ByteOrder::Little).unwrap();
    assert_eq!(
        w.iter().filter(|w| w.starts_with("lfx2 block")).count(),
        1,
        "{w:?}"
    );
    assert_eq!(little, clean.to_bytes(ByteOrder::Little).unwrap().0);
}
