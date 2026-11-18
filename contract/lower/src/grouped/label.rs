//! A section's `header` or `footer` (LLP 1084 D3): one `text`, the words the
//! platform's list draws there, and after it the `button`s it seats at the
//! label's trailing edge, as SwiftUI's `Section` header of a text and a
//! button and a UIKit header with an accessory do.

use super::{is, m, n, s, with};
use contract_syntax::{Attr, Node, Span};

/// Whether `children` are a label's: its text, then buttons only.
pub(super) fn admitted(children: &[Node]) -> bool {
    matches!(children, [text, buttons @ ..] if is(text, "text") && buttons.iter().all(|b| is(b, "button")))
}

/// The sheet of a label that holds buttons, for the hosts that draw it: a
/// row whose text takes the room the buttons leave. The label's own rows go
/// into `rows`; its children come back with the text's.
pub(super) fn beside(children: &[Node], rows: &mut Vec<Attr>, span: Span) -> Vec<Node> {
    let [text, buttons @ ..] = children else {
        return children.to_vec();
    };
    if buttons.is_empty() {
        return children.to_vec();
    }
    rows.extend([
        s("display", "flex", span),
        s("flex-direction", "row", span),
        s("align-items", "center", span),
        m("gap", "row-gap", span),
    ]);
    let grows = vec![n("flex-grow", 1.0, span), n("min-width", 0.0, span)];
    let mut out = vec![with(text, grows)];
    out.extend(buttons.iter().cloned());
    out
}
