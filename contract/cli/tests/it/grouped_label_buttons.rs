//! LLP 1084 D3: a grouped section's `header` or `footer` is one `text` and,
//! after it, the buttons the platform's list draws at its trailing edge
//! (SwiftUI's `Section` header of a text and a button; expo/exact#836).

use super::grouped_list::{boot_source, id};

/// `body` under a 402 × 874 column.
fn app(body: &str) -> String {
    let body = body
        .lines()
        .map(|l| format!("      {l}\n"))
        .collect::<String>();
    format!(
        "component App\n  state n = 1\n  action remove\n    n = n - 1\n  view\n    column testId=\"root\" width=402 height=874\n{body}"
    )
}

fn refused(body: &str) -> String {
    let e = contract::compile(&app(body)).unwrap_err();
    assert_eq!(e.id, "lower-grouped-list", "{}", e.message);
    e.message
}

#[test]
fn a_label_holds_its_text_and_the_buttons_after_it() {
    let r = boot_source(&app("list appearance=\"auto\" testId=\"list\" flex=1
  section
    header testId=\"header\"
      text \"Série 1\" testId=\"title\"
      button press=remove aria-label=\"Edit série 1\" testId=\"edit\"
        text \"Edit\"
      button press=remove aria-label=\"Delete série 1\" testId=\"delete\"
        image \"symbol:delete\"
    text \"Run\"
    footer testId=\"footer\"
      text \"Three sets.\" testId=\"note\"
      button press=remove testId=\"more\"
        text \"More\"
  section
    header
      text \"Plain\"
    text \"Walk\"
  section
    header
      text \"\"
      button press=remove testId=\"alone\"
        text \"Add\"
    text \"Swim\""));
    let k = r.kernel();
    let list = k.grouped_list(id(&r, "list")).unwrap();
    let s = &list.sections[0];
    assert_eq!(s.header.as_deref(), Some("Série 1"));
    assert_eq!(s.header_buttons, [id(&r, "edit"), id(&r, "delete")]);
    assert_eq!(s.footer.as_deref(), Some("Three sets."));
    assert_eq!(s.footer_buttons, [id(&r, "more")]);
    assert!(list.sections[1].header_buttons.is_empty());
    // A blank text beside a button is still a label, which the platform's
    // list makes only where a section has one.
    assert_eq!(list.sections[2].header.as_deref(), Some(""));
    assert_eq!(list.sections[2].header_buttons, [id(&r, "alone")]);

    // The sheet the other hosts draw: the text takes the room the buttons
    // leave, and they stand at the label's trailing inset, centred on it.
    let frame = |name: &str| k.node(id(&r, name)).unwrap().frame;
    for (label, text, last) in [("header", "title", "delete"), ("footer", "note", "more")] {
        let (label, text, last) = (frame(label), frame(text), frame(last));
        let inset = text.x;
        assert!(inset > 0.0, "the label's inset: {inset}");
        assert!(
            (last.x + last.width - (label.width - inset)).abs() < 0.5,
            "the last button ends at the trailing inset: {last:?} in {label:?}"
        );
        assert!(
            ((last.y + last.height / 2.0) - (text.y + text.height / 2.0)).abs() < 0.5,
            "centred on the text: {last:?}, {text:?}"
        );
    }
    let (title, edit, delete) = (frame("title"), frame("edit"), frame("delete"));
    assert!(
        title.x + title.width <= edit.x && edit.x + edit.width <= delete.x,
        "in the order written: {title:?}, {edit:?}, {delete:?}"
    );
}

#[test]
fn a_label_holds_nothing_but_its_text_and_buttons() {
    for body in [
        // A button before the text.
        "header\n      button press=remove\n        text \"Edit\"\n      text \"Sets\"",
        // Buttons alone.
        "header\n      button press=remove\n        text \"Edit\"",
        // A second text.
        "header\n      text \"Sets\"\n      text \"3\"\n      button press=remove\n        text \"Edit\"",
        // Anything else after the text.
        "header\n      text \"Sets\"\n      image \"symbol:delete\"",
        "footer\n      text \"Sets\"\n      input type=\"checkbox\" switch checked=true",
    ] {
        let row = "    text \"Run\"";
        let section = match body.starts_with("header") {
            true => format!("    {body}\n{row}"),
            false => format!("{row}\n    {body}"),
        };
        let message = refused(&format!("list appearance=\"auto\"\n  section\n{section}"));
        assert!(
            message.contains("one `text`") && message.contains("`button`"),
            "{body}: {message}"
        );
    }
}
