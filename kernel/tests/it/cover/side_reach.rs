//! A scroller on a side a native container covers (the iPhone Duo's side strip, a sidebar): what it reaches under the
//! container's controls and what decides that reach.

use super::{frame, route, style};
use exact_kernel::{HostCover, Kernel, NodeType, Op, StyleId, StyleValue};

/// `route()` with a `scroll` (10) in its list's place, holding a column (11)
/// that fills it, and `rows` on the scroll.
fn side_scroll(rows: &[(StyleId, StyleValue<'static>)]) -> Kernel {
    let mut kernel = route();
    let ops = vec![
        Op::CreateView {
            id: 10,
            node_type: NodeType::ScrollView,
        },
        Op::SetStyle {
            id: 10,
            patch: Box::new(style(
                &[
                    &[
                        (StyleId::FlexGrow, StyleValue::Number(1.0)),
                        (StyleId::MinHeight, StyleValue::Number(0.0)),
                    ],
                    rows,
                ]
                .concat(),
            )),
        },
        Op::CreateView {
            id: 11,
            node_type: NodeType::View,
        },
        Op::SetStyle {
            id: 11,
            patch: Box::new(style(&[(StyleId::Height, StyleValue::Number(300.0))])),
        },
        Op::SetChildren {
            id: 10,
            children: vec![11],
        },
        Op::SetChildren {
            id: 2,
            children: vec![3, 4, 10],
        },
    ];
    kernel.apply(1, 2, &ops).unwrap();
    kernel
}

#[test]
fn a_scroll_on_a_covered_side_reaches_under_the_side_controls() {
    let mut kernel = side_scroll(&[]);
    // The iPhone Duo's cover display: its side strip holds the bar's items.
    kernel
        .set_host_cover(2, Some(HostCover::Edges([0.0, 84.0, 0.0, 0.0])))
        .unwrap();
    assert_eq!(
        frame(&mut kernel, 10).2,
        390.0,
        "its box runs under the strip, where UIKit draws its indicator"
    );
    assert_eq!(
        (frame(&mut kernel, 11).0, frame(&mut kernel, 11).2),
        (0.0, 306.0),
        "and its content keeps its place"
    );
    assert_eq!(
        frame(&mut kernel, 4).2,
        306.0,
        "a box that is no scroll stays clear"
    );
    // The cover changing re-derives the scroll that reaches past it.
    kernel
        .set_host_cover(2, Some(HostCover::Edges([0.0, 0.0, 0.0, 47.0])))
        .unwrap();
    assert_eq!(frame(&mut kernel, 10).2, 390.0);
    assert_eq!(
        frame(&mut kernel, 11).0,
        47.0,
        "turned, the strip on the left"
    );
    kernel.set_host_cover(2, None).unwrap();
    assert_eq!(
        (frame(&mut kernel, 11).0, frame(&mut kernel, 11).2),
        (0.0, 390.0),
        "uncovered, as authored"
    );
}

#[test]
fn a_scroll_off_the_covered_side_does_not_reach_it() {
    let mut kernel = side_scroll(&[(StyleId::MarginRight, StyleValue::Number(8.0))]);
    kernel
        .set_host_cover(2, Some(HostCover::Edges([0.0, 84.0, 0.0, 0.0])))
        .unwrap();
    assert_eq!(
        frame(&mut kernel, 10).2,
        298.0,
        "a margin keeps it off the strip: 390 - 84 - 8"
    );
}

#[test]
fn a_scroll_narrower_than_its_box_does_not_reach_the_strip() {
    let mut kernel = side_scroll(&[
        (StyleId::BoxSizing, StyleValue::Text("border-box".into())),
        (StyleId::MaxWidth, StyleValue::Number(200.0)),
    ]);
    kernel
        .set_host_cover(2, Some(HostCover::Edges([0.0, 84.0, 0.0, 0.0])))
        .unwrap();
    assert_eq!(
        (frame(&mut kernel, 10).2, frame(&mut kernel, 11).2),
        (200.0, 200.0),
        "its maximum ends it before the strip: no reach, its content unpadded"
    );
}

#[test]
fn a_change_to_what_decides_the_reach_derives_it_again() {
    let cover = Some(HostCover::Edges([0.0, 84.0, 0.0, 0.0]));
    let pad = |kernel: &mut Kernel| {
        let patch = Box::new(style(&[(StyleId::PaddingRight, StyleValue::Number(8.0))]));
        kernel
            .apply(2, 3, &[Op::SetStyle { id: 2, patch }])
            .unwrap();
    };
    // The route's padding written after the scroll reached past its side,
    let mut updated = side_scroll(&[]);
    updated.set_host_cover(2, cover).unwrap();
    assert_eq!(frame(&mut updated, 10).2, 390.0);
    pad(&mut updated);
    // and before.
    let mut fresh = side_scroll(&[]);
    pad(&mut fresh);
    fresh.set_host_cover(2, cover).unwrap();
    for id in [10, 11] {
        assert_eq!(frame(&mut updated, id), frame(&mut fresh, id), "#{id}");
    }
}
