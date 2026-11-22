#if os(iOS)
import UIKit
import XCTest
@testable import ExactKit
@testable import ExactNavigation

/// A header's hosted items (LLP 1141.004). Hand-built routes check projection
/// and lifting. The app fixture covers the laid-out bar's input, overflow and
/// accessibility.
final class BarHostedIOSTests: XCTestCase {
    private var window: UIWindow!
    private var sessions: [ExactSession] = []

    override func tearDown() {
        for session in sessions { session.destroy() }
        sessions = []
        window?.isHidden = true
        window = nil
        super.tearDown()
    }

    /// A route whose header holds a level-2 heading "Log" (id 3) and then
    /// `rest`, created as given; `kids` are the header's children after the
    /// heading, `more` any further `children` ops.
    private func route(_ rest: [[String: Any]], kids: [Int], more: [[String: Any]] = [], header: [Int]? = nil) -> Presenter {
        // A session's presenter: a face's texts are set by its text engine.
        let session = ExactApp.shared.makeSession(label: "hosted-\(UUID().uuidString)")
        sessions.append(session)
        let p = session.presenter
        window = UIWindow(frame: CGRect(x: 0, y: 0, width: 400, height: 400))
        p.viewport.frame = window.bounds
        window.addSubview(p.viewport)
        window.makeKeyAndVisible()
        var ops: [[String: Any]] = [
            ["op": "create", "id": 1, "kind": "view", "props": ["navigationKey": "log"]],
            ["op": "create", "id": 2, "kind": "view", "props": ["semanticTag": "header"]],
            ["op": "create", "id": 3, "kind": "text", "props": ["text": "Log", "accessibilityRole": "heading"]],
        ]
        ops += rest + more
        ops += [["op": "children", "id": 2, "ids": header ?? [3] + kids], ["op": "children", "id": 1, "ids": [2]], ["op": "roots", "ids": [1]]]
        p.apply(wireBatch(ops))
        return p
    }

    private func text(_ id: Int, _ words: String, _ props: [String: String] = [:], style: [String: Any] = [:]) -> [String: Any] {
        ["op": "create", "id": id, "kind": "text", "props": ["text": words].merging(props) { $1 }, "style": style]
    }

    private func status(_ id: Int, _ props: [String: String] = [:], style: [String: Any] = [:]) -> [String: Any] {
        ["op": "create", "id": id, "kind": "view", "props": ["accessibilityRole": "status"].merging(props) { $1 }, "style": style]
    }

    private let dot: [String: Any] = ["background_color": [52, 199, 89, 255], "width": 7, "height": 7]

    private func progress(_ id: Int) -> [String: Any] { ["op": "create", "id": id, "kind": "control", "props": ["type": "progress"]] }
    private func bar(_ p: Presenter) throws -> HeaderShape { try XCTUnwrap(HeaderShape(route: try XCTUnwrap(p.views[1]), back: nil, projected: true)) }

    /// A readout hidden as authored, by a box around it, is no item.
    func testAReadoutInABoxHiddenAsAuthoredShowsNothing() throws {
        let p = route([["op": "create", "id": 4, "kind": "view", "style": ["display": "none"]], text(5, "Saved", ["accessibilityRole": "status"])],
                      kids: [4], more: [["op": "children", "id": 4, "ids": [5]]])
        let shape = try bar(p)
        XCTAssertTrue(shape.trailing.isEmpty, "hidden by the box around it")
        XCTAssertTrue(shape.hosted.isEmpty)
    }

    /// What the title takes, it takes whole: a row of a spinner and its words
    /// is one item, with or without `role="status"`; a spinner and then a
    /// text are an item and the subtitle. A status is never the subtitle,
    /// and a symbol beside it in a row is an item of its own.
    func testTheTitleTakesAHostedElementWholeOrNotAtAll() throws {
        for role in ["", "status"] {
            let p = route([["op": "create", "id": 4, "kind": "view", "props": role.isEmpty ? [:] : ["accessibilityRole": role]], progress(5), text(6, "Updating")],
                          kids: [4], more: [["op": "children", "id": 4, "ids": [5, 6]]])
            let shape = try bar(p)
            XCTAssertEqual(shape.trailing.map(\.id), [4], "the row whole: \(role)")
            XCTAssertEqual(shape.trailing.first?.hosted, true)
            XCTAssertEqual(shape.group?.subtitle ?? "", "", "nothing in it is the subtitle: \(role)")
        }
        let siblings = route([progress(5), text(6, "Updating")], kids: [5, 6])
        let apart = try bar(siblings)
        XCTAssertEqual(apart.trailing.map(\.id), [5])
        XCTAssertEqual(apart.group?.subtitle, "Updating")
        let status = route([["op": "create", "id": 4, "kind": "view"],
                            ["op": "create", "id": 5, "kind": "image", "props": ["imageSource": "symbol:sf/checkmark", "symbolName": "checkmark"]],
                            text(6, "Saved", ["accessibilityRole": "status"]), text(7, "3 entries")],
                           kids: [4, 7], more: [["op": "children", "id": 4, "ids": [5, 6]]])
        let saved = try bar(status)
        XCTAssertEqual(saved.trailing.map(\.id), [5, 6])
        XCTAssertEqual(saved.group?.subtitle, "3 entries")
        XCTAssertTrue(saved.group?.glyphs.isEmpty ?? true)
    }

    /// A row of glyphs beside a readout is still the glyph line, the readout
    /// its own item; beside a hidden status it is the glyph line whole.
    func testAGlyphLineBesideAReadoutStaysOne() throws {
        for hidden in [false, true] {
            let p = route([["op": "create", "id": 4, "kind": "view"],
                           ["op": "create", "id": 5, "kind": "image", "props": ["imageSource": "symbol:sf/bell.slash", "symbolName": "bell.slash"]],
                           text(6, "Muted"), text(7, "Live", ["accessibilityRole": "status"], style: hidden ? ["display": "none"] : [:])],
                          kids: [4], more: [["op": "children", "id": 4, "ids": [5, 6, 7]]])
            let shape = try bar(p)
            XCTAssertEqual(shape.group?.glyphs.map(\.symbol), ["bell.slash", nil], "hidden: \(hidden)")
            XCTAssertEqual(shape.group?.subtitle, "Muted")
            XCTAssertEqual(shape.trailing.map(\.id), hidden ? [] : [7])
        }
    }

    /// A disabled button is UIKit's disabled bar item, shown dimmed, though
    /// nothing presses it (the euxy port's "● Clock" written as one); one
    /// neither pressed nor disabled is still the page's.
    func testADisabledButtonIsADisabledItemThoughNothingPressesIt() throws {
        let button = { (id: Int, props: [String: String]) -> [String: Any] in
            ["op": "create", "id": id, "kind": "control", "props": ["type": "button", "accessibilityRole": "button"].merging(props) { $1 }]
        }
        let p = route([button(4, ["disabled": "true", "accessibilityLabel": "Clock"]), button(5, ["accessibilityLabel": "Idle"]),
                       button(6, ["disabled": "false", "accessibilityLabel": "Enabled"])], kids: [4, 5, 6])
        let shape = try XCTUnwrap(HeaderShape(route: try XCTUnwrap(p.views[1]), back: nil, projected: true))
        XCTAssertEqual(shape.trailing.map(\.id), [4])
        XCTAssertTrue(shape.trailing.first?.disabled ?? false)
        XCTAssertTrue(shape.placed.contains { $0.id == 4 })
        XCTAssertFalse(shape.placed.contains { $0.id == 5 || $0.id == 6 })
        // An inert button that presses is an item, dimmed, as in the page.
        let inert = route([["op": "create", "id": 4, "kind": "control", "handlers": ["press"], "props": ["type": "button", "accessibilityRole": "button", "accessibilityLabel": "Send", "inert": "true"]]], kids: [4])
        XCTAssertEqual(try bar(inert).trailing.first?.disabled, true)
    }

    /// Glass is for what presses (D4): an item nothing in which presses has
    /// none, one holding a control the bar has no object for (a switch)
    /// shares the bar's, and one that writes its own material has its own. In
    /// the shared glass, words keep a title item's inset unless the element
    /// writes its sides. A row holding a button is walked, not hosted.
    func testGlassAndInsetAreForWhatPresses() throws {
        func item(_ kids: [[String: Any]], _ ids: [Int], style: [String: Any] = [:], props: [String: String] = [:]) throws -> HeaderShape.Item {
            let p = route([["op": "create", "id": 4, "kind": "view", "props": props, "style": style]] + kids, kids: [4], more: [["op": "children", "id": 4, "ids": ids]])
            return try XCTUnwrap(bar(p).trailing.first)
        }
        let toggle: [String: Any] = ["op": "create", "id": 7, "kind": "control", "props": ["type": "checkbox", "accessibilityRole": "switch", "accessibilityLabel": "Live"]]
        let spinner = try item([progress(5), text(6, "Updating")], [5, 6])
        XCTAssertFalse(spinner.hostedGlass, "a progress only shows")
        XCTAssertEqual(spinner.hostedInset, 0)
        let live = try item([text(6, "Live"), toggle], [6, 7])
        XCTAssertTrue(live.hostedGlass)
        // The inset is for iOS 26's glass, whose capsule's round ends cut into words.
        var glass = false
        if #available(iOS 26.0, *) { glass = true }
        XCTAssertEqual(live.hostedInset, glass ? HeaderShape.Item.titleInset : 0)
        let unworded = try item([text(6, "Live", style: ["display": "none"]), toggle], [6, 7])
        XCTAssertEqual(unworded.hostedInset, 0, "a hidden label shows no words")
        let sided = try item([text(6, "Live"), toggle], [6, 7], style: ["padding_left": 4, "written": ["padding_left"]])
        XCTAssertEqual(sided.hostedInset, 0, "the author's side")
        let own = try item([text(6, "Live"), toggle], [6, 7], props: ["backgroundMaterial": "glass"])
        XCTAssertFalse(own.hostedGlass, "its own material, drawn by it")
        let button: [String: Any] = ["op": "create", "id": 7, "kind": "button", "handlers": ["press"], "props": ["accessibilityLabel": "Retry"]]
        let p = route([text(8, "Inbox"), ["op": "create", "id": 4, "kind": "view"], text(6, "Offline"), button], kids: [8, 4], more: [["op": "children", "id": 4, "ids": [6, 7]]])
        XCTAssertEqual(try bar(p).trailing.map(\.id), [6, 7], "after the subtitle, the text hosted and the button an item")
    }

    /// UIKit's own item is a button whose face is one symbol or one text (a
    /// link among them, which follows its `href`), a native button or a badge
    /// (LLP 1141.004 D1); a pressed row, of words or holding a spinner, is
    /// hosted with its press, in the glass. What the bar replaces is covered by
    /// a bar, so a readout lifted out of it is laid out.
    func testAButtonOfOneTextIsUIKitsItemAndAPressedRowIsHosted() throws {
        let link: [String: Any] = ["op": "create", "id": 4, "kind": "button", "props": ["href": "/history", "accessibilityLabel": "History"]]
        let words: [String: Any] = ["op": "create", "id": 5, "kind": "view", "handlers": ["press"]]
        let spinner: [String: Any] = ["op": "create", "id": 7, "kind": "view", "handlers": ["press"]]
        let p = route([link, text(9, "History"), words, text(6, "Sync"), spinner, progress(8)], kids: [4, 5, 7],
                      more: [["op": "children", "id": 4, "ids": [9]], ["op": "children", "id": 5, "ids": [6]], ["op": "children", "id": 7, "ids": [8]]])
        let shape = try bar(p)
        XCTAssertEqual(shape.trailing.map(\.id), [4, 5, 7])
        XCTAssertEqual(shape.trailing.map(\.hosted), [false, true, true], "the link's one text is UIKit's; a pressed row is no button")
        XCTAssertEqual(shape.trailing.map(\.hostedGlass), [false, true, true], "each hosted row presses")
        XCTAssertEqual(shape.hosted.map(\.id), [5, 7])
        XCTAssertFalse(shape.placed.contains { [5, 7].contains($0.id) }, "hosted, not hidden with what the bar places")
        let route = RouteController(try XCTUnwrap(p.views[1]))
        XCTAssertEqual(NavigationHost.cover(of: try XCTUnwrap(p.views[5]), in: route), .bar)
    }

    /// A badge UIKit draws whole is its item; a spinner in the badge it
    /// cannot draw, so that button is hosted with its press. The Back
    /// control UIKit's back button stands for is never hosted, whatever
    /// its face.
    func testABadgeWithASpinnerIsHostedAndBackNever() throws {
        let fill: [String: Any] = ["background_color": [40, 100, 200, 255], "border_radius": 16]
        let p = route([["op": "create", "id": 4, "kind": "button", "handlers": ["press"], "props": ["accessibilityLabel": "Me"]],
                       ["op": "create", "id": 5, "kind": "view", "style": fill], text(6, "AB"),
                       ["op": "create", "id": 7, "kind": "button", "handlers": ["press"], "props": ["accessibilityLabel": "Busy"]],
                       ["op": "create", "id": 8, "kind": "view", "style": fill], text(9, "AB"), progress(10)],
                      kids: [4, 7], more: [["op": "children", "id": 4, "ids": [5]], ["op": "children", "id": 5, "ids": [6]],
                                           ["op": "children", "id": 7, "ids": [8]], ["op": "children", "id": 8, "ids": [9, 10]]])
        let shape = try bar(p)
        XCTAssertEqual(shape.trailing.map(\.id), [4, 7])
        XCTAssertEqual(shape.trailing.map(\.hosted), [false, true], "the badge drawn; the badge with a spinner hosted")
        XCTAssertNotNil(shape.trailing.first?.badge)
        // A pushed route whose authored Back control holds a spinner, a
        // sibling kept in the page: UIKit's back button stands for it, and
        // the bar hides it with what it places.
        let back = route([["op": "create", "id": 4, "kind": "button", "handlers": ["press"], "props": ["id": "back", "accessibilityLabel": "Back"]], progress(5),
                          text(6, "Kept", style: ["appearance": "none", "written": ["appearance"]])],
                         kids: [], more: [["op": "children", "id": 4, "ids": [5]]], header: [4, 3, 6])
        let pushed = try XCTUnwrap(HeaderShape(route: try XCTUnwrap(back.views[1]), back: "back", projected: true))
        XCTAssertTrue(pushed.hosted.isEmpty, "Back is UIKit's")
        XCTAssertTrue(pushed.leading.isEmpty)
        XCTAssertTrue(pushed.placed.contains { $0.id == 4 }, "hidden with what the bar places")
    }

    /// The title's ownership is the title presented: with segments in the
    /// title view, a glyph line after the heading is an item of its own. A
    /// text its author keeps in the page (`appearance: none`) is not the
    /// subtitle.
    func testTheTitleOwnsWhatItPresents() throws {
        let tab: (Int, String, Bool) -> [String: Any] = { id, words, on in
            ["op": "create", "id": id, "kind": "button", "handlers": ["press"], "props": ["accessibilityRole": "tab", "accessibilityLabel": words, "accessibilitySelected": on ? "true" : "false"]]
        }
        let p = route([["op": "create", "id": 4, "kind": "view"],
                       ["op": "create", "id": 5, "kind": "image", "props": ["imageSource": "symbol:sf/bell.slash", "symbolName": "bell.slash"]],
                       text(6, "Muted"),
                       ["op": "create", "id": 10, "kind": "view", "props": ["accessibilityRole": "tablist"]], tab(11, "All", true), text(13, "All"), tab(12, "Missed", false), text(14, "Missed")],
                      kids: [4, 10], more: [["op": "children", "id": 4, "ids": [5, 6]], ["op": "children", "id": 10, "ids": [11, 12]],
                                            ["op": "children", "id": 11, "ids": [13]], ["op": "children", "id": 12, "ids": [14]]])
        let segmented = try bar(p)
        XCTAssertNotNil(segmented.segments)
        XCTAssertEqual(segmented.hosted.map(\.id), [4], "the glyph line, which the segments' title cannot show")
        // A status in the glyph line has the row walked: beside segments its
        // glyphs are items of their own, lifted. When the segments go, the
        // title takes the line again, in the authored tree, and the glyphs
        // are the title's alone.
        let live = route([["op": "create", "id": 4, "kind": "view"],
                          ["op": "create", "id": 5, "kind": "image", "props": ["imageSource": "symbol:sf/bell.slash", "symbolName": "bell.slash"]],
                          text(6, "Muted"), text(7, "Live", ["accessibilityRole": "status"]),
                          ["op": "create", "id": 10, "kind": "view", "props": ["accessibilityRole": "tablist"]], tab(11, "All", true), text(13, "All"), tab(12, "Missed", false), text(14, "Missed")],
                         kids: [4, 10], more: [["op": "children", "id": 4, "ids": [5, 6, 7]], ["op": "children", "id": 10, "ids": [11, 12]],
                                               ["op": "children", "id": 11, "ids": [13]], ["op": "children", "id": 12, "ids": [14]]])
        let liveRoute = RouteController(try XCTUnwrap(live.views[1]))
        let tabbed = try bar(live)
        XCTAssertEqual(tabbed.hosted.map(\.id), [5, 6, 7])
        _ = live.navigationHost.barItems(tabbed.trailing, liveRoute)
        XCTAssertFalse(try XCTUnwrap(live.views[5]).isDescendant(of: try XCTUnwrap(live.views[4])), "lifted")
        live.apply(wireBatch([["op": "children", "id": 2, "ids": [3, 4]]]))
        let untabbed = try bar(live)
        XCTAssertEqual(untabbed.group?.glyphs.map(\.symbol), ["bell.slash", nil], "the title's glyph line")
        XCTAssertEqual(untabbed.hosted.map(\.id), [7], "the glyphs are the title's alone")
        XCTAssertEqual(untabbed.trailing.map(\.id), [7])
        // A status kept in the page inside the glyph line stays there: the
        // bar hides the glyphs the title draws, not the row that holds it.
        let optOut = route([["op": "create", "id": 4, "kind": "view"],
                            ["op": "create", "id": 5, "kind": "image", "props": ["imageSource": "symbol:sf/bell.slash", "symbolName": "bell.slash"]],
                            text(6, "Muted"), text(7, "Live", ["accessibilityRole": "status"], style: ["appearance": "none", "written": ["appearance"]])],
                           kids: [4], more: [["op": "children", "id": 4, "ids": [5, 6, 7]]])
        let partial = try bar(optOut)
        XCTAssertEqual(partial.authored.map(\.id), [7])
        XCTAssertEqual(partial.group?.subtitle, "Muted")
        XCTAssertTrue(partial.hosted.isEmpty)
        XCTAssertEqual(Set(partial.placed.map(\.id)).intersection([4, 5, 6, 7]), [5, 6], "the glyphs, not the row")
        let kept = route([text(5, "3 left", style: ["appearance": "none", "written": ["appearance"]])], kids: [5])
        let shape = try XCTUnwrap(HeaderShape(route: try XCTUnwrap(kept.views[1]), back: nil, projected: true))
        XCTAssertEqual(shape.group?.subtitle ?? "", "", "kept in the page, not the bar's subtitle")
        XCTAssertTrue(shape.hosted.isEmpty)
    }

    /// What UIKit's item cannot draw whole is hosted: a switch that hears
    /// `press`, a badge holding a symbol and a text, of which a badge draws
    /// one. A link is an item only with an `href` to follow: `role="link"`
    /// alone, or an `href` cleared to nothing, is a button that presses
    /// nothing.
    func testWhatTheBarCannotDrawWholeIsHostedAndALinkNeedsAnHref() throws {
        let fill: [String: Any] = ["background_color": [40, 100, 200, 255], "border_radius": 16]
        let p = route([["op": "create", "id": 4, "kind": "control", "handlers": ["press"], "props": ["type": "checkbox", "accessibilityRole": "switch", "accessibilityLabel": "Alerts"]],
                       ["op": "create", "id": 5, "kind": "button", "handlers": ["press"], "props": ["accessibilityLabel": "Me"]],
                       ["op": "create", "id": 6, "kind": "view", "style": fill],
                       ["op": "create", "id": 7, "kind": "image", "props": ["imageSource": "symbol:sf/person", "symbolName": "person"]], text(8, "AB"),
                       ["op": "create", "id": 9, "kind": "button", "props": ["accessibilityRole": "link", "accessibilityLabel": "Nowhere"]],
                       ["op": "create", "id": 10, "kind": "button", "props": ["href": "", "accessibilityLabel": "Cleared"]]],
                      kids: [4, 5, 9, 10], more: [["op": "children", "id": 5, "ids": [6]], ["op": "children", "id": 6, "ids": [7, 8]]])
        let shape = try bar(p)
        XCTAssertEqual(shape.trailing.map(\.id), [4, 5], "no link that follows nothing")
        XCTAssertEqual(shape.trailing.map(\.hosted), [true, true])
        // A pressed row of two texts, of which UIKit's title keeps one, is
        // hosted; a badge whose symbol is hidden as authored draws its text.
        let more = route([["op": "create", "id": 4, "kind": "view", "handlers": ["press"]], text(5, "Retry"), text(6, "later"),
                          ["op": "create", "id": 7, "kind": "button", "handlers": ["press"], "props": ["accessibilityLabel": "Me"]],
                          ["op": "create", "id": 8, "kind": "view", "style": fill],
                          ["op": "create", "id": 9, "kind": "image", "props": ["imageSource": "symbol:sf/person", "symbolName": "person"], "style": ["display": "none"]],
                          text(10, "AB")],
                         kids: [4, 7], more: [["op": "children", "id": 4, "ids": [5, 6]], ["op": "children", "id": 7, "ids": [8]],
                                              ["op": "children", "id": 8, "ids": [9, 10]]])
        let drawn = try bar(more)
        XCTAssertEqual(drawn.trailing.map(\.hosted), [true, false])
        XCTAssertEqual(drawn.trailing.last?.badge?.text, "AB")
        XCTAssertNil(drawn.trailing.last?.badge?.symbol, "the hidden symbol is not drawn")
        // A badge of a dot and a text draws one of them: hosted. A badge whose
        // one symbol `visibility` hides shows only its fill: drawn as written.
        // A symbol and words on a pressed row are UIKit's image item, which
        // speaks the words, as a hand-built bar's is.
        let dot: [String: Any] = ["background_color": [52, 199, 89, 255], "width": 7, "height": 7]
        let faces = route([["op": "create", "id": 4, "kind": "button", "handlers": ["press"], "props": ["accessibilityLabel": "Live"]],
                           ["op": "create", "id": 5, "kind": "view", "style": fill], ["op": "create", "id": 6, "kind": "view", "style": dot], text(7, "AB"),
                           ["op": "create", "id": 8, "kind": "button", "handlers": ["press"], "props": ["accessibilityLabel": "Me"]],
                           ["op": "create", "id": 9, "kind": "view", "style": fill],
                           ["op": "create", "id": 10, "kind": "image", "props": ["imageSource": "symbol:sf/person", "symbolName": "person"], "style": ["visibility": "hidden"]],
                           ["op": "create", "id": 11, "kind": "view", "handlers": ["press"], "props": ["accessibilityLabel": "Retry"]],
                           ["op": "create", "id": 12, "kind": "image", "props": ["imageSource": "symbol:sf/arrow.clockwise", "symbolName": "arrow.clockwise"]], text(13, "Retry"),
                           ["op": "create", "id": 14, "kind": "button", "handlers": ["press"], "props": ["accessibilityLabel": "Nested"]],
                           ["op": "create", "id": 15, "kind": "view", "style": fill], ["op": "create", "id": 16, "kind": "view"], text(17, "AB")],
                          kids: [4, 8, 14, 11], more: [["op": "children", "id": 4, "ids": [5]], ["op": "children", "id": 5, "ids": [6, 7]],
                                                       ["op": "children", "id": 8, "ids": [9]], ["op": "children", "id": 9, "ids": [10]],
                                                       ["op": "children", "id": 14, "ids": [15]], ["op": "children", "id": 15, "ids": [16]], ["op": "children", "id": 16, "ids": [17]],
                                                       ["op": "children", "id": 11, "ids": [12, 13]]])
        let painted = try bar(faces)
        // A badge whose part sits in a box of its own is no plain badge, and a
        // pressed row is no button: each is drawn as written.
        XCTAssertEqual(painted.trailing.map(\.hosted), [true, true, true, true])
        // A button of a symbol beside words is UIKit's image item, named by the
        // words; one of two texts, of a text in the author's font, or of a dot
        // beside words is drawn as written.
        let press: (Int) -> [String: Any] = { ["op": "create", "id": $0, "kind": "button", "handlers": ["press"]] }
        let plain = route([press(4), ["op": "create", "id": 5, "kind": "image", "props": ["imageSource": "symbol:sf/arrow.clockwise", "symbolName": "arrow.clockwise"]], text(6, "Retry"),
                           press(7), text(8, "Retry"), text(9, "later"),
                           press(10), text(11, "Mono", style: ["font_family": 3, "written": ["font_family"]]),
                           press(12), ["op": "create", "id": 13, "kind": "view", "style": dot], text(14, "Live"),
                           press(15), ["op": "create", "id": 16, "kind": "view"], text(17, "Live"),
                           press(18), text(19, "History", ["href": "/history"])],
                          kids: [4, 7, 10, 12, 15, 18], more: [["op": "children", "id": 4, "ids": [5, 6]], ["op": "children", "id": 7, "ids": [8, 9]],
                                                               ["op": "children", "id": 10, "ids": [11]], ["op": "children", "id": 12, "ids": [13, 14]],
                                                               ["op": "children", "id": 15, "ids": [16]], ["op": "children", "id": 16, "ids": [17]],
                                                               ["op": "children", "id": 18, "ids": [19]]])
        let plainShape = try bar(plain)
        // A box around the words, or words that link of their own, are drawn as written.
        XCTAssertEqual(plainShape.trailing.map(\.hosted), [false, true, true, true, true, true])
        XCTAssertEqual(plainShape.trailing.first?.symbol, "arrow.clockwise")
        XCTAssertEqual(plainShape.trailing.first?.title, "Retry", "spoken by the item")
        // A pressed row `visibility` hides is drawn as written, hidden; a box
        // that paints only a shadow beside words is drawn as written; a badge
        // beside a sibling hidden as authored is still the badge.
        let edges = route([["op": "create", "id": 4, "kind": "view", "handlers": ["press"], "style": ["visibility": "hidden"]], text(5, "Gone"),
                           ["op": "create", "id": 6, "kind": "view", "handlers": ["press"]],
                           ["op": "create", "id": 7, "kind": "view", "style": ["box_shadow": "0 0 2px #000", "width": 8, "height": 8]], text(8, "Live"),
                           ["op": "create", "id": 9, "kind": "button", "handlers": ["press"], "props": ["accessibilityLabel": "Me"]],
                           ["op": "create", "id": 10, "kind": "view", "style": fill], text(11, "AB"),
                           ["op": "create", "id": 12, "kind": "control", "props": ["type": "progress"], "style": ["display": "none"]]],
                          kids: [4, 6, 9], more: [["op": "children", "id": 4, "ids": [5]], ["op": "children", "id": 6, "ids": [7, 8]],
                                                  ["op": "children", "id": 9, "ids": [10, 12]], ["op": "children", "id": 10, "ids": [11]]])
        let edged = try bar(edges)
        XCTAssertEqual(edged.trailing.map(\.hosted), [true, true, false])
        XCTAssertEqual(edged.trailing.first?.hostedGlass, false, "what `visibility` hides presses nothing a person sees")
        XCTAssertEqual(edged.trailing.last?.badge?.text, "AB", "the hidden sibling is no part")
    }

    /// A plain face writes nothing UIKit's title would drop, and a badge's box
    /// is one fill: words in Markdown, an opacity on the words or the button,
    /// a coloured text beside a symbol, a material on a label, and a badge box
    /// with a material, a press, a border or a faded part are each hosted and
    /// drawn as written. A colour on the words is still plain,
    /// and the item's tint. A readout whose one action is a link Markdown's
    /// parser finds (a bare URL too, none in code) presses, so it has glass.
    func testAPlainFaceWritesOnlyColourAndABadgeIsOneFill() throws {
        let press: (Int, [String: Any]) -> [String: Any] = { ["op": "create", "id": $0, "kind": "button", "handlers": ["press"], "style": $1] }
        let words = route([press(4, [:]), text(5, "Live", style: ["text_color": [255, 0, 0, 255], "written": ["text_color"]]),
                           press(6, [:]), text(7, "Live", style: ["opacity": 0, "written": ["opacity"]]),
                           press(8, [:]), text(9, "[History](/history)", ["markup": "markdown"]),
                           press(10, ["opacity": 0.5, "written": ["opacity"]]), text(11, "Live"),
                           status(12), text(13, "See [History](/history)", ["markup": "markdown"]),
                           status(14), text(15, "See https://example.com", ["markup": "markdown"]),
                           status(16), text(17, "Type `[x](/go)`", ["markup": "markdown"]),
                           press(18, [:]), ["op": "create", "id": 19, "kind": "image", "props": ["imageSource": "symbol:sf/stop", "symbolName": "stop"]],
                           text(20, "Stop", style: ["text_color": [255, 0, 0, 255], "written": ["text_color"]]),
                           press(21, [:]), text(22, "AB", ["backgroundMaterial": "regular"])],
                          kids: [4, 6, 8, 10, 12, 14, 16, 18, 21], more: [["op": "children", "id": 4, "ids": [5]], ["op": "children", "id": 6, "ids": [7]],
                                                                          ["op": "children", "id": 8, "ids": [9]], ["op": "children", "id": 10, "ids": [11]],
                                                                          ["op": "children", "id": 12, "ids": [13]], ["op": "children", "id": 14, "ids": [15]],
                                                                          ["op": "children", "id": 16, "ids": [17]], ["op": "children", "id": 18, "ids": [19, 20]],
                                                                          ["op": "children", "id": 21, "ids": [22]]])
        let plain = try bar(words)
        // A coloured text beside a symbol, which UIKit would not show, and a
        // material on a label, are drawn as written.
        XCTAssertEqual(plain.trailing.map(\.hosted), [false, true, true, true, true, true, true, true, true])
        XCTAssertEqual(plain.trailing.first?.title, "Live")
        XCTAssertEqual(Array(plain.trailing.first?.tint?.first?.prefix(3) ?? []), [255, 0, 0], "the words' colour is the item's tint")
        XCTAssertEqual(plain.trailing[4...6].map(\.hostedGlass), [true, true, false], "a link presses; a link in code is words")
        let fill: [String: Any] = ["background_color": [40, 100, 200, 255], "border_radius": 16]
        let me: (Int) -> [String: Any] = { ["op": "create", "id": $0, "kind": "button", "handlers": ["press"], "props": ["accessibilityLabel": "Me"]] }
        let box: (Int, [String: Any], [String: Any]) -> [String: Any] = { ["op": "create", "id": $0, "kind": "view", "style": fill.merging($1) { $1 }, "props": $2] }
        let badges = route([me(4), box(5, [:], [:]), text(6, "AB"),
                            me(7), box(8, [:], ["backgroundMaterial": "regular"]), text(9, "AB"),
                            me(10), ["op": "create", "id": 11, "kind": "view", "style": fill, "handlers": ["press"]], text(12, "AB"),
                            me(13), box(14, ["border_width_top": 1, "border_width_right": 1, "border_width_bottom": 1, "border_width_left": 1, "border_color_top": [0, 0, 0, 255]], [:]), text(15, "AB"),
                            me(16), box(17, [:], [:]), text(18, "AB", style: ["opacity": 0.5, "written": ["opacity"]]),
                            ["op": "create", "id": 19, "kind": "button", "handlers": ["press"], "props": ["accessibilityLabel": "Me"], "style": ["opacity": 0, "written": ["opacity"]]],
                            box(20, [:], [:]), text(21, "AB"),
                            me(22), box(23, [:], [:]), text(24, "AB", ["backgroundMaterial": "regular"]),
                            me(25), box(26, ["opacity": 0.5, "written": ["opacity"]], [:]), text(27, "AB")],
                           kids: [4, 7, 10, 13, 16, 19, 22, 25], more: [["op": "children", "id": 4, "ids": [5]], ["op": "children", "id": 5, "ids": [6]],
                                                            ["op": "children", "id": 7, "ids": [8]], ["op": "children", "id": 8, "ids": [9]],
                                                            ["op": "children", "id": 10, "ids": [11]], ["op": "children", "id": 11, "ids": [12]],
                                                            ["op": "children", "id": 13, "ids": [14]], ["op": "children", "id": 14, "ids": [15]],
                                                            ["op": "children", "id": 16, "ids": [17]], ["op": "children", "id": 17, "ids": [18]],
                                                            ["op": "children", "id": 19, "ids": [20]], ["op": "children", "id": 20, "ids": [21]],
                                                            ["op": "children", "id": 22, "ids": [23]], ["op": "children", "id": 23, "ids": [24]],
                                                            ["op": "children", "id": 25, "ids": [26]], ["op": "children", "id": 26, "ids": [27]]])
        let drawn = try bar(badges)
        XCTAssertEqual(drawn.trailing.map(\.hosted), [false, true, true, true, true, true, true, true])
        XCTAssertEqual(drawn.trailing.first?.badge?.text, "AB")
    }

    /// A title wrapper that opts out after the bar lifted its readout keeps
    /// the readout in the page with it: the opt-out is read in the authored
    /// tree.
    func testAWrapperThatOptsOutKeepsItsLiftedReadout() throws {
        let p = route([["op": "create", "id": 8, "kind": "view", "handlers": ["press"]], status(6), text(7, "Live")],
                      kids: [], more: [["op": "children", "id": 6, "ids": [7]], ["op": "children", "id": 8, "ids": [3, 6]]], header: [8])
        let route = RouteController(try XCTUnwrap(p.views[1]))
        let first = try bar(p)
        XCTAssertEqual(first.hosted.map(\.id), [6], "the readout, hosted")
        XCTAssertEqual(first.group?.tap, 8)
        _ = p.navigationHost.barItems(first.trailing, route)
        XCTAssertFalse(try XCTUnwrap(p.views[6]).isDescendant(of: try XCTUnwrap(p.views[8])), "lifted")
        p.apply(wireBatch([["op": "style", "id": 8, "style": ["appearance": "none", "written": ["appearance"]]]]))
        let second = try bar(p)
        XCTAssertEqual(second.authored.map(\.id), [8])
        XCTAssertTrue(second.hosted.isEmpty, "kept in the page with its wrapper")
        XCTAssertTrue(second.trailing.isEmpty)
    }

    /// The item holds the element's own view, lifted out of the header; the
    /// next projection finds the element at its place, and a bar that stops
    /// showing it puts it back there.
    func testAHostedItemHoldsTheElementsOwnView() throws {
        let p = route([progress(4), text(5, "3 left")], kids: [4, 5])
        let route = RouteController(try XCTUnwrap(p.views[1]))
        let element = try XCTUnwrap(p.views[4])
        let first = try bar(p)
        let item = try XCTUnwrap(p.navigationHost.barItems(first.trailing, route).first)
        let view = try XCTUnwrap(item.customView as? HostedItemView)
        XCTAssertTrue(view.node === element)
        XCTAssertTrue(element.superview === view.stand, "lifted into the item")
        XCTAssertTrue(p.navigation.lifted?.parent(of: element) === p.views[2], "the header is still its parent")
        XCTAssertEqual(try bar(p).trailing.map(\.id), [4], "found where it was")
        XCTAssertTrue(p.navigationHost.barItems(try bar(p).trailing, route).first === item, "the same item")
        p.navigationHost.barHosted.show([], in: route)
        XCTAssertTrue(element.superview === p.views[2]?.container, "back in the header")
        XCTAssertEqual(p.views[2]?.container.subviews.compactMap { ($0 as? NodeView)?.id }, [3, 4, 5], "at its place")
    }

    /// A readout in the heading's box, lifted into its item, leaves the title as
    /// the first projection made it: the box holds a bar item, so it is no
    /// wrapper and the filled box in it no avatar, before the lift and after.
    func testALiftedReadoutInTheHeadingsBoxLeavesTheTitleAsItWas() throws {
        let p = route([["op": "create", "id": 8, "kind": "view"],
                       ["op": "create", "id": 4, "kind": "view", "style": ["background_color": [40, 100, 200, 255], "border_radius": 16]],
                       text(5, "AB"), status(6), text(7, "Live")],
                      kids: [], more: [["op": "children", "id": 4, "ids": [5]], ["op": "children", "id": 6, "ids": [7]],
                                       ["op": "children", "id": 8, "ids": [4, 3, 6]]], header: [8])
        let route = RouteController(try XCTUnwrap(p.views[1]))
        let first = try bar(p)
        XCTAssertEqual(first.trailing.map(\.id), [6], "the readout, hosted")
        XCTAssertNil(first.group?.avatar)
        _ = p.navigationHost.barItems(first.trailing, route)
        XCTAssertFalse(try XCTUnwrap(p.views[6]).isDescendant(of: try XCTUnwrap(p.views[8])), "lifted")
        let second = try bar(p)
        XCTAssertNil(second.group?.avatar, "still no avatar")
        XCTAssertEqual(second.trailing.map(\.id), [6])
    }

    /// The title reads a lifted element at its place: a second text, hosted
    /// while the first is the subtitle, is the subtitle once the first goes,
    /// and the bar puts it back in the header.
    func testTheTitleTakesALiftedElementWhenTheSubtitleBeforeItGoes() throws {
        let p = route([text(5, "3 left"), text(6, "synced")], kids: [5, 6])
        let route = RouteController(try XCTUnwrap(p.views[1]))
        let first = try bar(p)
        XCTAssertEqual(first.group?.subtitle, "3 left")
        XCTAssertEqual(first.trailing.map(\.id), [6])
        _ = p.navigationHost.barItems(first.trailing, route)
        let synced = try XCTUnwrap(p.views[6])
        XCTAssertFalse(synced.superview === p.views[2]?.container, "lifted")
        p.apply(wireBatch([["op": "children", "id": 2, "ids": [3, 6]]]))
        let second = try bar(p)
        XCTAssertEqual(second.group?.subtitle, "synced")
        XCTAssertTrue(second.trailing.isEmpty)
        p.navigationHost.barHosted.show(second.hosted.map(\.id), in: route)
        XCTAssertTrue(synced.superview === p.views[2]?.container, "back in the header")
    }

    /// A bar item's own colour that is the tint is UIKit's tint, which follows the bar's and dims
    /// with it; a share of the tint (`Highlight`) is a colour of the button's tint, undimmed while
    /// UIKit dims tints: the item keeps the colour it is given
    func testABarItemsTintColourIsUIKitsTintOrMadeFromItsButtons() throws {
        let p = route([["op": "create", "id": 4, "kind": "button", "handlers": ["press"], "props": ["accessibilityLabel": "Edit"],
                        "style": ["text_color": ["sys": "@tint", "c": [0, 122, 255, 255]], "written": ["text_color"]]],
                       text(5, "Edit"),
                       ["op": "create", "id": 6, "kind": "button", "handlers": ["press"], "props": ["accessibilityLabel": "Mark"],
                        "style": ["text_color": ["sys": "@tint/0.2", "c": [0, 122, 255, 51]], "written": ["text_color"]]],
                       text(7, "Mark")], kids: [4, 6], more: [["op": "children", "id": 4, "ids": [5]], ["op": "children", "id": 6, "ids": [7]]])
        window.tintColor = UIColor(red: 1, green: 0, blue: 0, alpha: 1)
        func item(_ id: UInt32) throws -> HeaderShape.Item {
            let shape = try XCTUnwrap(HeaderShape(route: try XCTUnwrap(p.views[1]), back: nil, projected: true))
            return try XCTUnwrap(shape.trailing.first { $0.id == id }, "the button is a bar item: \(shape.trailing.map(\.id))")
        }
        XCTAssertTrue(try item(4).followsTint, "the tint itself: UIKit's")
        XCTAssertNil(try item(4).tint)
        func share() throws -> [Double] { try XCTUnwrap(item(6).tint?.first, "a share of the tint is a colour of its own") }
        for (got, want) in zip(try share(), [255.0, 0, 0, 51]) { XCTAssertEqual(got, want, accuracy: 2, "a fifth of the button's red tint") }
        p.viewport.tintAdjustmentMode = .dimmed
        defer { p.viewport.tintAdjustmentMode = .automatic }
        for (got, want) in zip(try share(), [255.0, 0, 0, 51]) { XCTAssertEqual(got, want, accuracy: 2, "a fifth of the red tint, undimmed, while dimmed") }
    }

    private func spin(_ seconds: Double) { RunLoop.main.run(until: Date().addingTimeInterval(seconds)) }

    private func until(_ what: String, _ seconds: Double = 5, _ done: () -> Bool) {
        let deadline = Date().addingTimeInterval(seconds)
        while !done(), Date() < deadline { RunLoop.main.run(mode: .default, before: Date().addingTimeInterval(0.02)) }
        XCTAssertTrue(done(), what)
    }

    /// `bar-hosted.contract` booted in a window of a phone's size, its first route shown.
    private func session() throws -> ExactSession {
        let plan = try fixturePlan("BAR_HOSTED")
        let session = ExactApp.shared.makeSession(label: "bar-hosted-\(UUID().uuidString)")
        sessions.append(session)
        let view = ExactView(session: session), host = UIViewController()
        let scene = UIApplication.shared.connectedScenes.first as? UIWindowScene
        window = scene.map { UIWindow(windowScene: $0) } ?? UIWindow(frame: CGRect(x: 0, y: 0, width: 402, height: 874))
        window.frame = CGRect(x: 0, y: 0, width: 402, height: 874)
        window.rootViewController = host
        window.makeKeyAndVisible()
        host.view.addSubview(view)
        XCTAssertNil(session.boot(plan: plan, size: CGSize(width: 402, height: 874)).error)
        view.frame = host.view.bounds
        view.layoutIfNeeded()
        until("the first route") { session.presenter.navigationHost.primaryNavigation?.topViewController is RouteController }
        return session
    }

    private func node(_ testId: String, in p: Presenter) throws -> NodeView {
        try XCTUnwrap(p.views.values.first { $0.props["testId"] == testId }, testId)
    }

    private func item(_ testId: String, on c: UIViewController) throws -> UIBarButtonItem {
        let items = (c.navigationItem.leftBarButtonItems ?? []) + (c.navigationItem.rightBarButtonItems ?? [])
        return try XCTUnwrap(items.first { $0.accessibilityIdentifier == testId }, testId)
    }

    private func tap(_ point: CGPoint, with touch: TouchRunner) throws {
        var request = try TouchRunner.aim(point, in: window)
        request["op"] = "tap"
        XCTAssertEqual(try touch.ask(request)["done"] as? Bool, true)
    }

    func testATapOnAHostedReadoutPressesItsHeader() throws {
        guard ProcessInfo.processInfo.environment["EXACT_TOUCH_PORT"] != nil else {
            throw missing("touch", "a touch in the bar reaches the header through UIKit")
        }
        let touch = try TouchRunner(self)
        defer { touch.connection.cancel() }
        let session = try session(), p = session.presenter
        let words = try node("offline-text", in: p), presses = try node("header-presses", in: p)
        waitFor("the readout's words are laid out in the bar") { words.window != nil && !words.bounds.isEmpty }
        XCTAssertEqual(presses.accessibleText, "0")
        try tap(words.convert(CGPoint(x: words.bounds.midX, y: words.bounds.midY), to: window), with: touch)
        waitFor("the hosted readout's tap pressed the header", cap: 5) { presses.accessibleText == "1" }
    }

    func testMoreShowsAndFlipsASwitchChangedByTheApp() throws {
        guard ProcessInfo.processInfo.environment["EXACT_TOUCH_PORT"] != nil else {
            throw missing("touch", "open UIKit's More menu and choose its entry")
        }
        let touch = try TouchRunner(self)
        defer { touch.connection.cancel() }
        let session = try session(), p = session.presenter
        let nav = try XCTUnwrap(p.navigationHost.primaryNavigation)
        let home = try XCTUnwrap(nav.topViewController)
        let alerts = try item("alerts-switch", on: home)
        let switchNode = try node("alerts-switch", in: p), value = try node("alerts", in: p), noted = try node("noted", in: p)
        waitForActivation(session)
        XCTAssertEqual((alerts.menuRepresentation as? UIAction)?.state, .off)
        // Check the offer in the action's batch, before another layout can refresh it.
        let batch = session.runtime.press(try node("enable-alerts", in: p).id, now: session.now())
        XCTAssertNil(batch.error)
        p.apply(batch)
        XCTAssertEqual(value.accessibleText, "on")
        XCTAssertEqual((p.controls.controls[switchNode.id] as? UISwitch)?.isOn, true)
        XCTAssertEqual((alerts.menuRepresentation as? UIAction)?.state, .on, "the action's batch updates the More entry")
        XCTAssertEqual(noted.accessibleText, "0")

        // A center group in a navigator bar goes into UIKit's More menu.
        let navigationItem = home.navigationItem
        UIView.performWithoutAnimation {
            navigationItem.style = .navigator
            navigationItem.rightBarButtonItems = nil
            navigationItem.centerItemGroups = [UIBarButtonItemGroup(barButtonItems: [alerts], representativeItem: nil)]
            nav.navigationBar.layoutIfNeeded()
        }
        var morePoint: CGPoint?
        waitFor("UIKit's More button is hit-testable") {
            guard let frame = navigationItem.overflowPresentationSource?.frame(in: self.window), !frame.isEmpty else { return false }
            let point = CGPoint(x: frame.midX, y: frame.midY)
            guard let hit = self.window.hitTest(point, with: nil),
                  sequence(first: hit, next: \.superview).prefix(while: { $0 !== nav.navigationBar }).contains(where: { $0 is UIControl }) else { return false }
            morePoint = point
            return true
        }
        let point = try XCTUnwrap(morePoint)
        let bar = try XCTUnwrap(touch.ask(["op": "tree"])["elements"] as? [[String: Any]])
        XCTAssertTrue(bar.contains { $0["label"] as? String == "More" })
        var entry: [String: Any]?, labels: [String] = [], snapshotError: Error?
        func menuShowsAlerts() -> Bool {
            if entry == nil && snapshotError == nil {
                do {
                    let elements = try XCTUnwrap(touch.ask(["op": "tree"])["elements"] as? [[String: Any]])
                    labels = elements.compactMap { $0["label"] as? String }
                    entry = elements.first { $0["label"] as? String == "Alerts" }
                } catch { snapshotError = error }
            }
            return entry != nil || snapshotError != nil
        }
        try tap(point, with: touch)
        waitFor("More opens", cap: 2, onFailure: { _ in }, menuShowsAlerts)
        // UIKit can drop the first gesture after the previous test's window closes.
        if entry == nil && snapshotError == nil {
            try tap(point, with: touch)
            waitFor("More contains Alerts", cap: 6, facts: { "labels: \(labels)" }, menuShowsAlerts)
        }
        if let snapshotError { throw snapshotError }
        let shown = try XCTUnwrap(entry)
        XCTAssertEqual(shown["selected"] as? Bool, true, "More's Alerts entry is checked")
        let frame = try XCTUnwrap(shown["frame"] as? [Double])
        try tap(CGPoint(x: frame[0] + frame[2] / 2, y: frame[1] + frame[3] / 2), with: touch)
        waitFor("More flipped the switch and dispatched its press", cap: 5) { value.accessibleText == "off" && noted.accessibleText == "1" }
    }

    private func stops(in object: NSObject) -> [NSObject] {
        if let view = object as? UIView, view.isHidden || view.accessibilityElementsHidden { return [] }
        if object.isAccessibilityElement { return [object] }
        let children = object.accessibilityElements ?? (object as? UIView)?.subviews ?? []
        return children.compactMap { $0 as? NSObject }.flatMap { stops(in: $0) }
    }

    func testHostedReadoutsSpeakTheirLabelOnceAndKeepTheirControls() throws {
        guard UIApplication.shared.connectedScenes.first is UIWindowScene else {
            throw missing("scene", "UIKit lays out the bar in the app window")
        }
        let session = try session(), p = session.presenter
        let nav = try XCTUnwrap(p.navigationHost.primaryNavigation)
        let home = try XCTUnwrap(nav.topViewController)
        let view = try XCTUnwrap(item("offline", on: home).customView)
        let ready = try XCTUnwrap(item("sync-status", on: home).customView)
        waitFor("both readouts are laid out in the bar") {
            [view, ready].allSatisfy { $0.isDescendant(of: nav.navigationBar) && !$0.bounds.isEmpty }
        }
        nav.navigationBar.layoutIfNeeded()
        let passive = stops(in: ready)
        XCTAssertEqual(passive.map(\.accessibilityLabel), ["Ready"])
        XCTAssertEqual(passive.map(\.accessibilityTraits), [.staticText])
        let readout = stops(in: view)
        XCTAssertEqual(readout.map(\.accessibilityLabel), ["Offline, retried 0", "Retry", "Details"])
        let element = try XCTUnwrap(readout.first)
        XCTAssertEqual(element.accessibilityTraits, .staticText)
        XCTAssertEqual(element.accessibilityFrame, UIAccessibility.convertToScreenCoordinates(view.bounds, in: view))
        XCTAssertTrue(readout.dropFirst().allSatisfy { $0.accessibilityTraits.contains(.button) })
        p.press(try node("retry", in: p).id)
        waitFor("the accessibility element's label follows the readout", cap: 5) { element.accessibilityLabel == "Offline, retried 1" }
    }

    func testAHostedReadoutAnnouncesAnUpdateOnce() throws {
        guard UIApplication.shared.connectedScenes.first is UIWindowScene else {
            throw missing("scene", "the readout announces from the bar in the app window")
        }
        let session = try session(), p = session.presenter
        let nav = try XCTUnwrap(p.navigationHost.primaryNavigation)
        let home = try XCTUnwrap(nav.topViewController)
        let view = try XCTUnwrap(item("sync-status", on: home).customView)
        waitFor("the readout is laid out in the bar") { view.isDescendant(of: nav.navigationBar) && view.window != nil && !view.bounds.isEmpty }
        waitForActivation(session)

        var announcements: [NSAttributedString] = []
        let post = Presenter.postAnnouncement
        Presenter.postAnnouncement = { announcements.append($0) }
        defer { Presenter.postAnnouncement = post }
        p.press(try node("retry", in: p).id)
        waitFor("the readout displays the updated words", cap: 5) { self.stops(in: view).first?.accessibilityLabel == "Retrying" }
        waitForMainQueue()
        XCTAssertEqual(announcements.map(\.string), ["Retrying"])
        XCTAssertEqual(announcements.first?.attribute(.accessibilitySpeechQueueAnnouncement, at: 0, effectiveRange: nil) as? Bool, true)
        p.syncAccessibility()
        XCTAssertEqual(announcements.count, 1)
    }

    /// What a link item and a hosted item do (LLP 1141.004 D1, D3): a link
    /// is UIKit's item and follows its `href` under the agent's tap and under
    /// a tap on the item; in UIKit's overflow menu a hosted switch flips, its
    /// entry showing the switch's state, a row or a readout is its name,
    /// dimmed, and an entry acts only while the bar that shows still offers
    /// it.
    func testALinkItemAndAHostedItemDoWhatTheElementDoes() throws {
        guard UIApplication.shared.connectedScenes.first is UIWindowScene else { throw missing("scene", "a switch sends its actions through the app, which a hosted test has") }
        let session = try session(), p = session.presenter
        func node(_ testId: String) throws -> NodeView { try self.node(testId, in: p) }
        let nav = try XCTUnwrap(p.navigationHost.primaryNavigation)
        let home = try XCTUnwrap(nav.topViewController as? RouteController)
        let row = try item("live-row", on: home), offline = try item("offline", on: home)
        XCTAssertTrue(row.customView is HostedItemView && offline.customView is HostedItemView)
        // A row and a readout do nothing of their own: each is its name,
        // dimmed. What is inside them is reached in the bar.
        for (hosted, name) in [(row, "the row"), (offline, "the readout")] {
            let entry = try XCTUnwrap(hosted.menuRepresentation as? UIAction, name)
            XCTAssertTrue(entry.attributes.contains(.disabled), name)
            XCTAssertFalse(entry.title.isEmpty, name)
        }
        XCTAssertEqual((offline.menuRepresentation as? UIAction)?.title, "Offline, retried 0")
        p.press(try node("retry").id)
        until("Retry, in the bar, presses") { (try? node("retried"))?.accessibleText == "1" }
        // A switch that hears `press` is hosted, not a blank button, and its
        // entry shows and flips its state.
        let alerts = try item("alerts-switch", on: home)
        XCTAssertTrue(alerts.customView is HostedItemView)
        let alert = try XCTUnwrap(alerts.menuRepresentation as? UIAction)
        XCTAssertEqual(alert.title, "Alerts")
        XCTAssertEqual(alert.state, .off)
        XCTAssertFalse(alert.attributes.contains(.disabled))
        alert.performWithSender(nil, target: nil)
        until("the alerts switch flipped and pressed") { (try? node("alerts"))?.accessibleText == "on" && (try? node("noted"))?.accessibleText == "1" }
        until("its entry follows") { (alerts.menuRepresentation as? UIAction)?.state == .on }
        // The entry offered before that change does nothing now.
        alert.performWithSender(nil, target: nil)
        spin(0.3)
        XCTAssertEqual(try node("alerts").accessibleText, "on", "not flipped back by a stale entry")
        XCTAssertEqual(try node("noted").accessibleText, "1")
        // The readout's command opens the dialog written in it, as a tap on it
        // in the page does.
        p.press(try node("details-button").id)
        let details = try node("details")
        until("Details opens its dialog") { p.menus.isOpen(details) }
        p.press(try node("close-details").id)
        until("and it closes") { !p.menus.isOpen(details) && !p.menus.inTransition }
        // An entry from a menu made while Home showed does nothing once
        // another route's bar shows.
        let later = try XCTUnwrap(alerts.menuRepresentation as? UIAction)
        let link = try node("history-link")
        XCTAssertNil(try item("history-link", on: home).customView, "UIKit's own item")
        let reply = p.navigationHost.activateChrome(link)
        XCTAssertNil(reply?["error"], "\(reply ?? [:])")
        until("the agent's tap followed the link") { nav.viewControllers.count == 2 && nav.transitionCoordinator == nil }
        later.performWithSender(nil, target: nil)
        spin(0.3)
        XCTAssertEqual(try node("noted").accessibleText, "1", "no press from Home's bar under History's")
        let history = try XCTUnwrap(nav.topViewController)
        // A native button's words' colour is its item's tint; a filled
        // button of one compiled text is UIKit's own item.
        let red = try XCTUnwrap(item("stop", on: history).tintColor, "the words' colour").resolvedColor(with: history.traitCollection)
        var (r, g, b, a) = (CGFloat(0), CGFloat(0), CGFloat(0), CGFloat(0))
        red.getRed(&r, green: &g, blue: &b, alpha: &a)
        XCTAssertTrue(r > 0.8 && g < 0.4 && b < 0.4, "system red, not the bar's tint: \(red)")
        XCTAssertNil(try item("note-button", on: history).customView, "UIKit's own item")
        try XCTUnwrap(item("about-link", on: history).target as? BarPress).press()
        until("a tap on the item followed the link") { nav.viewControllers.count == 3 }
    }
}
#endif
