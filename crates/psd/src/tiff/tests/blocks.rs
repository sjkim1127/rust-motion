//! Per-block layouts: descriptors, type, masks, vector masks, patterns, filter effects, metadata, links and annotations.

use super::*;

#[test]
fn descriptor_blocks_swap_keys_and_numbers() {
    let mut desc = Descriptor::new("null");
    desc.items.push((
        Id::new("Opct"),
        Value::UnitFloat {
            unit: *b"#Prc",
            value: 75.5,
        },
    ));
    desc.items.push((
        Id::new("Nm  "),
        Value::Text(UnicodeString::new_nul("Drop Shadow")),
    ));
    desc.items.push((Id::new("enab"), Value::Boolean(true)));
    desc.items.push((
        Id::new("Md  "),
        Value::Enumerated {
            type_id: Id::new("BlnM"),
            value: Id::new("Mltp"),
        },
    ));
    desc.items.push((
        Id::new("list"),
        Value::List(vec![
            Value::Integer(-3),
            Value::Double(1.5),
            Value::LargeInteger(7),
        ]),
    ));
    desc.items.push((
        Id::new("longKeyName"),
        Value::Descriptor(Descriptor::new("innr")),
    ));
    let mut data = 0u32.to_be_bytes().to_vec();
    data.extend(VersionedDescriptor::new(desc).to_bytes());
    let (big, little) = block_rt(*b"lfx2", data);
    // Keys are reversed and the version field swapped in the little-endian bytes.
    assert!(little[36..].starts_with(b"MIB82xfl"));
    let body_at = 48;
    assert_eq!(&little[body_at..body_at + 4], &[0, 0, 0, 0]);
    assert_eq!(&little[body_at + 4..body_at + 8], &[16, 0, 0, 0]);
    assert_eq!(&big[body_at + 4..body_at + 8], &[0, 0, 0, 16]);
    assert!(little.windows(4).any(|w| w == b"crP#"), "unit key reversed");
    assert!(
        little.windows(11).any(|w| w == b"longKeyName"),
        "string ids are not reversed"
    );
}

#[test]
fn type_block_layout() {
    let mut data = Vec::new();
    data.put_u16(1);
    for v in [1.0, 0.0, 0.0, 1.0, 12.5, -3.0] {
        data.put_f64(v);
    }
    data.put_u16(50);
    data.extend(VersionedDescriptor::new(Descriptor::new("TxLr")).to_bytes());
    data.put_u16(1);
    data.extend(VersionedDescriptor::new(Descriptor::new("warp")).to_bytes());
    for v in [0.0, 0.0, 100.0, 20.0] {
        data.put_f64(v);
    }
    let (_, little) = block_rt(*b"TySh", data);
    assert!(little[48..].starts_with(&[1, 0]));
    assert_eq!(&little[50..58], &1.0f64.to_le_bytes());
}

#[test]
fn global_layer_mask_block() {
    let d = ImageSourceData {
        global_layer_mask: Some(GlobalLayerMask {
            data: vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50, 128, 0],
        }),
        ..Default::default()
    };
    let (little, w) = d.to_bytes(ByteOrder::Little).unwrap();
    assert!(w.is_empty());
    assert!(little[36..].starts_with(b"MIB8ksML"));
    let (back, _) = ImageSourceData::from_bytes(&little).unwrap();
    assert_eq!(back.global_layer_mask, d.global_layer_mask);
    assert_eq!(back.global_layer_mask.unwrap().opacity(), Some(50));
}

#[test]
fn vector_mask_records() {
    let mut v = Vec::new();
    v.put_u32(3);
    v.put_u32(0);
    // Closed length record, then two linked knots.
    v.put_u16(0);
    v.put_u16(2);
    v.put_i16(1);
    v.put(&[0; 20]);
    for sel in [1u16, 1] {
        v.put_u16(sel);
        for i in 0..6 {
            v.put_i32((i + 1) * 0x0010_0000);
        }
    }
    let (_, little) = block_rt(*b"vmsk", v);
    assert!(
        little.windows(4).any(|w| w == [0, 0, 0x10, 0]),
        "fixed-point coordinate swapped"
    );
}

#[test]
fn pattern_block() {
    let plane: Vec<u8> = (0..16u8).collect();
    let p = crate::patterns::PsdPattern {
        mode: 3,
        width: 4,
        height: 4,
        name: "dots".into(),
        id: "abc".into(),
        palette: None,
        depth: 8,
        channels: vec![plane.clone(), plane.clone(), plane.clone()],
        alpha: Some(plane),
    };
    let data = crate::patterns::write_pattern_block(std::slice::from_ref(&p)).unwrap();
    block_rt(*b"Patt", data.clone());
    // Little-endian pattern data parses back into the same pattern.
    let d = ImageSourceData {
        global_blocks: vec![TaggedBlock::new(*b"Patt", data)],
        ..Default::default()
    };
    let (little, _) = d.to_bytes(ByteOrder::Little).unwrap();
    let (back, _) = ImageSourceData::from_bytes(&little).unwrap();
    assert_eq!(
        crate::patterns::parse_pattern_block(&back.global_blocks[0].data).unwrap(),
        vec![p]
    );
}

#[test]
fn filter_effects_block() {
    use crate::filter_effects::{EffectsPlane, FilterEffects, FilterEffectsItem};
    let plane = EffectsPlane::encode(Compression::Rle, &[1, 2, 3, 4, 5, 6], 3, 2, 8).unwrap();
    let mask = EffectsPlane::encode(Compression::Raw, &[9; 6], 3, 2, 8).unwrap();
    let rect = crate::layer::Rect {
        top: 0,
        left: 0,
        bottom: 2,
        right: 3,
    };
    let fx = FilterEffects {
        version: 1,
        items: vec![FilterEffectsItem {
            id: "x".into(),
            version: 1,
            rect,
            depth: 8,
            max_channels: 1,
            slots: vec![Some(plane.clone()), None, Some(plane)],
            mask: Some((rect, mask)),
        }],
    };
    let data = fx.to_bytes();
    block_rt(*b"FEid", data.clone());
    let d = ImageSourceData {
        global_blocks: vec![TaggedBlock::new(*b"FEid", data)],
        ..Default::default()
    };
    let (little, _) = d.to_bytes(ByteOrder::Little).unwrap();
    let (back, _) = ImageSourceData::from_bytes(&little).unwrap();
    assert_eq!(
        FilterEffects::parse(&back.global_blocks[0].data).unwrap(),
        fx
    );
}

#[test]
fn metadata_setting_items() {
    let desc = VersionedDescriptor::new(Descriptor::new("null")).to_bytes();
    let items = vec![crate::metadata::MetadataItem::new(*b"cmls", desc)];
    block_rt(*b"shmd", crate::metadata::write_shmd(&items));
    // An unknown item cannot be converted: it is dropped on its own, the rest stays.
    let items = vec![
        crate::metadata::MetadataItem::new(*b"zzzz", vec![1, 2, 3, 4, 5, 6, 7, 8]),
        crate::metadata::MetadataItem::new(*b"mdyn", vec![0, 0, 0, 1]),
        crate::metadata::MetadataItem::new(
            *b"cmls",
            VersionedDescriptor::new(Descriptor::new("null")).to_bytes(),
        ),
    ];
    let d = ImageSourceData {
        global_blocks: vec![TaggedBlock::new(
            *b"shmd",
            crate::metadata::write_shmd(&items),
        )],
        ..Default::default()
    };
    let (little, w) = d.to_bytes(ByteOrder::Little).unwrap();
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].starts_with("shmd item zzzz dropped"), "{}", w[0]);
    let (back, w) = ImageSourceData::from_bytes(&little).unwrap();
    assert!(w.is_empty());
    let back_items = crate::metadata::parse_shmd(&back.global_blocks[0].data).unwrap();
    assert_eq!(back_items, items[1..].to_vec());
}

#[test]
fn unknown_descriptor_shaped_blocks_are_converted() {
    // A key the transcoder has never heard of, holding a versioned descriptor (as every
    // block Photoshop added since CS6 does): converted, not dropped.
    let mut desc = Descriptor::new("null");
    desc.items
        .push((Id::new("Nm  "), Value::Text(UnicodeString::new_nul("x"))));
    let data = VersionedDescriptor::new(desc.clone()).to_bytes();
    block_rt(*b"Qqqq", data);
    let mut with_version = 3u32.to_be_bytes().to_vec();
    with_version.extend(VersionedDescriptor::new(desc).to_bytes());
    block_rt(*b"Qqqq", with_version);
    // Empty data needs no conversion either.
    block_rt(*b"Mt32", Vec::new());
}

#[test]
fn linked_layer_items() {
    // One embedded file (liFD, version 7) as photocraft-io writes it.
    let mut item = Vec::new();
    item.put(b"liFD");
    item.put_u32(7);
    item.put_u8(4);
    item.put(b"uuid");
    let units: Vec<u16> = "a.png\0".encode_utf16().collect();
    item.put_u32(units.len() as u32);
    for u in units {
        item.put_u16(u);
    }
    item.put(b"png ");
    item.put(b"8BIM");
    let bytes = [0x89, b'P', b'N', b'G', 1, 2, 3];
    item.put_u64(bytes.len() as u64);
    item.put_u8(0);
    item.put(&bytes);
    item.put_u32(0);
    item.put_f64(0.0);
    item.put_u8(0);
    let mut data = (item.len() as u64).to_be_bytes().to_vec();
    data.put(&item);
    while !data.len().is_multiple_of(4) {
        data.push(0);
    }
    let (_, little) = block_rt(*b"lnk2", data);
    assert!(
        little.windows(4).any(|w| w == b"DFil"),
        "item type key reversed"
    );
    assert!(
        little.windows(7).any(|w| w == bytes),
        "file bytes untouched"
    );
}

#[test]
fn annotation_block() {
    let mut a = Vec::new();
    a.put_u16(2);
    a.put_u16(1);
    a.put_u32(1);
    let mut item = Vec::new();
    item.put(b"txtA");
    item.put_u8(0);
    item.put_u8(1);
    item.put_u16(0);
    for v in [10, 20, 30, 40, 50, 60, 70, 80] {
        item.put_i32(v);
    }
    item.put_u16(0);
    for v in [0xffff, 0, 0, 0] {
        item.put_u16(v);
    }
    for s in [&b"me"[..], b"note", b"2026"] {
        item.put_u8(s.len() as u8);
        item.put(s);
        if (1 + s.len()) % 2 == 1 {
            item.put_u8(0);
        }
    }
    let text = [0xfe, 0xff, 0, b'h', 0, b'i'];
    let mut d = Vec::new();
    d.put(b"txtC");
    d.put_u32(text.len() as u32);
    d.put(&text);
    item.put_u32(d.len() as u32 + 4);
    item.put(&d);
    a.put_u32(item.len() as u32 + 4);
    a.put(&item);
    let (_, little) = block_rt(*b"Anno", a);
    assert!(little.windows(4).any(|w| w == b"Atxt"));
    assert!(little.windows(6).any(|w| w == text), "text kept as stored");
}
