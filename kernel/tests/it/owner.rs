//! A content owner's segments (LLP 1123 D16) and container units: a route
//! under a bar reads the segments its host reports, in its content root's
//! coordinates, so its panes land on the window's segments whatever the bar
//! takes; `env()` stays the window's.

use exact_kernel::style::env::SegmentGrid;
use exact_kernel::{
    BlockAxis, Env, HostBlock, HostCover, Kernel, NodeType, Offer, Op, PropId, Rect, StyleId,
    StyleProps, StyleValue,
};

fn style(rows: &[(StyleId, StyleValue)]) -> StyleProps {
    let mut s = StyleProps::default();
    for (id, value) in rows {
        s.set_dynamic(*id, value).unwrap();
    }
    s
}

fn text(t: &str) -> StyleValue<'_> {
    StyleValue::Text(t.into())
}

/// The iPhone Duo's inner panel turned and folded like a laptop: a root, a
/// route filling it under a 134-point bar, the route's upper pane sized by
/// its first segment, its lower pane placed at its second, and a box half
/// the route's height.
fn laptop() -> Kernel {
    let mut kernel = Kernel::with_monospace();
    let ops = vec![
        Op::CreateView {
            id: 1,
            node_type: NodeType::View,
        },
        Op::SetStyle {
            id: 1,
            patch: Box::new(style(&[(StyleId::Height, StyleValue::Percent(100.0))])),
        },
        Op::CreateView {
            id: 2,
            node_type: NodeType::View,
        },
        Op::SetStyle {
            id: 2,
            patch: Box::new(style(&[
                (StyleId::PositionType, text("absolute")),
                (StyleId::Top, StyleValue::Number(0.0)),
                (StyleId::Bottom, StyleValue::Number(0.0)),
                (StyleId::Left, StyleValue::Number(0.0)),
                (StyleId::Right, StyleValue::Number(0.0)),
            ])),
        },
        Op::CreateView {
            id: 3,
            node_type: NodeType::View,
        },
        Op::SetStyle {
            id: 3,
            patch: Box::new(style(&[(
                StyleId::Height,
                text("var(--exact-viewport-segment-height-0-0)"),
            )])),
        },
        Op::CreateView {
            id: 4,
            node_type: NodeType::View,
        },
        Op::SetStyle {
            id: 4,
            patch: Box::new(style(&[
                (StyleId::PositionType, text("absolute")),
                (StyleId::Top, text("var(--exact-viewport-segment-top-0-1)")),
                (StyleId::Bottom, StyleValue::Number(0.0)),
                (StyleId::Left, StyleValue::Number(0.0)),
                (StyleId::Right, StyleValue::Number(0.0)),
            ])),
        },
        Op::CreateView {
            id: 5,
            node_type: NodeType::View,
        },
        Op::SetStyle {
            id: 5,
            patch: Box::new(style(&[
                (StyleId::PositionType, text("absolute")),
                (StyleId::Top, StyleValue::Number(0.0)),
                (StyleId::Height, text("50cqh")),
                (
                    StyleId::Width,
                    text("calc(var(--exact-safe-area-inset-top) + 10px)"),
                ),
            ])),
        },
        Op::SetChildren {
            id: 2,
            children: vec![3, 4, 5],
        },
        Op::SetChildren {
            id: 1,
            children: vec![2],
        },
        Op::AttachRoot { id: 1 },
    ];
    kernel.apply(0, 1, &ops).unwrap();
    // The window's division at 455.5 to 495.5, as the document's segments.
    kernel
        .set_segments(
            1,
            2,
            vec![
                Rect::new(0.0, 0.0, 669.0, 455.5),
                Rect::new(0.0, 495.5, 669.0, 455.5),
            ],
        )
        .unwrap();
    kernel.set_env(Env::new(82.0, 0.0, 34.0, 0.0)).unwrap();
    kernel
        .set_host_cover(2, Some(HostCover::Edges([134.0, 0.0, 0.0, 0.0])))
        .unwrap();
    // The route's host fixes its block, so container units read its content
    // root, the block less what the host clears (D16)
    kernel
        .set_host_block(2, Some(fixed_block(669.0, 951.0)))
        .unwrap();
    kernel
}

fn fixed_block(width: f32, height: f32) -> HostBlock {
    HostBlock {
        tactic: Default::default(),
        width: BlockAxis::Is(width),
        height: BlockAxis::Is(height),
    }
}

fn frame(kernel: &mut Kernel, id: u32) -> (f32, f32, f32, f32) {
    kernel
        .compute_layout(1, Offer::definite(669.0, 951.0))
        .unwrap();
    let f = kernel.node(id).unwrap().frame;
    (f.x, f.y, f.width, f.height)
}

/// What the iOS host reports for the route: its content root starts below
/// the bar, so the division is at 321.5 there.
fn route_segments() -> Option<SegmentGrid> {
    Some(SegmentGrid {
        cols: 1,
        rows: 2,
        rects: vec![
            Rect::new(0.0, 0.0, 669.0, 321.5),
            Rect::new(0.0, 361.5, 669.0, 455.5),
        ],
    })
}

#[test]
fn a_route_under_a_bar_places_its_panes_on_the_windows_segments() {
    let mut kernel = laptop();
    // With nothing reported a box in the route reads no segment (LLP 1123
    // D16), not the window's: the upper pane, which names no fallback, takes
    // its initial height.
    assert_eq!(frame(&mut kernel, 3).3, 0.0);
    assert_eq!(frame(&mut kernel, 3).1, 134.0);
    assert!(
        kernel.set_owner_segments(2, route_segments()).unwrap(),
        "the panes read them"
    );
    assert_eq!(
        frame(&mut kernel, 3),
        (0.0, 134.0, 669.0, 321.5),
        "ends at the band"
    );
    assert_eq!(
        frame(&mut kernel, 4),
        (0.0, 495.5, 669.0, 455.5),
        "starts after it"
    );
    // `cqh` is the route's content height. `var(--exact-safe-area-inset-top)`
    // is main's local inset, which no host reports here, so it is the window's
    // 82 inside the route as outside it.
    assert_eq!(frame(&mut kernel, 5), (0.0, 134.0, 92.0, 408.5));
    // Taken back, the route reads none again.
    assert!(kernel.set_owner_segments(2, None).unwrap());
    assert_eq!(frame(&mut kernel, 3).3, 0.0);
    assert_eq!(frame(&mut kernel, 5).2, 92.0);
}

#[test]
fn a_calculation_with_a_percentage_reads_the_owners_values() {
    let mut kernel = laptop();
    kernel.set_owner_segments(2, route_segments()).unwrap();
    let below = vec![
        Op::CreateView {
            id: 7,
            node_type: NodeType::View,
        },
        Op::SetStyle {
            id: 7,
            patch: Box::new(style(&[
                (
                    StyleId::Height,
                    text("calc(100% - var(--exact-viewport-segment-top-0-1))"),
                ),
                (StyleId::Width, text("calc(10% + 10cqh)")),
            ])),
        },
        Op::SetChildren {
            id: 2,
            children: vec![3, 4, 5, 7],
        },
    ];
    kernel.apply(0, 2, &below).unwrap();
    // The engine resolves these trees after styles are set: they still read
    // the route's segments and height, not the window's (495.5 and 951).
    let (_, y, width, height) = frame(&mut kernel, 7);
    assert_eq!((y, height), (455.5, 455.5));
    assert!((width - 148.6).abs() < 0.01, "{width}");
}

#[test]
fn a_subtree_built_detached_reads_the_owner_it_joins() {
    let mut kernel = laptop();
    kernel.set_owner_segments(2, route_segments()).unwrap();
    frame(&mut kernel, 3);
    // As a runner rebuilds a branch: the new pane is made and styled before
    // it is attached, then joins the route.
    let rebuilt = vec![
        Op::CreateView {
            id: 8,
            node_type: NodeType::View,
        },
        Op::SetStyle {
            id: 8,
            patch: Box::new(style(&[(
                StyleId::Height,
                text("var(--exact-viewport-segment-height-0-0)"),
            )])),
        },
        Op::CreateView {
            id: 9,
            node_type: NodeType::View,
        },
        Op::SetChildren {
            id: 9,
            children: vec![8],
        },
        Op::SetChildren {
            id: 2,
            children: vec![9, 4, 5],
        },
    ];
    kernel.apply(0, 2, &rebuilt).unwrap();
    assert_eq!(
        frame(&mut kernel, 8).3,
        321.5,
        "the route's, not the window's 455.5"
    );
}

#[test]
fn a_popover_in_an_owner_reads_the_documents_values_and_the_owners_container() {
    let mut kernel = laptop();
    kernel.set_owner_segments(2, route_segments()).unwrap();
    let lifted = vec![
        Op::CreateView {
            id: 10,
            node_type: NodeType::View,
        },
        Op::SetProp {
            id: 10,
            prop: exact_kernel::PropId::Popover,
            value: exact_kernel::PropValue::Str("auto".into()),
        },
        Op::CreateView {
            id: 11,
            node_type: NodeType::View,
        },
        Op::SetStyle {
            id: 11,
            patch: Box::new(style(&[
                (StyleId::Width, text("var(--exact-safe-area-inset-top)")),
                (StyleId::Height, text("10cqh")),
            ])),
        },
        Op::SetStyle {
            id: 10,
            patch: Box::new(style(&[(StyleId::Width, StyleValue::Number(400.0))])),
        },
        Op::CreateView {
            id: 12,
            node_type: NodeType::View,
        },
        Op::SetStyle {
            id: 12,
            patch: Box::new(style(&[
                (StyleId::Width, text("calc(10% + 10cqh)")),
                (StyleId::Height, StyleValue::Number(10.0)),
            ])),
        },
        Op::SetChildren {
            id: 10,
            children: vec![11, 12],
        },
        Op::SetChildren {
            id: 2,
            children: vec![3, 4, 5, 10],
        },
    ];
    kernel.apply(0, 2, &lifted).unwrap();
    // The window's 82, not the route's 0 at its cleared top (LLP 1123 D9).
    assert_eq!(frame(&mut kernel, 11).2, 82.0);
    // Its container is still the route's, as CSS keeps a dialog's (D12):
    // 10% of the route's 817, not of the window's 951.
    assert!((frame(&mut kernel, 11).3 - 81.7).abs() < 0.01);
    // So in a calculation with a percentage: 10% of the popover's 400 and
    // the route's 81.7, not the window's 95.1.
    let width = frame(&mut kernel, 12).2;
    assert!((width - 121.7).abs() < 0.01, "{width}");
}

#[test]
fn a_box_in_a_dialog_in_an_owner_reads_the_documents_values_and_the_owners_container() {
    let mut kernel = laptop();
    kernel.set_owner_segments(2, route_segments()).unwrap();
    // As lowering writes a `dialog`: absolute, its tag a prop. The box that
    // reads the values is a level below the dialog's root.
    let dialog = vec![
        Op::CreateView {
            id: 10,
            node_type: NodeType::View,
        },
        Op::SetProp {
            id: 10,
            prop: PropId::SemanticTag,
            value: exact_kernel::PropValue::Str("dialog".into()),
        },
        Op::SetStyle {
            id: 10,
            patch: Box::new(style(&[(StyleId::PositionType, text("absolute"))])),
        },
        Op::CreateView {
            id: 11,
            node_type: NodeType::View,
        },
        Op::CreateView {
            id: 12,
            node_type: NodeType::View,
        },
        Op::SetStyle {
            id: 12,
            patch: Box::new(style(&[
                (StyleId::Width, text("var(--exact-safe-area-inset-top)")),
                (StyleId::Height, text("10cqh")),
            ])),
        },
        Op::SetChildren {
            id: 11,
            children: vec![12],
        },
        Op::SetChildren {
            id: 10,
            children: vec![11],
        },
        Op::SetChildren {
            id: 2,
            children: vec![3, 4, 5, 10],
        },
    ];
    kernel.apply(0, 2, &dialog).unwrap();
    // The window's 82, not the route's 0 at its cleared top (LLP 1123 D9),
    // and 10% of the route's 817, not of the window's 951 (D12).
    let (_, _, width, height) = frame(&mut kernel, 12);
    assert_eq!(width, 82.0);
    assert!((height - 81.7).abs() < 0.01, "{height}");
}

#[test]
fn a_sticky_inset_in_an_owners_scroller_reads_what_the_scroller_leaves() {
    let mut kernel = laptop();
    kernel.set_owner_segments(2, route_segments()).unwrap();
    let scroller = vec![
        Op::CreateView {
            id: 12,
            node_type: NodeType::View,
        },
        Op::SetStyle {
            id: 12,
            patch: Box::new(style(&[
                (StyleId::OverflowY, text("scroll")),
                (StyleId::Height, StyleValue::Number(200.0)),
            ])),
        },
        Op::CreateView {
            id: 13,
            node_type: NodeType::View,
        },
        Op::SetStyle {
            id: 13,
            patch: Box::new(style(&[
                (StyleId::PositionType, text("sticky")),
                (
                    StyleId::Top,
                    text("calc(var(--exact-safe-area-inset-top) + 4px)"),
                ),
                (StyleId::Height, StyleValue::Number(30.0)),
            ])),
        },
        Op::CreateView {
            id: 14,
            node_type: NodeType::View,
        },
        Op::SetStyle {
            id: 14,
            patch: Box::new(style(&[(StyleId::Height, StyleValue::Number(1000.0))])),
        },
        Op::SetChildren {
            id: 12,
            children: vec![13, 14],
        },
        Op::SetChildren {
            id: 2,
            children: vec![3, 4, 5, 12],
        },
    ];
    kernel.apply(0, 2, &scroller).unwrap();
    frame(&mut kernel, 13);
    let sticky = kernel
        .sticky_constraint(kernel.arena().key_of(13).unwrap())
        .unwrap();
    // The scroller insets its content's top, past which its host sticks the
    // box, so `var(--exact-safe-area-inset-top)` reads 0 there, not the
    // route's 82
    assert_eq!(sticky.insets[0], Some(4.0));
}

#[test]
fn an_owner_on_one_segment_has_none_and_its_fallbacks_take_over() {
    let mut kernel = laptop();
    // The route lies wholly above the band: one segment, so none of the
    // segment properties, while the document still has two (LLP 1123 D16).
    kernel.set_owner_segments(2, None).unwrap();
    let halves = vec![
        Op::CreateView {
            id: 12,
            node_type: NodeType::View,
        },
        Op::SetStyle {
            id: 12,
            patch: Box::new(style(&[
                (
                    StyleId::Height,
                    text("var(--exact-viewport-segment-height-0-0, 50%)"),
                ),
                (
                    StyleId::Width,
                    text("var(--exact-viewport-segment-width-0-0, 50%)"),
                ),
            ])),
        },
        Op::SetChildren {
            id: 2,
            children: vec![12],
        },
    ];
    kernel.apply(0, 2, &halves).unwrap();
    // Half the route's content box (669 wide, 951 less the 134 bar tall).
    assert_eq!(frame(&mut kernel, 12).2, 334.5);
    assert_eq!(frame(&mut kernel, 12).3, 408.5);
}

#[test]
fn a_node_outside_every_owner_reads_the_documents_values() {
    let mut kernel = laptop();
    kernel.set_owner_segments(2, route_segments()).unwrap();
    let outside = vec![
        Op::CreateView {
            id: 6,
            node_type: NodeType::View,
        },
        Op::SetStyle {
            id: 6,
            patch: Box::new(style(&[
                (StyleId::PositionType, text("absolute")),
                (StyleId::Width, text("var(--exact-safe-area-inset-top)")),
                (StyleId::Height, text("10cqh")),
            ])),
        },
        Op::SetChildren {
            id: 1,
            children: vec![2, 6],
        },
    ];
    kernel.apply(0, 2, &outside).unwrap();
    // The window's 82, and 10% of the small viewport (the window, natively).
    assert_eq!(frame(&mut kernel, 6).2, 82.0);
    assert_eq!(frame(&mut kernel, 6).3, 95.1);
}

#[test]
fn invalid_segments_are_refused() {
    let mut kernel = laptop();
    let mut bad = route_segments().unwrap();
    bad.rects.pop();
    assert!(
        kernel.set_owner_segments(2, Some(bad)).is_err(),
        "two rows need two rects"
    );
    let mut bad = route_segments().unwrap();
    bad.rects[0].height = f32::NAN;
    assert!(kernel.set_owner_segments(2, Some(bad)).is_err());
}

/// A paragraph `id` of "ab " and an image `id + 2` in its text, `10cqh`
/// square: sized by its owner's container (LLP 1123 D12).
fn paragraph_with_image(id: u32) -> [Op; 6] {
    [
        Op::CreateView {
            id,
            node_type: NodeType::Text,
        },
        Op::CreateView {
            id: id + 1,
            node_type: NodeType::Text,
        },
        Op::SetProp {
            id: id + 1,
            prop: PropId::Text,
            value: "ab ".into(),
        },
        Op::CreateView {
            id: id + 2,
            node_type: NodeType::Image,
        },
        Op::SetStyle {
            id: id + 2,
            patch: Box::new(style(&[
                (StyleId::Width, text("10cqh")),
                (StyleId::Height, text("10cqh")),
            ])),
        },
        Op::SetChildren {
            id,
            children: vec![id + 1, id + 2],
        },
    ]
}

/// Laid out in 400 × 600: the box paragraph `id` reserves for its image,
/// the image's frame and the paragraph's height, to the hundredth.
fn measured(kernel: &mut Kernel, id: u32) -> ((f32, f32), (f32, f32), f32) {
    kernel
        .compute_layout(1, Offer::definite(400.0, 600.0))
        .unwrap();
    let reserved = kernel.node(id).unwrap().text_runs()[1].inline.unwrap();
    let image = kernel.node(id + 2).unwrap().frame;
    let paragraph = kernel.node(id).unwrap().frame.height;
    (
        (reserved.width, reserved.height),
        (image.width, image.height),
        (paragraph * 100.0).round() / 100.0,
    )
}

/// When only the owner's size changes, an image in a paragraph's text is
/// measured again with its paragraph, as a new natural size has it measured.
#[test]
fn an_inline_image_follows_its_owners_size() {
    let mut kernel = Kernel::with_monospace();
    let view = |id| Op::CreateView {
        id,
        node_type: NodeType::View,
    };
    let mut ops = vec![view(1), view(9)];
    ops.extend(paragraph_with_image(2));
    ops.extend([
        Op::SetChildren {
            id: 9,
            children: vec![2],
        },
        Op::SetChildren {
            id: 1,
            children: vec![9],
        },
        Op::AttachRoot { id: 1 },
    ]);
    kernel.apply(0, 1, &ops).unwrap();
    // The image stands on the baseline, 3.84 above the line's bottom. Its
    // owner's host fixes the block its container units read (D16).
    kernel
        .set_host_block(9, Some(fixed_block(400.0, 200.0)))
        .unwrap();
    assert_eq!(
        measured(&mut kernel, 2),
        ((20.0, 20.0), (20.0, 20.0), 23.84)
    );
    kernel
        .set_host_block(9, Some(fixed_block(400.0, 400.0)))
        .unwrap();
    assert_eq!(
        measured(&mut kernel, 2),
        ((40.0, 40.0), (40.0, 40.0), 43.84)
    );
}

/// A paragraph moved, inside a kept box, from one owner to another measures
/// its image again by the new owner's container (LLP 1123 D12).
#[test]
fn an_inline_image_moved_to_another_owner_is_measured_by_it() {
    let mut kernel = Kernel::with_monospace();
    let view = |id| Op::CreateView {
        id,
        node_type: NodeType::View,
    };
    let half = |id| Op::SetStyle {
        id,
        patch: Box::new(style(&[(StyleId::Width, StyleValue::Percent(50.0))])),
    };
    let mut ops = vec![view(1), view(2), half(2), view(3), half(3), view(4)];
    ops.extend(paragraph_with_image(5));
    ops.extend([
        Op::SetChildren {
            id: 4,
            children: vec![5],
        },
        Op::SetChildren {
            id: 2,
            children: vec![4],
        },
        Op::SetChildren {
            id: 1,
            children: vec![2, 3],
        },
        Op::AttachRoot { id: 1 },
    ]);
    kernel.apply(0, 1, &ops).unwrap();
    kernel
        .set_host_block(2, Some(fixed_block(200.0, 300.0)))
        .unwrap();
    kernel
        .set_host_block(3, Some(fixed_block(200.0, 100.0)))
        .unwrap();
    // The image on the baseline, 3.84 above the line's bottom.
    assert_eq!(
        measured(&mut kernel, 5),
        ((30.0, 30.0), (30.0, 30.0), 33.84)
    );
    let moved = Op::SetChildren {
        id: 3,
        children: vec![4],
    };
    kernel.apply(0, 2, &[moved]).unwrap();
    // A line of text is taller than the image now: 19.2.
    assert_eq!(measured(&mut kernel, 5), ((10.0, 10.0), (10.0, 10.0), 19.2));
}

#[test]
fn a_box_made_a_popover_at_run_time_takes_the_documents_values_and_gives_them_back() {
    let mut kernel = laptop();
    kernel.set_owner_segments(2, route_segments()).unwrap();
    let boxed = vec![
        Op::CreateView {
            id: 10,
            node_type: NodeType::View,
        },
        Op::SetStyle {
            id: 10,
            patch: Box::new(style(&[(StyleId::Width, StyleValue::Number(400.0))])),
        },
        Op::CreateView {
            id: 11,
            node_type: NodeType::View,
        },
        Op::SetStyle {
            id: 11,
            patch: Box::new(style(&[
                (
                    StyleId::Width,
                    text("var(--exact-viewport-segment-height-0-0)"),
                ),
                (StyleId::Height, StyleValue::Number(10.0)),
            ])),
        },
        Op::SetChildren {
            id: 10,
            children: vec![11],
        },
        Op::SetChildren {
            id: 2,
            children: vec![3, 4, 5, 10],
        },
    ];
    kernel.apply(0, 2, &boxed).unwrap();
    // In the route: its upper segment's height, 321.5.
    assert_eq!(frame(&mut kernel, 11).2, 321.5);
    let lift = Op::SetProp {
        id: 10,
        prop: exact_kernel::PropId::Popover,
        value: exact_kernel::PropValue::Str("auto".into()),
    };
    let receipt = kernel.apply(0, 3, &[lift]).unwrap();
    // In the top layer: the window's, 455.5 (LLP 1123 D9), and the host
    // hears the child whose style that changes.
    assert_eq!(frame(&mut kernel, 11).2, 455.5);
    let child = kernel.node(11).unwrap().key;
    assert!(receipt.touched.contains(&child), "{:?}", receipt.touched);
    let lower = Op::ClearProp {
        id: 10,
        prop: exact_kernel::PropId::Popover,
    };
    kernel.apply(0, 4, &[lower]).unwrap();
    assert_eq!(frame(&mut kernel, 11).2, 321.5);
}
