#if os(iOS)
import UIKit
import XCTest
@testable import ExactKit
@testable import ExactGroupedLists

/// LLP 1084 D3 (expo/exact#836): the buttons after a grouped section's
/// header or footer text are the author's own controls, carried into UIKit's
/// label at its trailing edge. A press reaches each, VoiceOver finds each
/// after the label's words, a batch that changes one leaves it there, and
/// the agent's tap reveals a label off screen.
final class GroupedLabelButtonsIOSTests: XCTestCase {
    func testALabelsButtonsStandAtItsTrailingEdgeAndPress() throws {
        let path = try XCTUnwrap(ProcessInfo.processInfo.environment["EXACT_GROUPED_LABEL_BUTTONS_PLAN"])
        ExactGroupedLists.install()
        let session = ExactApp.shared.makeSession(label: "grouped-label-buttons")
        defer { session.destroy() }
        let view = ExactView(session: session)
        guard let scene = UIApplication.shared.connectedScenes.first as? UIWindowScene else {
            throw missing("scene", "the list needs a hosted window scene")
        }
        let window = UIWindow(windowScene: scene)
        window.frame = window.screen.bounds
        let root = UIViewController()
        window.rootViewController = root
        window.makeKeyAndVisible()
        root.view.addSubview(view)
        view.frame = root.view.bounds
        defer { window.isHidden = true }
        XCTAssertNil(session.boot(plan: try Data(contentsOf: URL(fileURLWithPath: path)), size: view.bounds.size).error)
        let p = session.presenter, agent = Agent(session: session)
        func settle() {
            window.layoutIfNeeded()
            let reply = agent.clock(["settle": true])
            XCTAssertEqual(reply["settled"] as? Bool, true, "\(reply)")
        }
        func node(_ name: String) throws -> NodeView {
            try XCTUnwrap(p.views.values.first { $0.props["testId"] == name }, name)
        }
        func tap(_ name: String) throws {
            let id = Int(try node(name).id)
            let reveal = agent.reveal(["id": id])
            XCTAssertNil(reveal["error"], "\(reveal)")
            let reply = agent.tap(["id": id])
            XCTAssertNil(reply["error"], "\(reply)")
            settle()
        }
        func slot(_ name: String) throws -> Any? {
            let state = try XCTUnwrap(try JSONSerialization.jsonObject(with: Data(session.agent(#"{"op":"state"}"#).utf8)) as? [String: Any])
            return (state["slots"] as? [String: Any])?[name]
        }
        settle()
        let collection = try XCTUnwrap(p.groupedLists?.scroller(for: try node("list").id) as? UICollectionView)
        func label(_ kind: String, _ section: Int) throws -> GroupedLabelCell {
            collection.layoutIfNeeded()
            return try XCTUnwrap(collection.supplementaryView(forElementKind: kind, at: IndexPath(item: 0, section: section)) as? GroupedLabelCell)
        }
        let header = try label(UICollectionView.elementKindSectionHeader, 0)
        let edit = try node("edit"), delete = try node("delete")
        XCTAssertEqual((header.contentConfiguration as? UIListContentConfiguration)?.attributedText?.string, "Série 1")
        let e = edit.convert(edit.bounds, to: header), d = delete.convert(delete.bounds, to: header)
        XCTAssertTrue(edit.isDescendant(of: header) && delete.isDescendant(of: header), "carried into UIKit's header")
        XCTAssertLessThanOrEqual(e.maxX, d.minX, "in the order written")
        XCTAssertEqual(d.maxX, header.bounds.maxX - header.layoutMargins.right, accuracy: 0.5, "the last at the header's trailing margin")
        XCTAssertEqual(d.midY, header.bounds.midY, accuracy: 1, "centred in the header")
        let title = try XCTUnwrap(header.contentView.firstInTree { $0 as? UILabel })
        XCTAssertLessThanOrEqual(title.convert(title.bounds, to: header).maxX, e.minX, "the text ends before them")
        let middle = delete.convert(CGPoint(x: delete.bounds.midX, y: delete.bounds.midY), to: window)
        let hit = try XCTUnwrap(window.hitTest(middle, with: nil))
        XCTAssertTrue(hit === delete || hit.isDescendant(of: delete), "a touch on it reaches it: \(hit)")
        // UIKit's cell is one element, which would hide them from VoiceOver.
        XCTAssertFalse(header.isAccessibilityElement)
        let elements = try XCTUnwrap(header.accessibilityElements).map { $0 as AnyObject }
        XCTAssertEqual((elements.first as? NSObject)?.accessibilityLabel, "Série 1")
        XCTAssertEqual((elements.first as? NSObject)?.accessibilityTraits.contains(.header), true, "its words stay a heading")
        XCTAssertTrue(elements.dropFirst().elementsEqual([edit, delete]) { $0 === $1 }, "then each button: \(elements)")
        XCTAssertTrue(try label(UICollectionView.elementKindSectionHeader, 1).isAccessibilityElement, "a label of words alone is UIKit's one element")

        try tap("delete")
        XCTAssertEqual(try slot("sets") as? Int, 2, "its press ran")
        try tap("edit")
        XCTAssertEqual(try slot("editing") as? Bool, true)
        XCTAssertTrue(try node("edit").isDescendant(of: try label(UICollectionView.elementKindSectionHeader, 0)), "there after the batch that changed its words")
        let footer = try label(UICollectionView.elementKindSectionFooter, 0)
        XCTAssertTrue(try node("fewer").isDescendant(of: footer), "a footer's too")
        XCTAssertEqual((footer.contentConfiguration as? UIListContentConfiguration)?.attributedText?.string, "2 left")

        let far = try node("far")
        XCTAssertFalse(far.isDescendant(of: collection), "a label off screen holds no button")
        let before = collection.contentOffset.y
        try tap("far")
        XCTAssertEqual(try slot("sets") as? Int, 1, "the driver revealed its header and pressed it")
        XCTAssertGreaterThan(collection.contentOffset.y, before)
        XCTAssertTrue(far.isDescendant(of: try label(UICollectionView.elementKindSectionHeader, 2)))
        XCTAssertFalse(edit.isDescendant(of: collection), "the first header's buttons went home as it left")
    }
}
#endif
