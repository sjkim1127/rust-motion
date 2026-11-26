//! Legacy effects, adjustment layers and gradient maps.

use super::*;

#[test]
fn legacy_effects_and_adjustments() {
    // lrFX with a drop shadow and a solid fill.
    let mut fx = Vec::new();
    fx.put_u16(0);
    fx.put_u16(3);
    fx.put(b"8BIMcmnS");
    fx.put_u32(7);
    fx.put_u32(0);
    fx.put_u8(1);
    fx.put_u16(0);
    fx.put(b"8BIMdsdw");
    let mut sh = Vec::new();
    sh.put_u32(2);
    for v in [5 << 16, 75 << 16, 120 << 16, 7 << 16] {
        sh.put_i32(v);
    }
    for _ in 0..5 {
        sh.put_u16(0);
    }
    sh.put(b"8BIMmul ");
    sh.put_u8(1);
    sh.put_u8(1);
    sh.put_u8(191);
    sh.put_u16(0);
    sh.put_u16(0x1234);
    sh.put_u16(0);
    sh.put_u16(0);
    sh.put_u16(0);
    fx.put_u32(sh.len() as u32);
    fx.put(&sh);
    fx.put(b"8BIMsofi");
    let mut so = Vec::new();
    so.put_u32(2);
    so.put(b"8BIMnorm");
    so.put_u16(0);
    so.put_u16(0xffff);
    so.put_u16(0);
    so.put_u16(0);
    so.put_u16(0);
    so.put_u8(255);
    so.put_u8(1);
    so.put_u16(0);
    so.put_u16(0xffff);
    so.put_u16(0);
    so.put_u16(0);
    so.put_u16(0);
    fx.put_u32(so.len() as u32);
    fx.put(&so);
    let (_, little) = block_rt(*b"lrFX", fx);
    assert!(
        little.windows(8).any(|w| w == b"MIB8wdsd"),
        "effect keys reversed"
    );
    assert!(
        little.windows(2).any(|w| w == [0x34, 0x12]),
        "colour component swapped"
    );

    // levl: version + 29 records + the Lvls extension.
    let mut lv = Vec::new();
    lv.put_u16(2);
    for i in 0..29 * 5 {
        lv.put_u16(i as u16);
    }
    lv.put(b"Lvls");
    lv.put_u16(3);
    lv.put_u16(33);
    for i in 0..4 * 5 {
        lv.put_u16(1000 + i as u16);
    }
    let (_, little) = block_rt(*b"levl", lv);
    assert!(little.windows(4).any(|w| w == b"slvL"));

    // curv with the Crv extension.
    let mut cv = vec![0u8];
    cv.put_u16(1);
    cv.put_u32(0b101);
    cv.put_u16(2);
    cv.put_u16(0);
    cv.put_u16(0);
    cv.put_u16(255);
    cv.put_u16(255);
    cv.put_u16(1);
    cv.put_u16(128);
    cv.put_u16(100);
    cv.put(b"Crv ");
    cv.put_u16(4);
    cv.put_u32(1);
    cv.put_u16(2);
    cv.put_u16(1);
    cv.put_u16(7);
    cv.put_u16(9);
    block_rt(*b"curv", cv);

    for (key, data) in [
        (
            *b"hue2",
            vec![0, 2, 0, 0, 0, 10, 0, 20, 0, 30, 0, 40, 0, 50, 0, 60],
        ),
        (*b"brit", vec![0, 10, 0, 20, 0, 30, 1, 0]),
        (*b"thrs", vec![0, 128, 0, 0]),
        (*b"post", vec![0, 4, 0, 0]),
        (*b"nvrt", vec![]),
        (
            *b"expA",
            [
                1u16.to_be_bytes().to_vec(),
                0.5f32.to_be_bytes().to_vec(),
                0.0f32.to_be_bytes().to_vec(),
                1.0f32.to_be_bytes().to_vec(),
            ]
            .concat(),
        ),
        (*b"blnc", vec![0; 20]),
        (*b"mixr", vec![0, 1, 0, 0, 0, 100, 0, 0, 0, 0, 0, 0, 0, 0]),
        (*b"selc", vec![0, 1, 0, 0, 0, 5, 0, 6, 0, 7, 0, 8]),
        (
            *b"phfl",
            vec![0, 3, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 25, 1, 0],
        ),
        (
            *b"phfl",
            vec![0, 2, 0, 0, 1, 0, 2, 0, 3, 0, 4, 0, 0, 0, 25, 1, 0],
        ),
        (*b"lclr", vec![0, 1, 0, 0, 0, 0, 0, 0]),
        (
            *b"fxrp",
            [1.5f64.to_be_bytes().to_vec(), 2.5f64.to_be_bytes().to_vec()].concat(),
        ),
        (*b"brst", vec![0, 0, 0, 1, 0, 0, 0, 2]),
        (*b"FMsk", vec![0, 0, 255, 255, 0, 0, 0, 0, 0, 0, 0, 50]),
        (
            *b"lsct",
            [3u32.to_be_bytes().to_vec(), b"8BIMpass".to_vec()].concat(),
        ),
        (*b"lspf", vec![0, 0, 0, 5]),
        (*b"lnsr", b"layr".to_vec()),
        (*b"knko", vec![1, 0, 0, 0]),
        (*b"knko", vec![2, 0, 0, 0]),
        (*b"infx", vec![1, 0, 0, 0]),
        (*b"clbl", vec![0, 0, 0, 0]),
        (*b"tsly", vec![0, 0, 0, 0]),
        (*b"lmgm", vec![1, 0, 0, 0]),
        (*b"vmgm", vec![1, 0, 0, 0]),
        (*b"iOpa", vec![128, 0, 0, 0]),
    ] {
        let (big, little) = block_rt(key, data.clone());
        if data.len() >= 2 && key != *b"nvrt" {
            assert_ne!(big, little, "{}", String::from_utf8_lossy(&key));
        }
    }
}

#[test]
fn gradient_map_layout() {
    let mut g = Vec::new();
    g.put_u16(3);
    g.put_u8(0);
    g.put_u8(1);
    g.put(b"Lnr ");
    g.put_u32(2);
    g.put_u16(u16::from(b'a'));
    g.put_u16(0);
    g.put_u16(2);
    for (loc, mid) in [(0u32, 50u32), (4096, 50)] {
        g.put_u32(loc);
        g.put_u32(mid);
        g.put_u16(0);
        g.put_u16(0x1111);
        g.put_u16(0x2222);
        g.put_u16(0x3333);
        g.put_u16(0);
        g.put_u16(0);
    }
    g.put_u16(2);
    for loc in [0u32, 4096] {
        g.put_u32(loc);
        g.put_u32(50);
        g.put_u16(100);
    }
    g.put_u16(2);
    g.put_u16(1);
    g.put_u16(32);
    g.put_u16(0);
    g.put_u32(0xdead_beef);
    for _ in 0..12 {
        g.put_u16(0);
    }
    let (_, little) = block_rt(*b"grdm", g);
    assert!(little.windows(4).any(|w| w == 0xdead_beefu32.to_le_bytes()));
    assert!(little.windows(4).any(|w| w == b" rnL"));
}
