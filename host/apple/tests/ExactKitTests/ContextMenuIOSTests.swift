#if os(iOS)
import UIKit
import XCTest
@testable import ExactKit
@testable import ExactNavigation

/// LLP 1021 §5.1 on UIKit: a node naming a popover with `contextPopover` is
/// a UIContextMenuInteraction. The node lifts as its own targeted preview,
/// the popover's `contextPreview` row is the preview controller's view
/// (sized by its layout), its menu rows the UIMenu, and tapping the preview
/// presses that row: `.pop` when the press navigates, pushed without the
/// stack's animation, `.dismiss` otherwise. The native fixture's `peek` row.
/// UIKit, so a simulator runs it: bun host/apple/build.mjs --test --ios
final class ContextMenuIOSTests: XCTestCase {
    private var window: UIWindow?
    private var session: ExactSession?

    override func tearDown() {
        session?.destroy()
        window?.isHidden = true
        window = nil
        session = nil
        super.tearDown()
    }

    /// UIKit's commit animator, as the delegate sees it.
    private final class Commit: NSObject, UIContextMenuInteractionCommitAnimating {
        var preferredCommitStyle: UIContextMenuInteractionCommitStyle = .pop
        var previewViewController: UIViewController? { nil }
        var animated = 0
        func addAnimations(_ animations: @escaping () -> Void) { animated += 1; animations() }
        func addCompletion(_ completion: @escaping () -> Void) { completion() }
    }

    private func boot() throws -> ExactSession {
        if ExactEnv.agentPaints { throw XCTSkip("the agent paints the popover (D4)") }
        let plan = try Data(contentsOf: URL(fileURLWithPath: try XCTUnwrap(ProcessInfo.processInfo.environment["EXACT_FIXTURE_PLAN"], "build.mjs --test --ios compiles the fixture's plan")))
        let session = ExactApp.shared.makeSession(label: "context-menu")
        self.session = session
        let view = ExactView(session: session), host = UIViewController()
        let scene = UIApplication.shared.connectedScenes.first as? UIWindowScene
        let window = scene.map { UIWindow(windowScene: $0) } ?? UIWindow(frame: CGRect(x: 0, y: 0, width: 402, height: 874))
        window.frame = CGRect(x: 0, y: 0, width: 402, height: 874)
        window.rootViewController = host
        window.makeKeyAndVisible()
        host.view.addSubview(view)
        self.window = window
        XCTAssertNil(session.boot(plan: plan, size: window.bounds.size).error)
        view.frame = host.view.bounds
        view.layoutIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.3))
        return session
    }
    private func node(_ session: ExactSession, _ testId: String) throws -> NodeView {
        try XCTUnwrap(session.presenter.views.values.first { $0.props["testId"] == testId }, testId)
    }
    private func text(_ node: NodeView) -> String { node.paragraphSpec().runs.map(\.text).joined() }
    private func until(_ what: String, _ seconds: Double = 3, _ done: () -> Bool) {
        let deadline = Date().addingTimeInterval(seconds)
        while !done(), Date() < deadline { RunLoop.main.run(mode: .default, before: Date().addingTimeInterval(0.02)) }
        XCTAssertTrue(done(), what)
    }
    /// `peek`'s parent made a strip as tall and wide as it that clips, as
    /// Landscapes' strips are, where UIKit's growing lift would be cut and the
    /// node lifts on a stage; the returned closure puts the parent back
    private func stripAround(_ peek: NodeView) throws -> () -> Void {
        let strip = try XCTUnwrap(peek.superview)
        let (frame, bounds, clips) = (strip.frame, strip.bounds, strip.clipsToBounds)
        strip.clipsToBounds = true
        strip.frame = strip.convert(peek.frame, to: strip.superview)
        strip.bounds.origin = peek.frame.origin
        return { strip.clipsToBounds = clips; strip.frame = frame; strip.bounds = bounds }
    }
    /// Where a target places a view's middle (its center moved by its transform's translation), or where a view's middle shows, in the window
    private func shown(_ target: UIPreviewTarget) -> CGPoint {
        target.container.convert(CGPoint(x: target.center.x + target.transform.tx, y: target.center.y + target.transform.ty), to: nil)
    }
    private func shown(_ view: UIView) -> CGPoint { view.convert(CGPoint(x: view.bounds.midX, y: view.bounds.midY), to: nil) }
    /// The same point, but for the rounding of converting it through the stage
    private func near(_ a: CGPoint, _ b: CGPoint) -> Bool { abs(a.x - b.x) < 1e-6 && abs(a.y - b.y) < 1e-6 }
    /// The view of the controller that holds `node`, where a node a box already cuts lifts
    private func holder(of node: NodeView) throws -> UIView {
        try XCTUnwrap(sequence(first: try XCTUnwrap(node.superview) as UIResponder, next: { $0.next }).lazy.compactMap { ($0 as? UIViewController)?.viewIfLoaded }.first)
    }

    /// In a strip that clips around it, a node lifts on its stage, which takes its parent's
    /// coordinate space with the ancestors' scale and fade over its
    /// controller's view: the target carries the node's own scale, and stands
    /// where the node shows, also where the strip already cuts it. Where no
    /// stage can carry it (its own turn, a perspective, a mask, a scale of its
    /// parent's children), it lifts as it would without one (`LiftTarget.target`).
    func testTheTargetCarriesTheAncestorsScale() throws {
        let session = try boot()
        let host = session.presenter.menus.context
        let peek = try node(session, "peek")
        let interaction = try XCTUnwrap(peek.interactions.compactMap { $0 as? UIContextMenuInteraction }.first)
        let configuration = try XCTUnwrap(host.contextMenuInteraction(interaction, configurationForMenuAtLocation: .zero))
        defer { host.contextMenuInteraction(interaction, willEndFor: configuration, animator: nil) }
        let parent = try XCTUnwrap(peek.superview)
        func lifted() throws -> UIPreviewTarget {
            try XCTUnwrap(host.contextMenuInteraction(interaction, configuration: configuration, highlightPreviewForItemWithIdentifier: configuration.identifier)).target
        }
        let undo = try stripAround(peek)
        defer { undo() }
        parent.transform = CGAffineTransform(scaleX: 0.5, y: 0.5)
        parent.alpha = 0.5
        peek.transform = CGAffineTransform(scaleX: 0.96, y: 0.96)
        defer { parent.transform = .identity; parent.alpha = 1; peek.transform = .identity }
        let inside = try lifted()
        let stage = try XCTUnwrap(inside.container as? LiftStage)
        XCTAssertTrue(stage.superview === (try holder(of: peek)), "over its controller's view")
        XCTAssertEqual(stage.transform, CGAffineTransform(scaleX: 0.5, y: 0.5), "its parent's scale")
        XCTAssertEqual(stage.layer.opacity, 0.5, accuracy: 0.001, "and its fade")
        parent.isHidden = true
        stage.follow()
        XCTAssertTrue(stage.isHidden, "hidden with it")
        parent.isHidden = false
        stage.follow()
        XCTAssertEqual(inside.transform, CGAffineTransform(scaleX: 0.96, y: 0.96), "its own scale")
        XCTAssertTrue(near(shown(inside), shown(peek)), "where it shows")
        peek.layer.anchorPoint = CGPoint(x: 0, y: 0.5)
        peek.transform = CGAffineTransform(rotationAngle: .pi / 3)
        XCTAssertTrue(near(shown(try lifted()), shown(peek)), "a card turned about its edge, from its middle")
        peek.layer.transform = CATransform3DMakeRotation(.pi / 3, 0, 1, 0)
        XCTAssertTrue(try lifted().container === parent, "turned in 3D itself: in its parent, as without a stage")
        peek.layer.anchorPoint = CGPoint(x: 0.5, y: 0.5)
        peek.transform = CGAffineTransform(scaleX: 0.96, y: 0.96)
        var perspective = CATransform3DIdentity
        perspective.m34 = -1 / 500
        parent.layer.sublayerTransform = perspective
        XCTAssertTrue(try lifted().container === parent, "under a perspective, no stage's plane carries it: in its parent")
        parent.layer.sublayerTransform = CATransform3DIdentity
        let mask = CALayer()
        mask.frame = parent.bounds
        mask.backgroundColor = UIColor.black.cgColor
        parent.layer.mask = mask
        XCTAssertTrue(try lifted().container === parent, "under a mask, whose shape no stage follows, in its parent")
        parent.layer.mask = nil
        parent.layer.sublayerTransform = CATransform3DMakeScale(2, 2, 1)
        XCTAssertTrue(try lifted().container === LiftTarget.target(peek)?.container, "under a scale of its parent's children, as without a stage")
        parent.layer.sublayerTransform = perspective
        parent.clipsToBounds = true
        peek.transform = CGAffineTransform(scaleX: 0.96, y: 0.96).translatedBy(x: parent.bounds.width, y: 0)
        XCTAssertTrue(try lifted().container === (try holder(of: peek)), "cut by it too, over the controller's view, as without a stage")
        parent.layer.sublayerTransform = CATransform3DIdentity
        let cut = try lifted()
        XCTAssertTrue(cut.container === stage, "cut by its parent: on the same stage, above it")
        XCTAssertTrue(near(shown(cut), shown(peek)), "from where it shows")
    }

    /// A card in a strip as tall as itself, which clips, grows uncut: its
    /// stage stands over the controller's view, outside every box that clips.
    /// While it holds what UIKit draws there, the stage follows the strip's
    /// scroll, so a press that turns into a scroll returns the card where the
    /// scroll has taken it; it outlasts the presentation until UIKit takes
    /// that back, as UIKit's return can.
    func testALiftGrowsUncutOverAStripThatClipsAndMovesWithItsScroll() throws {
        let session = try boot()
        let host = session.presenter.menus.context
        let peek = try node(session, "peek")
        let interaction = try XCTUnwrap(peek.interactions.compactMap { $0 as? UIContextMenuInteraction }.first)
        let configuration = try XCTUnwrap(host.contextMenuInteraction(interaction, configurationForMenuAtLocation: .zero))
        let strip = try XCTUnwrap(peek.superview)
        let (frame, bounds) = (strip.frame, strip.bounds)
        strip.clipsToBounds = true
        strip.frame = strip.convert(peek.frame, to: strip.superview)
        strip.bounds.origin = peek.frame.origin
        defer { strip.clipsToBounds = false; strip.frame = frame; strip.bounds = bounds }
        let target = try XCTUnwrap(host.contextMenuInteraction(interaction, configuration: configuration, highlightPreviewForItemWithIdentifier: configuration.identifier)).target
        let stage = try XCTUnwrap(target.container as? LiftStage)
        let holder = try holder(of: peek)
        XCTAssertFalse(stage.isDescendant(of: strip), "not in the strip")
        XCTAssertTrue(sequence(first: stage as UIView, next: { $0.superview }).prefix { $0 !== holder }.allSatisfy { !$0.clipsToBounds && $0.layer.mask == nil }, "no box that clips holds it")
        XCTAssertTrue(near(shown(target), shown(peek)), "where it shows")
        let highlight = UIView(frame: peek.frame)
        stage.addSubview(highlight)
        strip.bounds.origin.x += 40
        until("it moves with the strip's scroll") { self.near(self.shown(target), self.shown(peek)) }
        strip.bounds.origin.x += 1000
        until("it hides once the card has scrolled out of the strip") { stage.isHidden }
        strip.bounds.origin.x -= 1000
        until("and shows again as the card comes back") { !stage.isHidden }
        host.contextMenuInteraction(interaction, willEndFor: configuration, animator: nil)
        XCTAssertNotNil(stage.superview, "UIKit's return outlasts the presentation")
        highlight.removeFromSuperview()
        until("it goes once UIKit takes its view back") { stage.superview == nil }
    }

    /// An authored close lets the presentation's record go at once; UIKit's
    /// return still takes the node back onto its stage, also past a press that
    /// began meanwhile, and the stage stays until UIKit ends the presentation.
    func testAnAuthoredCloseKeepsTheStageUntilThePresentationEnds() throws {
        let session = try boot()
        let host = session.presenter.menus.context
        let peek = try node(session, "peek")
        let undo = try stripAround(peek)
        defer { undo() }
        let interaction = try XCTUnwrap(peek.interactions.compactMap { $0 as? UIContextMenuInteraction }.first)
        let configuration = try XCTUnwrap(host.contextMenuInteraction(interaction, configurationForMenuAtLocation: .zero))
        let lifted = try XCTUnwrap(host.contextMenuInteraction(interaction, configuration: configuration, highlightPreviewForItemWithIdentifier: configuration.identifier))
        let stage = try XCTUnwrap(lifted.target.container as? LiftStage)
        host.contextMenuInteraction(interaction, willDisplayMenuFor: configuration, animator: nil)
        // UIKit takes what it drew on the stage into a layer of its own while the menu shows: the stage stays for the return
        let drawn = UIView()
        stage.addSubview(drawn)
        drawn.removeFromSuperview()
        var turned = false
        DispatchQueue.main.async { turned = true }
        until("a turn passes") { turned }
        XCTAssertNotNil(stage.window, "held for the menu that showed")
        host.close(try XCTUnwrap(host.open?.popover))
        XCTAssertNil(host.open)
        let next = try XCTUnwrap(host.contextMenuInteraction(interaction, configurationForMenuAtLocation: .zero), "another press")
        defer { host.contextMenuInteraction(interaction, willEndFor: next, animator: nil) }
        XCTAssertNotNil(stage.window, "the stage stays for UIKit's return")
        let returned = try XCTUnwrap(host.contextMenuInteraction(interaction, configuration: configuration, dismissalPreviewForItemWithIdentifier: configuration.identifier),
                                     "UIKit's return asked for after the close")
        XCTAssertTrue(returned.view === peek && returned.target.container === stage && near(shown(returned.target), shown(peek)), "the node back onto its stage, where it shows")
        let incarnation = peek.incarnation
        peek.incarnation = 0
        XCTAssertNil(host.contextMenuInteraction(interaction, configuration: configuration, dismissalPreviewForItemWithIdentifier: configuration.identifier),
                     "a view lent to another node since returns nothing")
        peek.incarnation = incarnation
        host.contextMenuInteraction(interaction, willEndFor: configuration, animator: nil)
        XCTAssertNil(stage.superview, "and goes as the presentation ends, holding nothing")
    }

    /// Only a box that clips and would cut the node as UIKit grows it calls for
    /// a stage: with room to grow, the node lifts as it did without stages.
    func testOnlyAClipThatWouldCutTheGrowingLiftCallsForAStage() {
        let host = UIViewController(), window = UIWindow(frame: CGRect(x: 0, y: 0, width: 400, height: 400))
        window.rootViewController = host
        window.makeKeyAndVisible()
        defer { window.isHidden = true }
        let clip = UIView(frame: CGRect(x: 0, y: 0, width: 300, height: 200)), card = UIView(frame: CGRect(x: 100, y: 50, width: 100, height: 100))
        clip.clipsToBounds = true
        clip.addSubview(card)
        host.view.addSubview(clip)
        XCTAssertFalse(LiftStage.cut(card), "room to grow in its clip")
        card.frame = CGRect(x: 100, y: 0, width: 100, height: 200)
        XCTAssertTrue(LiftStage.cut(card), "a strip as tall as the card")
        clip.clipsToBounds = false
        XCTAssertFalse(LiftStage.cut(card), "nothing clips")
    }

    /// A stage whose parent has left the window, been freed with the strip a
    /// batch removed, or moved under another controller's view, hides what
    /// UIKit holds on it until UIKit takes that back.
    func testAStageWhoseParentHasGoneHides() throws {
        let host = UIViewController(), window = UIWindow(frame: CGRect(x: 0, y: 0, width: 300, height: 300))
        window.rootViewController = host
        window.makeKeyAndVisible()
        defer { window.isHidden = true }
        var made: LiftStage?
        autoreleasepool {
            let parent = UIView(frame: CGRect(x: 0, y: 0, width: 200, height: 100)), card = UIView(frame: CGRect(x: 10, y: 10, width: 50, height: 50))
            parent.addSubview(card)
            host.view.addSubview(parent)
            made = LiftStage(for: card)
            parent.removeFromSuperview()
        }
        let stage = try XCTUnwrap(made, "a stage over the controller's view")
        defer { stage.end() }
        stage.follow()
        XCTAssertTrue(stage.isHidden, "its parent gone, nothing of the card shows on it")
        let parent = UIView(frame: CGRect(x: 0, y: 0, width: 200, height: 100)), card = UIView(frame: CGRect(x: 10, y: 10, width: 50, height: 50))
        parent.addSubview(card)
        host.view.addSubview(parent)
        let moving = try XCTUnwrap(LiftStage(for: card))
        defer { moving.end() }
        moving.follow()
        XCTAssertFalse(moving.isHidden)
        let other = UIViewController()
        host.view.addSubview(other.view)
        other.view.addSubview(parent)
        moving.follow()
        XCTAssertTrue(moving.isHidden, "its parent under another controller's view, nothing of the card shows on it")
        // Torn down and freed while UIKit's view is still on it, as a reset can
        autoreleasepool {
            let torn = LiftStage(for: card)
            torn?.addSubview(UIView())
            torn?.end()
        }
    }

    func testTheLiftsOutlineIsTheNodesOwn() throws {
        let session = try boot()
        let host = session.presenter.menus.context
        let peek = try node(session, "peek")
        let interaction = try XCTUnwrap(peek.interactions.compactMap { $0 as? UIContextMenuInteraction }.first)
        func outline() throws -> CGPath? {
            let configuration = try XCTUnwrap(host.contextMenuInteraction(interaction, configurationForMenuAtLocation: .zero))
            defer { host.contextMenuInteraction(interaction, willEndFor: configuration, animator: nil) }
            return try XCTUnwrap(host.contextMenuInteraction(interaction, configuration: configuration, highlightPreviewForItemWithIdentifier: configuration.identifier)).parameters.visiblePath?.cgPath
        }
        func corners(_ radii: [Double]) {
            session.presenter.apply(wireBatch([["op": "style", "id": Int(peek.id), "style": [
                "border_radius_top_left": radii[0], "border_radius_top_right": radii[1],
                "border_radius_bottom_right": radii[2], "border_radius_bottom_left": radii[3]]]]))
        }
        corners([12, 12, 4, 4])
        XCTAssertEqual(try outline(), peek.roundedPath(in: peek.bounds).cgPath, "unequal radii: its own outline")
        corners([0, 0, 0, 0])
        XCTAssertNil(try outline(), "no radius: the whole view, as UIKit shows it by default")
    }

    func testTheLiftsGroundIsTheNodesLayerFill() throws {
        let session = try boot()
        let host = session.presenter.menus.context
        let peek = try node(session, "peek")
        let interaction = try XCTUnwrap(peek.interactions.compactMap { $0 as? UIContextMenuInteraction }.first)
        func ground(_ style: [String: Any]) throws -> UIColor? {
            session.presenter.apply(wireBatch([["op": "style", "id": Int(peek.id), "style": style]]))
            peek.layer.displayIfNeeded()
            let configuration = try XCTUnwrap(host.contextMenuInteraction(interaction, configurationForMenuAtLocation: .zero))
            defer { host.contextMenuInteraction(interaction, willEndFor: configuration, animator: nil) }
            return try XCTUnwrap(host.contextMenuInteraction(interaction, configuration: configuration, highlightPreviewForItemWithIdentifier: configuration.identifier)).parameters.backgroundColor
        }
        let unfilled = try ground([:])
        var red: CGFloat = 1, green: CGFloat = 1
        try XCTUnwrap(ground(["background_color": [0, 122, 255, 255]])).getRed(&red, green: &green, blue: nil, alpha: nil)
        XCTAssertEqual(red, 0, accuracy: 0.01, "a filled bubble's ground is its fill, not UIKit's white")
        XCTAssertEqual(green, 122.0 / 255, accuracy: 0.01)
        XCTAssertEqual(try ground(["background_color": [0, 122, 255, 255], "background_clip": "text"]), unfilled,
                       "a fill clipped to the text is drawn in the copy: UIKit's own ground")
    }

    /// UIKit hides the long-pressed node and grows its highlight on the
    /// node's stage, which the route's mount holds after the route, at depth 0.
    /// The positioned route ranks above 0, so the stage takes the route's
    /// depth: what UIKit adds there paints over the route, and the node never
    /// shows blank.
    func testTheHighlightUIKitAddsPaintsOverTheRoute() throws {
        let session = try boot()
        let host = session.presenter.menus.context
        let peek = try node(session, "peek")
        let undo = try stripAround(peek)
        defer { undo() }
        let interaction = try XCTUnwrap(peek.interactions.compactMap { $0 as? UIContextMenuInteraction }.first)
        let configuration = try XCTUnwrap(host.contextMenuInteraction(interaction, configurationForMenuAtLocation: .zero))
        defer { host.contextMenuInteraction(interaction, willEndFor: configuration, animator: nil) }
        let lifted = try XCTUnwrap(host.contextMenuInteraction(interaction, configuration: configuration, highlightPreviewForItemWithIdentifier: configuration.identifier))
        let mount = try XCTUnwrap(try holder(of: peek) as? PresentationMount, "the route's mount")
        let route = try XCTUnwrap(mount.subviews.first { peek.isDescendant(of: $0) } as? NodeView)
        let stage = try XCTUnwrap(lifted.target.container as? LiftStage)
        XCTAssertTrue(stage.superview === mount, "on a stage in the route's mount")
        XCTAssertGreaterThan(route.paintZPosition, 0, "the positioned route ranks above the origin")
        XCTAssertEqual(stage.layer.zPosition, route.paintZPosition, accuracy: 1e-9)
        XCTAssertTrue(NodeView.hitOrder(mount.subviews).first === stage)
    }

    func testTheRowLiftsIntoUIKitsMenuWithItsPreviewAndThePreviewPopsIntoItsScreen() throws {
        let session = try boot()
        let host = session.presenter.menus.context
        let peek = try node(session, "peek")
        let undo = try stripAround(peek)
        defer { undo() }
        let interaction = try XCTUnwrap(peek.interactions.compactMap { $0 as? UIContextMenuInteraction }.first, "the node's context menu is UIKit's")
        XCTAssertEqual(peek.contextRecognizer?.isEnabled, false, "its plain long press is off: the interaction recognizes it")
        XCTAssertEqual(text(try node(session, "peeks")), "peeks 0")

        let configuration = try XCTUnwrap(host.contextMenuInteraction(interaction, configurationForMenuAtLocation: CGPoint(x: 10, y: 10)))
        XCTAssertEqual(text(try node(session, "peeks")), "peeks 0", "nothing fires before the menu is shown")
        let lifted = try XCTUnwrap(host.contextMenuInteraction(interaction, configuration: configuration, highlightPreviewForItemWithIdentifier: configuration.identifier))
        XCTAssertTrue(lifted.view === peek, "the node itself, which UIKit hides and returns")
        XCTAssertEqual(lifted.parameters.visiblePath?.cgPath, UIBezierPath(roundedRect: peek.bounds, cornerRadius: 12).cgPath,
                       "its one radius as UIKit's rounded rectangle, which UIKit keeps through the lift")
        XCTAssertTrue(lifted.target.container is LiftStage && near(shown(lifted.target), shown(peek)), "from the node's place, on its stage, which moves with its parent")
        let owner = try XCTUnwrap(host.open)
        let controller = try XCTUnwrap(host.lift(owner))
        let menu = try XCTUnwrap(host.menu(owner))
        host.contextMenuInteraction(interaction, willDisplayMenuFor: configuration, animator: nil)
        XCTAssertEqual(peek.alpha, 1, "the host leaves the node's opacity alone while the menu shows")
        XCTAssertEqual(text(try node(session, "peeks")), "peeks 1", "its own contextmenu, once, before the rows or preview are read")

        let preview = try node(session, "peek-preview")
        XCTAssertTrue(preview.superview === controller.view, "the preview row is the controller's view's content")
        XCTAssertFalse(preview.isHidden)
        XCTAssertEqual(controller.preferredContentSize, CGSize(width: 300, height: 180), "sized by its layout: the popover's 300-point content box")
        XCTAssertEqual(preview.frame.origin, .zero)
        // Two sections (the hr), the preview not among the items.
        let titles = menu.children.flatMap { ($0 as? UIMenu)?.children ?? [$0] }.map { ($0 as? UIAction)?.title ?? "" }
        XCTAssertEqual(titles, ["Bump", "Clear"])
        XCTAssertEqual((menu.children.last as? UIMenu)?.children.first.flatMap { ($0 as? UIAction)?.attributes.contains(.destructive) }, true)
        XCTAssertEqual(session.presenter.menus.observation()?["kind"] as? String, "contextmenu")

        // Tapping the preview: its press pushes Detail, unanimated, and UIKit pops into it.
        let before = session.presenter.navigationHost.activeKey
        let commit = Commit()
        commit.preferredCommitStyle = .dismiss
        host.contextMenuInteraction(interaction, willPerformPreviewActionForMenuWith: configuration, animator: commit)
        XCTAssertEqual(commit.preferredCommitStyle, .pop, "a press that navigates commits with .pop")
        XCTAssertEqual(commit.animated, 1, "with an animation, without which UIKit shrinks the preview away")
        XCTAssertNotEqual(session.presenter.navigationHost.activeKey, before, "the press pushed")
        XCTAssertNotNil(try node(session, "route-detail"))
        XCTAssertNil(host.contextMenuInteraction(interaction, configuration: configuration, dismissalPreviewForItemWithIdentifier: configuration.identifier),
                     "after it navigated, the source is under the new screen: no return to it")
        host.contextMenuInteraction(interaction, willEndFor: configuration, animator: nil)
        XCTAssertEqual(peek.alpha, 1, "and after it ends")
        XCTAssertNil(host.open)
        let popover = try XCTUnwrap(session.presenter.views.values.first { $0.props["id"] == "peek" })
        XCTAssertTrue(popover.container.subviews.contains(preview), "the row is back in its popover")
        XCTAssertTrue(popover.isHidden, "which stays hidden")
    }

    /// The view's focus (key commands): a bar's touch sets it aside until the
    /// next touch on the page, a second bar touch keeps it held, and a
    /// context menu shown meanwhile returns it when it ends.
    func testABarsTouchSetsTheViewsFocusAsideUntilTheNextTouch() throws {
        let session = try boot()
        let view = try XCTUnwrap(session.view)
        let focus = session.presenter.menus.focus
        let shortcut = try node(session, "peek")
        session.presenter.apply(wireBatch([["op": "props", "id": shortcut.id, "set": ["accessibilityKeyShortcuts": "Meta+k"]]]))
        if !view.isFirstResponder { XCTAssertTrue(view.becomeFirstResponder()) }
        focus.setAside(untilTouch: true)
        XCTAssertFalse(view.isFirstResponder)
        focus.setAside(untilTouch: true)
        XCTAssertTrue(focus.untilNextTouch, "a second bar touch keeps what is held")
        focus.touched()
        XCTAssertTrue(view.isFirstResponder, "back at the next touch on the page")
        focus.setAside(untilTouch: true)
        focus.setAside()
        XCTAssertFalse(focus.untilNextTouch, "a menu with an end returns it")
        focus.restore()
        XCTAssertTrue(view.isFirstResponder)
        // A focused node is blurred by a bar's touch, and the view, which hears
        // key commands, takes the focus at the next touch on the page.
        let node = try node(session, "peek")
        XCTAssertTrue(node.becomeFirstResponder())
        focus.setAside(untilTouch: true)
        XCTAssertFalse(node.isFirstResponder)
        XCTAssertFalse(view.isFirstResponder, "not while the bar's menu may be up")
        XCTAssertTrue(focus.untilNextTouch)
        focus.touched()
        XCTAssertTrue(view.isFirstResponder)
        // A context menu over a focused node: neither the node nor the view
        // keeps the focus while it shows (a resigning node hands it to the
        // view), and the node has it back after.
        XCTAssertTrue(node.becomeFirstResponder())
        focus.setAside()
        XCTAssertFalse(node.isFirstResponder)
        XCTAssertFalse(view.isFirstResponder)
        focus.restore()
        XCTAssertTrue(node.isFirstResponder)
    }

    /// A text field or text area being edited keeps its focus, and its
    /// keyboard, through a bar's touch and a context menu alike: UIKit and
    /// the app decide when it goes, not the menu (MenuFocusIOS).
    func testAFieldBeingEditedKeepsItsFocusThroughABarsTouchAndAMenu() throws {
        let session = try boot()
        let focus = session.presenter.menus.focus
        // The first of the fixture's text fields that takes the focus.
        let field = try XCTUnwrap(session.presenter.views.values.compactMap(\.field).first { $0.window != nil && $0.becomeFirstResponder() },
                                  "the fixture has a text field")
        focus.setAside(untilTouch: true)
        XCTAssertTrue(field.isFirstResponder, "a bar's touch leaves the field alone")
        XCTAssertFalse(focus.untilNextTouch)
        focus.setAside()
        XCTAssertTrue(field.isFirstResponder, "and so does a context menu")
        focus.restore()
        XCTAssertTrue(field.isFirstResponder)
        _ = field.resignFirstResponder()
    }

    func testAMenuDismissedWithoutAChoiceReturnsItsRowAndAPreviewThatDoesNotNavigateDismisses() throws {
        let session = try boot()
        let host = session.presenter.menus.context
        let peek = try node(session, "peek")
        let undo = try stripAround(peek)
        defer { undo() }
        let interaction = try XCTUnwrap(peek.interactions.compactMap { $0 as? UIContextMenuInteraction }.first)
        let configuration = try XCTUnwrap(host.contextMenuInteraction(interaction, configurationForMenuAtLocation: .zero))
        let owner = try XCTUnwrap(host.open)
        _ = try XCTUnwrap(host.lift(owner))
        let preview = try node(session, "peek-preview")
        let returned = try XCTUnwrap(host.contextMenuInteraction(interaction, configuration: configuration, dismissalPreviewForItemWithIdentifier: configuration.identifier))
        XCTAssertTrue(returned.view === peek && returned.target.container is LiftStage && near(shown(returned.target), shown(peek)),
                      "dismissed, UIKit returns the node to its place, on its stage")
        host.contextMenuInteraction(interaction, willEndFor: configuration, animator: nil)
        XCTAssertNil(host.open)
        let popover = try XCTUnwrap(session.presenter.views.values.first { $0.props["id"] == "peek" })
        XCTAssertTrue(popover.container.subviews.contains(preview))

        // A menu item presses its row.
        var pressed: [String] = []
        let press = session.presenter.onPress
        session.presenter.onPress = { [weak presenter = session.presenter] id in pressed.append(presenter?.views[id]?.props["testId"] ?? "?"); press?(id) }
        _ = try XCTUnwrap(host.contextMenuInteraction(interaction, configurationForMenuAtLocation: .zero))
        let menu = try XCTUnwrap(host.menu(try XCTUnwrap(host.open)))
        let bump = try XCTUnwrap(menu.children.flatMap { ($0 as? UIMenu)?.children ?? [$0] }.first as? UIAction)
        UIButton().sendAction(bump)
        XCTAssertEqual(pressed, ["peek-bump"])
        host.contextMenuInteraction(interaction, willEndFor: try XCTUnwrap(host.contextMenuInteraction(interaction, configurationForMenuAtLocation: .zero)), animator: nil)

        // A preview whose press stays on this screen dismisses.
        session.presenter.onPress = { _ in }
        let again = try XCTUnwrap(host.contextMenuInteraction(interaction, configurationForMenuAtLocation: .zero))
        _ = try XCTUnwrap(host.lift(try XCTUnwrap(host.open)))
        let commit = Commit()
        host.contextMenuInteraction(interaction, willPerformPreviewActionForMenuWith: again, animator: commit)
        XCTAssertEqual(commit.preferredCommitStyle, .dismiss)
        XCTAssertEqual(commit.animated, 0)
        host.contextMenuInteraction(interaction, willEndFor: again, animator: nil)
        session.presenter.onPress = press
    }

    /// A link no authored menu answers lifts with the host's own menu: the
    /// link itself on its platter, its URL over Open Link, Copy Link and Share.
    func testALinkNoMenuAnswersLiftsItselfWithTheLinksOwnMenu() throws {
        let session = try boot()
        let host = session.presenter.menus.context
        func titles(_ menu: UIMenu) -> [String] { menu.children.map { ($0 as? UIAction)?.title ?? "" } }
        func interaction(_ node: NodeView) -> UIContextMenuInteraction? { node.interactions.compactMap { $0 as? UIContextMenuInteraction }.first }

        let link = try node(session, "plain-link")
        let linked = try XCTUnwrap(interaction(link), "a link gets UIKit's interaction with no menu authored")
        let configuration = try XCTUnwrap(host.contextMenuInteraction(linked, configurationForMenuAtLocation: CGPoint(x: link.bounds.midX, y: link.bounds.midY)))
        let lifted = try XCTUnwrap(host.contextMenuInteraction(linked, configuration: configuration, highlightPreviewForItemWithIdentifier: configuration.identifier))
        XCTAssertTrue(lifted.view === link, "the link itself lifts")
        let owner = try XCTUnwrap(host.open)
        XCTAssertNil(host.lift(owner), "no preview: UIKit shows the lifted link")
        let menu = try XCTUnwrap(host.menu(owner))
        XCTAssertEqual(menu.title, "https://example.com/fixture")
        XCTAssertEqual(titles(menu), ["Open Link", "Copy Link", "Share…"])
        host.contextMenuInteraction(linked, willEndFor: configuration, animator: nil)

        // A card's authored menu answers a long press on a link inside it.
        XCTAssertNil(interaction(try node(session, "card-link")))
        let card = try node(session, "peek-card"), inCard = try node(session, "card-link")
        let cardMenu = try XCTUnwrap(interaction(card))
        let authored = try XCTUnwrap(host.contextMenuInteraction(cardMenu, configurationForMenuAtLocation: inCard.convert(CGPoint(x: inCard.bounds.midX, y: inCard.bounds.midY), to: card)))
        XCTAssertEqual(host.open?.isLink, false)
        XCTAssertEqual(host.open?.name, "peek")
        host.contextMenuInteraction(cardMenu, willEndFor: authored, animator: nil)

        // A run's link: its words leave the paragraph while their picture is
        // lifted, and tapping that picture follows the link. A path in the app
        // gets that menu only on the app's declared origin.
        let paragraph = try node(session, "link-text")
        guard ExactEnv.appMetadata["ExactOrigin"] != nil else {
            XCTAssertNil(interaction(paragraph), "a path in the app has no menu without a declared origin")
            return
        }
        let run = try XCTUnwrap(paragraph.inlineText.first { $0.props["testId"] == "run-link" })
        let box = try XCTUnwrap(paragraph.inlineRects(run).first)
        let runs = try XCTUnwrap(interaction(paragraph))
        let at = try XCTUnwrap(host.contextMenuInteraction(runs, configurationForMenuAtLocation: CGPoint(x: box.midX, y: box.midY)))
        let runOwner = try XCTUnwrap(host.open)
        XCTAssertTrue(runOwner.isLink)
        XCTAssertEqual(runOwner.name, "/detail")
        XCTAssertEqual(runOwner.inlineID, run.id)
        XCTAssertEqual(titles(try XCTUnwrap(host.menu(runOwner))), ["Open Link", "Copy Link", "Share…"])
        let ink = try XCTUnwrap(paragraph.textRasterLayer)
        XCTAssertNil(ink.mask)
        _ = try XCTUnwrap(host.contextMenuInteraction(runs, configuration: at, highlightPreviewForItemWithIdentifier: at.identifier))
        let cut = try XCTUnwrap(ink.mask as? CAShapeLayer, "the run's words are cut out of the paragraph")
        XCTAssertFalse(try XCTUnwrap(cut.path).contains(paragraph.layer.convert(CGPoint(x: box.midX, y: box.midY), to: ink), using: .evenOdd))
        let commit = Commit()
        host.contextMenuInteraction(runs, willPerformPreviewActionForMenuWith: at, animator: commit)
        XCTAssertEqual(commit.preferredCommitStyle, .dismiss)
        XCTAssertNotNil(try node(session, "route-detail"), "the run's link followed")
        host.contextMenuInteraction(runs, willEndFor: at, animator: nil)
        XCTAssertNil(ink.mask, "and back when the menu ends")
    }

    /// A link's Copy Link and Share take its URL as a browser resolves it, on the app's declared origin; only web
    /// links get the host's menu (LLP 1021 §5.1).
    func testALinksMenuTakesTheURLABrowserWouldResolve() {
        let origin = "https://example.com", visit = "/docs/guide?tab=2#top"
        let url = { (href: String) in ContextMenuHost.linkURL(href, origin: origin, visit: visit)?.absoluteString }
        XCTAssertEqual(url("/detail"), "https://example.com/detail")
        XCTAssertEqual(url("about"), "https://example.com/docs/about")
        XCTAssertEqual(url("#moves"), "https://example.com/docs/guide?tab=2#moves")
        XCTAssertEqual(url("?tab=3"), "https://example.com/docs/guide?tab=3")
        XCTAssertEqual(url("//cdn.example.org/x"), "https://cdn.example.org/x")
        XCTAssertEqual(url("https://other.example/a b"), "https://other.example/a%20b")
        XCTAssertNil(ContextMenuHost.linkURL("/detail", origin: nil, visit: visit), "a link in the app has no URL without a declared origin")
        XCTAssertEqual(ContextMenuHost.linkURL("https://other.example/", origin: nil, visit: visit)?.absoluteString, "https://other.example/")
        for href in ["mailto:a@example.com", "tel:+15555550100", "javascript:alert(1)", "JavaScript:void(0)", "myapp://open", ""] {
            XCTAssertFalse(NodeView.isWebLink(href), href)
            XCTAssertFalse(NodeView.hasLinkMenu(href, origin: origin), href)
            XCTAssertNil(url(href), href)
        }
        for href in ["https://example.com", "HTTP://example.com", "/detail", "about", "#moves", "//cdn.example.org/x"] {
            XCTAssertTrue(NodeView.isWebLink(href), href)
            XCTAssertTrue(NodeView.hasLinkMenu(href, origin: origin), href)
        }
        // Without a declared origin a path in the app names no URL: no menu, as a tap already opens it
        for href in ["/detail", "about", "#moves", "//cdn.example.org/x"] {
            XCTAssertFalse(NodeView.hasLinkMenu(href, origin: nil), href)
        }
        XCTAssertTrue(NodeView.hasLinkMenu("https://example.com", origin: nil))
    }
}
#endif
