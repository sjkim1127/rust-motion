//! Document model tests.

use super::*;
use std::sync::Arc;

fn doc() -> Document {
    Document::with_background(
        "t",
        Size::new(100, 50),
        ColorMode::Rgb,
        SampleType::U8,
        Color::WHITE,
    )
}

#[test]
fn any_lock_counts_every_lock() {
    assert!(!Locks::default().any());
    let each = [
        Locks {
            transparency: true,
            ..Locks::default()
        },
        Locks {
            pixels: true,
            ..Locks::default()
        },
        Locks {
            position: true,
            ..Locks::default()
        },
        Locks {
            artboard: true,
            ..Locks::default()
        },
        Locks {
            all: true,
            ..Locks::default()
        },
    ];
    assert!(each.iter().all(|k| k.any()));
}

#[test]
fn unused_link_group_never_wraps() {
    let mut d = doc();
    assert_eq!(d.unused_link_group(), Some(1));
    let with = |d: &mut Document, groups: &[u64]| {
        d.layers.truncate(1);
        for g in groups {
            let mut l = Layer::raster("l", PixelFormat::RGBA8);
            l.link_group = Some(*g);
            d.layers.push(l);
        }
    };
    with(&mut d, &[3, 7]);
    assert_eq!(d.unused_link_group(), Some(8));
    with(&mut d, &[u64::MAX - 1]);
    assert_eq!(d.unused_link_group(), Some(u64::MAX));
    // A stored maximal id would wrap to 0 (#961): take the smallest free id instead.
    with(&mut d, &[u64::MAX]);
    assert_eq!(d.unused_link_group(), Some(1));
    with(&mut d, &[0, 1, 2, 4, u64::MAX]);
    assert_eq!(d.unused_link_group(), Some(3));
}

#[test]
fn new_document_has_locked_background() {
    let d = doc();
    assert_eq!(d.layers.len(), 1);
    let bg = &d.layers[0];
    assert_eq!(bg.name, "Background");
    assert!(bg.locks.transparency);
    let s = bg.surface().unwrap();
    assert_eq!(s.pixel(0, 0), vec![1.0, 1.0, 1.0, 1.0]);
    assert_eq!(s.pixel(99, 49), vec![1.0, 1.0, 1.0, 1.0]);
    assert_eq!(s.pixel(100, 0), vec![0.0; 4]);
}

/// The Background is the locked bottom pixel layer, whatever it is called.
#[test]
fn background_is_structural_not_named() {
    let mut d = doc();
    let id = d.layers[0].id;
    assert!(d.is_background(id));
    for name in ["Hintergrund", "背景", "Arrière-plan"] {
        d.layers[0].name = name.into();
        assert_eq!(d.background().map(|l| l.id), Some(id), "{name}");
    }
    // A layer called "Background" without the locks, or above the bottom, is not it.
    let mut top = Layer::raster("Background", d.pixel_format());
    top.locks.transparency = true;
    top.locks.position = true;
    let top_id = top.id;
    d.layers.push(top);
    assert!(!d.is_background(top_id));
    d.layers[0].locks.position = false;
    assert!(d.background().is_none());
    d.layers[0].locks.position = true;
    d.layers[0].clipped = true;
    assert!(d.background_mut().is_none());
}

#[test]
fn max_group_depth_counts_nesting_without_recursing() {
    let mut d = doc();
    let bg = d.layers[0].id;
    let mut chain = Layer::raster("L", d.pixel_format());
    for _ in 0..3 {
        chain = Layer::group("G", vec![chain]);
    }
    d.insert_above(Some(bg), chain);
    assert_eq!(d.max_group_depth(), 3);
    // An unfilled group does not add a level (nothing lives inside it).
    d.insert_above(Some(bg), Layer::group("empty", vec![]));
    assert_eq!(d.max_group_depth(), 3);
}

#[test]
fn pixel_format_follows_mode_and_depth() {
    let d = Document::new("c", Size::new(1, 1), ColorMode::Cmyk, SampleType::U16);
    assert_eq!(
        d.pixel_format(),
        PixelFormat::new(ColorMode::Cmyk, SampleType::U16, true)
    );
    let b = Document::new("b", Size::new(1, 1), ColorMode::Bitmap, SampleType::U8);
    assert_eq!(b.pixel_format().mode, ColorMode::Grayscale);
}

#[test]
fn text_runs_with_maximal_lengths_normalize_to_text_length() {
    let t = TextLayer {
        text: "ab".into(),
        runs: vec![
            text::TextRun {
                len: 1,
                style: text::CharStyle::default(),
            },
            text::TextRun {
                len: usize::MAX,
                style: text::CharStyle {
                    size_pt: 24.0,
                    ..Default::default()
                },
            },
        ],
        paragraphs: vec![
            text::ParagraphRun {
                len: 1,
                style: text::ParagraphStyle::default(),
            },
            text::ParagraphRun {
                len: usize::MAX,
                style: text::ParagraphStyle {
                    align: text::TextAlign::Center,
                    ..Default::default()
                },
            },
        ],
        ..Default::default()
    };

    let chars = t.char_runs();
    assert_eq!(chars.iter().map(|r| r.len).sum::<usize>(), t.text.len());
    assert_eq!(chars.len(), 2);
    assert_eq!(chars[1].len, 1);
    assert_eq!(chars[1].style.size_pt, 24.0);

    let paragraphs = t.paragraph_runs();
    assert_eq!(
        paragraphs.iter().map(|r| r.len).sum::<usize>(),
        t.text.len()
    );
    assert_eq!(paragraphs.len(), 2);
    assert_eq!(paragraphs[1].len, 1);
    assert_eq!(paragraphs[1].style.align, text::TextAlign::Center);
}

#[test]
fn insert_remove_shift() {
    let mut d = doc();
    let bg = d.layers[0].id;
    let a = d.insert_above(Some(bg), Layer::raster("A", d.pixel_format()));
    let b = d.insert_above(Some(bg), Layer::raster("B", d.pixel_format()));
    // order bottom->top: bg, B, A
    let names: Vec<_> = d.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Background", "B", "A"]);
    assert!(d.shift(b, 1));
    assert_eq!(d.layers[2].id, b);
    assert!(!d.shift(b, 1));
    assert_eq!(d.remove(a).unwrap().name, "A");
    assert!(d.layer(a).is_none());
}

#[test]
fn insert_below_lands_under_the_reference() {
    let mut d = doc();
    let bg = d.layers[0].id;
    let a = d.insert_above(Some(bg), Layer::raster("A", d.pixel_format()));
    let b = d.insert_below(Some(a), Layer::raster("B", d.pixel_format()));
    let names: Vec<_> = d.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Background", "B", "A"]);
    assert_eq!(d.layers[1].id, b);
}

#[test]
fn groups_walk_and_paths() {
    let mut d = doc();
    let inner = Layer::raster("inner", d.pixel_format());
    let inner_id = inner.id;
    let g = Layer::group("G", vec![inner]);
    let gid = d.insert_above(None, g);
    let walk: Vec<_> = d
        .walk()
        .into_iter()
        .map(|(p, depth, l)| (p, depth, l.name.clone()))
        .collect();
    assert_eq!(
        walk,
        vec![
            (vec![0], 0, "Background".into()),
            (vec![1], 0, "G".into()),
            (vec![1, 0], 1, "inner".into())
        ]
    );
    assert_eq!(d.path_of(inner_id), Some(vec![1, 0]));
    assert_eq!(d.layer(gid).unwrap().blend, BlendMode::PassThrough);
    // insert above a nested layer stays inside the group
    let n = d.insert_above(Some(inner_id), Layer::raster("n", d.pixel_format()));
    assert_eq!(d.path_of(n), Some(vec![1, 1]));
    assert_eq!(d.layer_count(), 4);
}

/// `layer` and `path_of` search the tree directly (no `walk`): they must agree with it at
/// every depth, and miss cleanly.
#[test]
fn layer_lookup_agrees_with_walk() {
    let mut d = doc();
    let format = d.pixel_format();
    let leaf = |n: &str| Layer::raster(n, format);
    let deep = Layer::group(
        "A",
        vec![
            leaf("a0"),
            Layer::group(
                "B",
                vec![leaf("b0"), Layer::group("C", vec![leaf("c0")]), leaf("b2")],
            ),
            leaf("a2"),
        ],
    );
    d.insert_above(None, deep);
    d.insert_above(None, leaf("top"));
    let walk: Vec<(LayerPath, LayerId)> = d.walk().into_iter().map(|(p, _, l)| (p, l.id)).collect();
    assert_eq!(walk.len(), 10);
    for (path, id) in &walk {
        assert_eq!(d.path_of(*id).as_ref(), Some(path));
        assert_eq!(d.layer(*id).map(|l| l.id), Some(*id));
        assert_eq!(d.layer_mut(*id).map(|l| l.id), Some(*id));
    }
    let missing = Layer::raster("elsewhere", d.pixel_format()).id;
    assert!(
        d.path_of(missing).is_none()
            && d.layer(missing).is_none()
            && d.layer_mut(missing).is_none()
    );
}

#[test]
fn a_locked_group_locks_its_contents() {
    let mut d = doc();
    let inner = Layer::raster("inner", d.pixel_format());
    let inner_id = inner.id;
    let mut g = Layer::group("G", vec![Layer::group("H", vec![inner])]);
    g.locks.position = true;
    let gid = d.insert_above(None, g);
    d.layer_mut(inner_id).unwrap().locks.pixels = true;
    let k = d.effective_locks(inner_id);
    assert!(k.position && k.pixels && !k.all);
    assert!(
        !d.effective_locks(gid).pixels,
        "a child's lock doesn't lock its group"
    );
    assert_eq!(d.effective_locks(LayerId(u64::MAX)), Locks::default());
}

#[test]
fn duplicate_assigns_fresh_ids_recursively() {
    let g = Layer::group("G", vec![Layer::raster("x", PixelFormat::RGBA8)]);
    let dup = g.duplicate();
    assert_ne!(g.id, dup.id);
    assert_ne!(g.children().unwrap()[0].id, dup.children().unwrap()[0].id);
    assert_eq!(dup.children().unwrap()[0].name, "x");
}

#[test]
fn duplicate_clears_psd_id_but_keeps_blocks() {
    let mut l = Layer::raster("x", PixelFormat::RGBA8);
    l.psd_id = Some(7);
    l.psd_blocks.push((*b"vmsk", Arc::new(vec![1])));
    let d = l.duplicate();
    assert_eq!(d.psd_id, None);
    assert_eq!(d.psd_blocks, l.psd_blocks);
}

#[test]
fn layer_names_increment() {
    let mut d = doc();
    assert_eq!(d.next_layer_name("Layer"), "Layer 1");
    d.insert_above(None, Layer::raster("Layer 1", d.pixel_format()));
    assert_eq!(d.next_layer_name("Layer"), "Layer 2");
}

#[test]
fn mask_density() {
    let mut m = LayerMask::hide_all();
    assert_eq!(m.value(5, 5), 0.0);
    m.density = 0.5;
    assert!((m.value(5, 5) - 0.5).abs() < 1e-6);
    m.enabled = false;
    assert_eq!(m.value(5, 5), 1.0);
}

#[test]
fn snapshot_clone_is_independent() {
    let mut d = doc();
    let snap = d.clone();
    let bg = d.layers[0].id;
    d.layer_mut(bg)
        .unwrap()
        .surface_mut()
        .unwrap()
        .write_pixel(0, 0, &[0.0, 0.0, 0.0, 1.0]);
    assert_eq!(snap.layers[0].surface().unwrap().pixel(0, 0), vec![1.0; 4]);
}

#[test]
fn layer_content_article_and_kind_name() {
    assert_eq!(LayerContent::Adjustment(Adjustment::Invert).article(), "an");
    assert_eq!(
        LayerContent::Adjustment(Adjustment::Invert).kind_name(),
        "Adjustment"
    );
    assert_eq!(
        LayerContent::Raster(Surface::new(PixelFormat::RGBA8)).article(),
        "a"
    );
    assert_eq!(
        LayerContent::Group(Group {
            children: Vec::new(),
            expanded: true,
            artboard: None
        })
        .article(),
        "a"
    );
}
