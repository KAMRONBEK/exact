#if os(iOS) || os(tvOS)
import UIKit
import ExactKit

/// A route whose first child is a `header` holding exactly one heading
/// (LLP 1035.001 D9's header-shaped route). The bar shows the heading as
/// its title — large for a level-1 heading, inline for any other — and the
/// header's buttons as its items, those before the heading leading and
/// those after it trailing, each pressing its authored button. The stack's
/// Back control is UIKit's back button, never an item. A header with no
/// heading that holds only what the bar places is a bar with no title,
/// its items trailing (`exact_kernel::bars`). iOS does not paint the
/// header; the web and the agent do (LLP 1021 D4's one presentation).
struct HeaderShape: Equatable {
    struct Item: Equatable {
        let id: UInt32, title: String, symbol: String?, label: String?, disabled: Bool, accessibilityDisabled: Bool
        /// The button's `testId`: the item's `accessibilityIdentifier`, so
        /// XCUITest and Maestro address it as any other node (LLP 1116 D2).
        let testId: String?
        let toolbarGroup: UInt32?
        /// A face drawn from the button's one filled box (an avatar or a
        /// badge: a shape holding a text or a symbol), when the button has
        /// one: the item's image, in the box's own colours and corners.
        let badge: BadgeFace?
        /// Whether the button opens a menu-shaped popover or a confirmation
        /// (`MenuHost.pullsDown`): the item's menu, its rows read when it
        /// opens, as LLP 1021 D3's pull-down. Any other popover opens as
        /// itself, from the item (`MenuHost.openFromBar`).
        let menu: Bool
        /// The popover's `aria-label`: the menu's title, as an in-page
        /// button's is (MenusIOS: "Open location in", "Sort by").
        let menuTitle: String
        /// The menu button's `menuOrder` (`MenuHost.order`).
        let menuOrder: String
        /// A native button's prominent style: a prominent bar item (iOS 26),
        /// as LLP 1069.011.000 D2 maps `NSToolbarItem`.
        let prominent: Bool
        /// A native button's `glass` or `clear-glass` style: a glass capsule
        /// of its own, not the one iOS 26 shares among adjacent items
        /// (`sharesBackground`). A bar item has no clear variant.
        let ownGlass: Bool
        /// The colour the author gave the item's face, light and dark: a
        /// symbol's tint, else a `color` written on the button itself. Nil
        /// (UIKit's tint, the platform default) for a colour the button
        /// only inherits or the compiler implied.
        let tint: [[Double]]?
        /// Whether that colour is the tint itself: the item's tint then, its node's `ownAccent` or the bar's
        let followsTint: Bool
        /// A native button's title font where its face's rows give a size or
        /// a weight (LLP 1069.011.001 D4), as its in-page control draws it:
        /// the item's title in it, so its glass fits those words. Nil is
        /// UIKit's bar title font.
        let font: UIFont?
        let active: NativeButtonState
        let activeTitleColor: BatchValue?
        var supportsActiveColor: Bool { symbol == nil && badge == nil && !hosted }
        /// A hosted element (LLP 1141.004 D1): an item nothing presses as a
        /// whole, whose custom view holds the element's own views
        /// (BarHostedIOS.swift).
        let hosted: Bool
        /// Whether a hosted item shares the bar's glass: something in it
        /// presses, and it draws no material of its own (D4).
        let hostedGlass: Bool
        /// The room a hosted item keeps beside its words inside the shared
        /// glass where its element writes no side padding: UIKit's
        /// `titleInset` (D4).
        let hostedInset: CGFloat
        /// How far a `UIBarButtonItem(title:)`'s label sits inside its item
        /// on each side, which UIKit does not publish: 12 pt on iOS 26 and 27
        /// (measured), so its glass is 16 pt wider than the words each side.
        static let titleInset: CGFloat = 12
        /// The font UIKit draws a plain bar item's title in, which it does
        /// not publish: the system font at 17 pt, medium in the 26 design and
        /// bold under Bold Text (both measured on iOS 27,
        /// `testTheUnsaidTitleFontIsUIKits`); regular before it, semibold
        /// under Bold Text. A row its author left unsaid keeps this one's
        /// size or weight; the kernel has stepped the weights it sends.
        static func titleFont(bold: Bool) -> UIFont {
            if #available(iOS 26.0, *), LinkedDesign.liquidGlass { return .systemFont(ofSize: 17, weight: bold ? .bold : .medium) }
            return .systemFont(ofSize: 17, weight: bold ? .semibold : .regular)
        }
        init(_ button: NodeView, hosted: Bool = false) {
            var symbol: String?, text = "", ink: (NodeView, String)?
            func walk(_ node: NodeView) {
                for child in Self.shown(node) {
                    if child.kind == "image", let name = child.faceSymbol {
                        if symbol == nil { symbol = name; ink = (child, "text_color") }
                    } else if child.isParagraph, text.isEmpty { text = child.accessibleText; ink = ink ?? (child, "text_color") }
                    else { walk(child) }
                }
            }
            // A native button's children are its face, not views (LLP 1069.011.000 D1).
            let face = button.isNativeButton ? button.face : nil
            if let face {
                symbol = face.symbol
                text = face.title ?? ""
            } else {
                walk(button)
            }
            // Up the authored tree: a hosted element the bar lifted keeps its group.
            var grouped: UInt32?
            if let presenter = button.presenter {
                for case let node as NodeView in presenter.logicalAncestry(of: button).dropFirst() {
                    if node.props["semanticTag"] == "header" { break }
                    if node.props["accessibilityRole"] == "group" { grouped = node.id; break }
                }
            }
            toolbarGroup = grouped
            id = button.id
            testId = button.props["testId"]
            self.hosted = hosted
            // Glass is for what presses (D4): an item nothing in which
            // presses is content on the bar, and a material the element draws
            // is its own.
            hostedGlass = hosted && Self.presses(button) && button.props["backgroundMaterial"] == nil
            // A bare custom view hugs its capsule, whose round ends then cut
            // into its words, so words in the glass (iOS 26's) keep a title item's inset.
            var glassed = false
            if #available(iOS 26.0, tvOS 26.0, *) { glassed = hostedGlass }
            hostedInset = glassed && Self.shows(words: button) && !button.wrote("padding_left") && !button.wrote("padding_right") ? Self.titleInset : 0
            badge = button.isNativeButton || hosted ? nil : BadgeFace(button)
            let invocation = button.presenter?.invocation(of: button)
            menu = invocation.map { $0.opensPopover && button.presenter?.menus.pullsDown($0.target) == true } == true
            menuTitle = menu ? invocation?.target.props["accessibilityLabel"] ?? "" : ""
            menuOrder = menu ? button.props["menuOrder"] ?? "" : ""
            self.symbol = symbol
            title = text
            label = hosted ? nil : button.props["accessibilityLabel"] ?? face?.label
            // An inert button is no enabled item, as in the page.
            disabled = button.disabled || button.inert
            accessibilityDisabled = button.ariaUnavailable
            prominent = ["filled", "bordered-prominent", "prominent-glass", "prominent-clear-glass"].contains(face?.style ?? "")
            // A badge (an avatar) on a button that asks for glass sits in its own capsule, as a glass button does.
            ownGlass = ["glass", "clear-glass"].contains(face?.style ?? "")
                || (badge != nil && Materials.glass(button.props["backgroundMaterial"]))
            #if os(tvOS)
            // tvOS's bar title font is not measured: its own stays.
            font = nil
            #else
            let rows = face?.rows.title ?? [:]
            font = rows["font_size"] != nil || rows["font_weight"] != nil
                ? ButtonConfigurationIOS.font(Self.titleFont(bold: button.traitCollection.legibilityWeight == .bold), rows: rows) : nil
            #endif
            // The platform's tint unless the author said otherwise (LLP
            // 1115.000 D2): a `color` written on the symbol, else a `color`
            // written on the button, a platform colour as much as a literal.
            // One the button only inherits, or a sheet wrote, is unsaid.
            // Read from the node it is written on, the tint in it as a value (`carriedTint`)
            active = NativeButtonState(button)
            let ownsTitle = face.map { NativeButtonState.colorOwnership($0.rows).title } ?? (ink?.0.wrote("text_color") == true)
            activeTitleColor = symbol == nil && badge == nil && !hosted && !ownsTitle
                && active.style.keys.allSatisfy { $0 == "press_color" } ? active.style["press_color"] : nil
            let source = ink.flatMap { $0.0.kind == "image" && $0.0.ownColor("text_color") != nil ? $0.0 : nil } ?? (button.ownColor("text_color") != nil ? button : nil)
            followsTint = source?.style["text_color"]?.tintAlpha == 1
            if !followsTint, let source, let light = source.carriedChannels("text_color", dark: false), let dark = source.carriedChannels("text_color", dark: true) {
                tint = [light, dark]
            } else {
                tint = nil
            }
        }
        /// Whether `node` or a node in it presses: a button, a control (a
        /// `progress` only shows), a field, a link or a `press`.
        static func presses(_ node: NodeView) -> Bool {
            // What `visibility` hides presses nothing a person sees; a child it
            // shows (`visibility: visible`) still may.
            (!node.cssVisibilityHidden && (node.isButton || node.actsAsButton || node.handlers.contains("press") || node.kind == "input"
                || (node.kind == "control" && node.props["type"] != "progress") || node.props["href"] != nil
                // A link or a press inline, which its paragraph owns, a Markdown link among them.
                || node.inlineText.contains { $0.handlers.contains("press") || !($0.props["href"] ?? "").isEmpty }
                || (node.props["markup"] == "markdown" && (node.props["text"] ?? "").contains("]("))))
                || shown(node).contains(where: presses)
        }
        /// `node`'s children that show (none `display: none`).
        static func shown(_ node: NodeView) -> [NodeView] {
            NodeView.committed(node.container.subviews).filter { $0.style["display"]?.string != "none" }
        }
        /// Whether the bar draws `button` as UIKit's own item (D1): a native
        /// button, but for a second line or a picture; a button whose face is
        /// one symbol, one text, or a symbol beside a text (`plain`); or the
        /// badge of LLP 1075.003 §9.6. Every other element that presses (a
        /// pressed row, a button with a dot, a second text, a font of the
        /// author's, a picture or a link inline) is hosted with its press and
        /// drawn as written, and so is one `visibility` hides.
        static func drawsFace(_ button: NodeView) -> Bool {
            guard !button.cssVisibilityHidden else { return false }
            if button.isNativeButton { return button.face.map { !$0.hasSubtitleOrPicture } ?? true }
            guard button.kind == "button" else { return false }
            return badge(button) || plain(button)
        }
        /// What UIKit's own item shows whole: one symbol (its image), one plain
        /// text (its title), or a symbol beside a text (the image, named by the
        /// text), each the button's own child. A plain text is one run in the
        /// bar's type: no span, link, press or picture inline, no `href` or
        /// handler of its own, no font of the author's. The button's own box is
        /// the bar's glass; a box between it and its parts is no plain face.
        static func plain(_ button: NodeView) -> Bool {
            // The button's own box is the glass, but a font, an opacity or a
            // filter on it reaches its words, which UIKit's title would drop.
            guard !layered(button), !["font_family", "font_size", "font_weight", "font_style", "opacity", "filter"].contains(where: button.wrote) else { return false }
            let parts = shown(button)
            guard (1...2).contains(parts.count) else { return false }
            var texts = 0, symbols = 0
            for part in parts {
                guard !part.cssVisibilityHidden, part.handlers.isEmpty else { return false }
                if plainText(part) { texts += 1 }
                else if part.kind == "image", part.faceSymbol != nil, written(part).allSatisfy(symbolRows.contains) { symbols += 1 }
                else { return false }
            }
            return texts <= 1 && symbols <= 1
        }
        /// One run of words in the bar's type, which acts as nothing of its own
        /// and writes nothing UIKit's title would drop: its colour and where
        /// it sits only, no Markdown, no span, `href` or handler.
        static func plainText(_ node: NodeView) -> Bool {
            node.isParagraph && node.inlineText.isEmpty && node.handlers.isEmpty && (node.props["href"] ?? "").isEmpty
                && node.props["markup"] == nil && written(node).allSatisfy(plainRows.contains)
        }
        /// What a plain text may write: its colour, and where it sits.
        static let plainRows: Set<String> = ["text_color", "flex_grow", "flex_shrink", "flex_basis", "align_self", "text_align", "direction",
                                             "width", "height", "min_width", "min_height", "max_width", "max_height",
                                             "margin_top", "margin_right", "margin_bottom", "margin_left",
                                             "padding_top", "padding_right", "padding_bottom", "padding_left"]
        /// What a plain symbol may write besides: its size and weight, which
        /// the bar's image size replaces, and its tint.
        static let symbolRows: Set<String> = plainRows.union(["font_size", "font_weight", "tint_color", "object_fit"])
        /// The style rows `node`'s author wrote.
        static func written(_ node: NodeView) -> [String] {
            guard case .array(let rows)? = node.style["written"] else { return [] }
            return rows.compactMap { if case .string(let key) = $0 { return key } else { return nil } }
        }
        /// The badge of §9.6, and only it: one filled box, no border, shadow
        /// or picture fill, holding one text (no picture or link inline), one
        /// symbol or one picture, and no layer beside it.
        static func badge(_ button: NodeView) -> Bool {
            guard button.kind == "button", BadgeFace(button) != nil, !layered(button) else { return false }
            let kids = shown(button)
            guard kids.count == 1, let box = kids.first, box.kind == "view", !layered(box), !box.cssVisibilityHidden,
                  box.handlers.isEmpty, box.props["backgroundMaterial"] == nil, !box.wrote("opacity"), !box.wrote("filter"),
                  box.style["box_shadow"] == nil, box.style["background_image"] == nil, box.number("border_width") == 0,
                  !["top", "right", "bottom", "left"].contains(where: { box.number("border_width_" + $0) > 0 }) else { return false }
            let parts = shown(box)
            guard parts.count == 1, let part = parts.first, !part.cssVisibilityHidden else { return false }
            guard part.handlers.isEmpty, !part.wrote("opacity"), !part.wrote("filter") else { return false }
            // A badge draws its text at its own size (§9.6), a font of the author's or not.
            if part.isParagraph { return part.inlineText.isEmpty && (part.props["href"] ?? "").isEmpty && part.props["markup"] == nil }
            return part.kind == "image" && (part.faceSymbol != nil || part.imageSource != nil)
        }
        /// Whether `node` draws a child as a layer with no view (`FlatLeavesIOS`).
        static func layered(_ node: NodeView) -> Bool {
            guard let flats = node.presenter?.flats else { return false }
            return (flats.children(of: node.id) ?? []).contains { id in flats.leaves[id].map { $0.style["display"]?.string != "none" } ?? false }
        }
        /// Whether `node` shows words: a paragraph in it, shown, says something.
        static func shows(words node: NodeView) -> Bool {
            // A native button's words are its face's, not views.
            if node.isNativeButton { return !(node.face?.title ?? "").isEmpty }
            return node.isParagraph ? !node.accessibleText.isEmpty : shown(node).contains { shows(words: $0) }
        }
        /// Everything a bar item is made from. A hosted element is not: its
        /// views follow it in place (`BarHosted.show`).
        var source: String {
            if hosted { return "hosted:\(toolbarGroup.map(String.init) ?? ""):\(id):\(testId ?? ""):\(hostedGlass):\(hostedInset)" }
            return "\(face):\(disabled):\(accessibilityDisabled)"
        }
        /// `source` but for whether the item is enabled.
        var face: String {
            if hosted { return source }
            return "\(toolbarGroup.map(String.init) ?? ""):\(id):\(title):\(symbol ?? ""):\(label ?? ""):\(prominent):\(ownGlass):\(badge?.source ?? ""):\(menu):\(menuTitle):\(menuOrder):\(tint ?? []):\(followsTint):\(font.map { "\($0.fontName) \($0.pointSize) \(TextEngine.controlWeight($0))" } ?? ""):\(testId ?? ""):\(activeSignature)"
        }
        private var activeSignature: String {
            let values = active.style.mapValues(\.any)
            guard !values.isEmpty, let data = try? JSONSerialization.data(withJSONObject: values, options: [.sortedKeys]) else { return "" }
            return String(decoding: data, as: UTF8.self) + String(activeTitleColor != nil)
        }
    }
    let header: NodeView
    let heading: NodeView?
    package let title: String
    let level: Int
    let leading: [Item], trailing: [Item]
    /// The header's `input type="search"`, if it has one: the item's
    /// search controller (LLP 1075.003 §9.6).
    let search: NodeView?
    /// The header's tablist of text tabs, if it has one: the item's title
    /// view, a segmented control (LLP 1075.003 §9.8).
    let segments: NodeView?
    /// What the heading's group holds besides it (an avatar, a subtitle) and
    /// whether pressing it does something: the item's title, richer (§9.10).
    let group: HeaderTitle?
    /// The header's nodes the bar places (`presents`): the heading, every
    /// item's button (the Back control's too), the search field, the
    /// tablist when it has tabs for the segmented title, and what the title
    /// shows of the heading's group.
    let placed: [NodeView]
    /// The elements the bar shows in views of their own (LLP 1141.004 D1),
    /// lifted out of the header rather than hidden with it.
    let hosted: [NodeView]
    /// Explicit opt-outs stay in the authored header with their descendants.
    let authored: [NodeView]
    let authoredBack: Bool

    /// `back` names the stack's Back control, left to UIKit's back button
    /// when there is one (`backIsUIKits`): the root of a presented stack
    /// has none, so there it is an item like the others.
    init?(route: NodeView, back: String?, backIsUIKits: Bool = true, projected: Bool = false) {
        // What is header-shaped is the kernel's rule, read over this route's views (`HeaderRule`, LLP 1115.001 D11).
        guard let rule = HeaderRule(route: route, back: back, backIsPlatforms: backIsUIKits, projected: projected) else { return nil }
        // Every role the header rule assigns can remain authored. In
        // particular the title's tap target is not one of its bar items.
        let roles = rule.items + rule.hosted + [rule.heading, rule.tap, rule.search, rule.segments].compactMap { $0 }
        let authored = projected ? roles.filter { $0.wrote("appearance") && $0.style["appearance"]?.string == "none" } : []
        self.authored = authored
        // In the authored tree: an element the bar lifted is still inside what keeps it.
        let projects = { (node: NodeView) in !authored.contains { Self.inside(node, $0) } }
        authoredBack = back != nil && roles.contains { $0.props["id"] == back && !projects($0) }
        let heading = rule.heading.flatMap { projects($0) ? $0 : nil }, titleProjects = heading != nil
        self.heading = heading
        let search = rule.search.flatMap { projects($0) ? $0 : nil }
        let segments = rule.segments.flatMap { projects($0) ? $0 : nil }
        let tap = rule.tap.flatMap { projects($0) ? $0 : nil }
        header = rule.header
        // A heading of only white space names nothing (accname trims it);
        // UIKit would take it as a title and show a pair of quotes
        let text = heading?.accessibleText ?? ""
        title = text.unicodeScalars.allSatisfy(\.properties.isWhitespace) ? "" : text
        level = titleProjects ? rule.level : 0
        // The title decides first, with the buttons, readouts, search field
        // and tablist apart from it; it takes a hosted element whole or not
        // at all (D1), and the rest are the bar's hosted items.
        // What the author keeps in the page is not the title's to read either.
        let candidates = rule.hosted.filter(projects)
        let apart = rule.items + rule.readouts + [rule.search, rule.segments].compactMap { $0 } + authored
        let group = heading.flatMap { HeaderTitle(header: rule.header, heading: $0, tap: tap, apart: apart, atomic: candidates, kept: authored) }
        #if os(tvOS)
        // tvOS's bar has no item that focus passes by undimmed: none hosted.
        let hosted: [NodeView] = []
        #else
        // What the title shows, as presented: segments take the title view
        // from a drawn title, whose glyph line is then an item of its own.
        let titled = HeaderShape.presented(group, segments: segments)?.placed ?? []
        // A readout is never the title's, even inside a glyph line it takes.
        // Read in the authored tree: an element the bar lifted is still in its box.
        // A button whose face the bar cannot draw is hosted with its press;
        // the Back control UIKit's back button stands for is no item.
        let hosted = candidates.filter { node in
            rule.readouts.contains { $0 === node } || !titled.contains { Self.inside($0, node) || Self.inside(node, $0) }
        } + (rule.leading + rule.trailing).filter { node in rule.items.contains { $0 === node } && projects(node) && !Item.drawsFace(node) }
        #endif
        self.hosted = hosted
        let item = { (node: NodeView) -> Item? in
            if hosted.contains(where: { $0 === node }) { return Item(node, hosted: true) }
            return rule.hosted.contains(where: { $0 === node }) ? nil : Item(node)
        }
        leading = rule.leading.filter(projects).compactMap(item)
        trailing = rule.trailing.filter(projects).compactMap(item)
        self.search = search
        self.segments = segments
        self.group = group
        let tablist = segments.flatMap { HeaderShape.titleTabs($0).isEmpty ? nil : $0 }
        // The title's pressable wrapper goes with the title it holds, unless
        // it holds an opted-out node, which stays where it is.
        let wrapper = titleProjects ? tap.flatMap { tap in authored.contains { Self.inside($0, tap) } ? nil : tap } : nil
        placed = ([rule.heading].compactMap { $0 } + rule.items.filter { item in !hosted.contains { $0 === item } } + [search, tablist, wrapper].compactMap { $0 }
                  + (HeaderShape.presented(group, segments: segments)?.placed ?? [])).filter(projects)
    }

    /// Whether `node` is `box` or inside it in the authored tree, where an
    /// element the bar lifted is still in its parent (LLP 1141.004 D3).
    static func inside(_ node: NodeView, _ box: NodeView) -> Bool {
        node.presenter?.logicalAncestry(of: node).contains { $0 === box } ?? node.isDescendant(of: box)
    }

    /// The tabs of a header's tablist the segmented title shows (§9.8).
    static func titleTabs(_ list: NodeView) -> [NodeView] {
        list.container.subviews.compactMap { $0 as? NodeView }.filter {
            $0.isButton && $0.props["accessibilityRole"] == "tab" && $0.handlers.contains("press")
        }
    }

    static func == (a: HeaderShape, b: HeaderShape) -> Bool {
        a.header === b.header && a.authoredBack == b.authoredBack && a.title == b.title && a.level == b.level && a.leading == b.leading && a.trailing == b.trailing && a.search === b.search && a.segments === b.segments && a.group == b.group && a.authored.map(\.id) == b.authored.map(\.id) && a.hosted.map(\.id) == b.hosted.map(\.id)
    }
}

#endif
