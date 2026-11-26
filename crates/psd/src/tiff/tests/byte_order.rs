//! Unknown blocks, RLE row counts and blending ranges across byte orders.

use super::*;

#[test]
fn unknown_blocks_kept_in_big_dropped_in_little() {
    let mut d = sample_layers();
    d.global_blocks
        .push(TaggedBlock::new(*b"Zzzz", vec![1, 2, 3, 4, 5]));
    let (big, w) = d.to_bytes(ByteOrder::Big).unwrap();
    assert!(w.is_empty());
    let (back, w) = ImageSourceData::from_bytes(&big).unwrap();
    assert!(w.is_empty());
    assert_eq!(back.global_blocks, d.global_blocks);
    // Big-endian data of unknown layout cannot go into a little-endian file.
    let (little, w) = d.to_bytes(ByteOrder::Little).unwrap();
    assert_eq!(
        w.iter()
            .filter(|w| w.starts_with("Zzzz block (5 bytes) dropped"))
            .count(),
        1,
        "{w:?}"
    );
    let (back, w) = ImageSourceData::from_bytes(&little).unwrap();
    assert!(w.is_empty());
    assert_eq!(
        back.global_blocks.iter().map(|b| b.key).collect::<Vec<_>>(),
        vec![*b"Patt", *b"FMsk"]
    );
    assert_eq!(known(&back).layer_info, known(&d).layer_info);
}

#[test]
fn unknown_blocks_from_a_little_endian_file_are_kept_with_their_byte_order() {
    let d = sample_layers();
    let (little, _) = d.to_bytes(ByteOrder::Little).unwrap();
    // A little-endian file with a block of unknown layout, among known ones.
    let mut raw = little.clone();
    raw.extend_from_slice(b"MIB8zzzZ");
    raw.extend_from_slice(&6u32.to_le_bytes());
    raw.extend_from_slice(&[9, 8, 7, 6, 5, 4, 0, 0]);
    raw.extend_from_slice(b"MIB8diyl");
    raw.extend_from_slice(&4u32.to_le_bytes());
    raw.extend_from_slice(&7u32.to_le_bytes());
    let (back, w) = ImageSourceData::from_bytes(&raw).unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(known(&back).layer_info, known(&d).layer_info);
    let kept = back
        .global_blocks
        .iter()
        .find(|b| &b.key == b"Zzzz")
        .expect("kept");
    assert!(kept.is_foreign_order());
    assert_eq!(kept.signature, *b"MIB8");
    assert_eq!(kept.data, [9, 8, 7, 6, 5, 4]);
    // The known block after it was still converted.
    assert_eq!(
        back.global_blocks
            .iter()
            .find(|b| &b.key == b"lyid")
            .unwrap()
            .data,
        7u32.to_be_bytes()
    );
    // Written back to a little-endian TIFF, the bytes are exactly what was read.
    let (again, w) = back.to_bytes(ByteOrder::Little).unwrap();
    assert!(w.is_empty(), "{w:?}");
    let tail = &raw[raw.len() - 18 - 16..];
    assert!(
        again.windows(tail.len()).any(|x| x == tail),
        "foreign-order block restored verbatim"
    );
    assert_eq!(
        ImageSourceData::from_bytes(&again).unwrap().0.global_blocks,
        back.global_blocks
    );
    // The big-endian (PSD) model keeps the marker, so a PSD writer can tell it apart.
    let (big, _) = back.to_bytes(ByteOrder::Big).unwrap();
    assert!(big.windows(8).any(|x| x == b"MIB8Zzzz"));
    assert_eq!(
        ImageSourceData::from_bytes(&big).unwrap().0.global_blocks,
        back.global_blocks
    );
}

#[test]
fn rle_counts_follow_the_container_and_samples_do_not() {
    let file = testgen::layered(Version::Psd, ColorMode::Rgb, 16, Compression::Rle);
    let d = ImageSourceData::from_psd(&file);
    let (big, _) = d.to_bytes(ByteOrder::Big).unwrap();
    let (little, _) = d.to_bytes(ByteOrder::Little).unwrap();
    let (back, _) = ImageSourceData::from_bytes(&little).unwrap();
    let d = known(&d);
    let (a, b) = (
        d.layer_info.as_ref().unwrap(),
        back.layer_info.as_ref().unwrap(),
    );
    for (la, lb) in a.layers.iter().zip(&b.layers) {
        for (ca, cb) in la.channels.iter().zip(&lb.channels) {
            assert_eq!(ca, cb);
            let (w, h) = la.channel_rect(ca.id).size().unwrap();
            assert_eq!(
                ca.decode(w, h, 16, Version::Psd).unwrap(),
                cb.decode(w, h, 16, Version::Psd).unwrap()
            );
        }
    }
    // The encoded samples (after the count table) are identical bytes in both files, while
    // the count table itself is swapped.
    let (rec, ch) = a
        .layers
        .iter()
        .flat_map(|l| {
            l.channels
                .iter()
                .filter(|c| c.id >= -1 && c.compression == Some(Compression::Rle))
                .map(move |c| (l, c))
        })
        .find(|(l, c)| l.rect.height() > 1 && c.data.len() > l.rect.height() as usize * 2 + 8)
        .unwrap();
    let table = rec.rect.height() as usize * 2;
    let samples = &ch.data[table..];
    assert!(big.windows(samples.len()).any(|w| w == samples));
    assert!(little.windows(samples.len()).any(|w| w == samples));
    let counts = &ch.data[..table];
    let swapped: Vec<u8> = counts.chunks(2).flat_map(|c| [c[1], c[0]]).collect();
    assert!(little
        .windows(table + samples.len())
        .any(|w| w[..table] == swapped[..] && w[table..] == *samples));
}

#[test]
fn blending_ranges_are_bytes_in_either_order() {
    let mut file = testgen::layered(Version::Psd, ColorMode::Rgb, 8, Compression::Rle);
    let ranges: Vec<u8> = [0u8, 0, 255, 255].repeat(10);
    let marked: Vec<u8> = (1u8..=40).collect();
    for (i, l) in file
        .layer_info
        .as_mut()
        .unwrap()
        .layers
        .iter_mut()
        .enumerate()
    {
        l.blending_ranges.data = if i == 0 {
            marked.clone()
        } else {
            ranges.clone()
        };
    }
    let d = ImageSourceData::from_psd(&file);
    let (little, _) = d.to_bytes(ByteOrder::Little).unwrap();
    // Written verbatim (what Photoshop writes in an `II` TIFF), not swapped per u32.
    assert!(little.windows(marked.len()).any(|w| w == marked));
    let (back, _) = ImageSourceData::from_bytes(&little).unwrap();
    for (a, b) in d
        .layer_info
        .as_ref()
        .unwrap()
        .layers
        .iter()
        .zip(&back.layer_info.as_ref().unwrap().layers)
    {
        assert_eq!(a.blending_ranges, b.blending_ranges);
    }
}
