#if os(iOS)
import UIKit
import XCTest
@testable import ExactKit
@testable import ExactNavigation

/// A header's hosted items (LLP 1141.004) over hand-built routes: what is one,
/// what the title takes of them, when one shares the bar's glass, and the
/// element lifted into its item and back. These presenters have no kernel, so
/// nothing is laid out; the native fixture's readout, pushed and laid out, is
/// NavigationBasicsIOSTests'.
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
        XCTAssertEqual(live.hostedInset, HeaderShape.Item.titleInset)
        let sided = try item([text(6, "Live"), toggle], [6, 7], style: ["padding_left": 4, "written": ["padding_left"]])
        XCTAssertEqual(sided.hostedInset, 0, "the author's side")
        let own = try item([text(6, "Live"), toggle], [6, 7], props: ["backgroundMaterial": "glass"])
        XCTAssertFalse(own.hostedGlass, "its own material, drawn by it")
        let button: [String: Any] = ["op": "create", "id": 7, "kind": "button", "handlers": ["press"], "props": ["accessibilityLabel": "Retry"]]
        let p = route([text(8, "Inbox"), ["op": "create", "id": 4, "kind": "view"], text(6, "Offline"), button], kids: [8, 4], more: [["op": "children", "id": 4, "ids": [6, 7]]])
        XCTAssertEqual(try bar(p).trailing.map(\.id), [6, 7], "after the subtitle, the text hosted and the button an item")
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

}
#endif
