// The buttons a grouped section's header or footer shows after its text
// (LLP 1084 D3; expo/exact#836): the author's own controls, carried into
// UIKit's header as one trailing accessory, as a hand-built header seats a
// button beside its title (`UICellAccessory.customView`) and SwiftUI's
// `Section` header of a text and a button draws one. They are carried as a
// custom row's views are (`GroupedPlacingIOS.swift`): home before a batch
// that must read them there, and back into the label after it. So each is
// the control its author wrote, with its press, its menu, its style and its
// accessibility element; unstyled, the platform's plain tinted button.
import ExactKit
#if os(iOS) || os(tvOS)
import UIKit

/// A section's header or footer, with the seat of the buttons it shows.
/// UIKit makes a list cell that has content one accessibility element, which
/// would hide its buttons from VoiceOver. A label with buttons is their
/// container instead: its words, with the label and traits UIKit gives the
/// cell (a header's is a heading), then each button.
final class GroupedLabelCell: UICollectionViewListCell {
    fileprivate(set) var seat: LabelSeat?
    private lazy var words = UIAccessibilityElement(accessibilityContainer: self)
    override var isAccessibilityElement: Bool {
        get { seat == nil && super.isAccessibilityElement }
        set { super.isAccessibilityElement = newValue }
    }
    override var accessibilityElements: [Any]? {
        get {
            guard let seat else { return super.accessibilityElements }
            words.accessibilityLabel = accessibilityLabel
            words.accessibilityTraits = accessibilityTraits
            words.accessibilityFrameInContainerSpace = contentView.frame
            return (words.accessibilityLabel?.isEmpty ?? true ? [] : [words]) + seat.subviews
        }
        set { super.accessibilityElements = newValue }
    }
}

/// The one accessory view a label's buttons stand in, placed among
/// themselves as layout placed them in the label.
final class LabelSeat: UIView {
    /// The buttons it holds, in order.
    fileprivate var buttons: [UInt32] = []
    /// Its origin in the label the buttons were carried from.
    fileprivate var origin = CGPoint.zero
    override var intrinsicContentSize: CGSize { bounds.size }
    override func sizeThatFits(_ size: CGSize) -> CGSize { bounds.size }
}

extension GroupedListView {
    /// The buttons section `index` shows in its header or its footer.
    private func buttons(header: Bool, section index: Int) -> [UInt32] {
        guard model.sections.indices.contains(index) else { return [] }
        return header ? model.sections[index].headerButtons : model.sections[index].footerButtons
    }

    /// Section `index`'s buttons carried into `cell`, its header or footer:
    /// one trailing accessory as large as the buttons and the gaps between
    /// them. A label whose buttons came, went or changed size is given its
    /// accessory again and the list laid out (`mount`).
    func seat(_ cell: GroupedLabelCell, header: Bool, section index: Int) {
        let nodes = buttons(header: header, section: index).compactMap { host.presenter.views[$0] }
        for node in nodes where carried[node.id] == nil {
            guard let parent = node.superview else { continue }
            noteHome(parent)
            carried[node.id] = (parent, node.frame, node.authoredInert, nil)
        }
        let homes = nodes.compactMap { node in carried[node.id].map { (node, node.grid?.drawn ?? $0.frame) } }
        let ids = homes.map(\.0.id)
        // A button the label shows no longer (its cell reused for another section's) goes home.
        for case let old as NodeView in cell.seat?.subviews ?? [] where !ids.contains(old.id) { sendHome(old) }
        guard let first = homes.first else {
            if cell.seat != nil { cell.seat = nil; cell.accessories = []; resized = true }
            return
        }
        let box = homes.dropFirst().reduce(first.1) { $0.union($1.1) }
        if cell.seat?.buttons != ids || cell.seat?.bounds.size != box.size {
            let seat = LabelSeat(frame: CGRect(origin: .zero, size: box.size))
            seat.buttons = ids
            cell.seat = seat
            cell.accessories = [.customView(configuration: .init(customView: seat, placement: .trailing(), reservedLayoutWidth: .actual, maintainsFixedSize: true))]
            resized = true
        }
        guard let seat = cell.seat else { return }
        seat.origin = box.origin
        for (node, home) in homes {
            if node.superview !== seat { seat.addSubview(node) }
            node.frame = home.offsetBy(dx: -box.minX, dy: -box.minY)
        }
    }

    /// Every label on screen takes its buttons again: a batch may have sent
    /// them home, or changed which a section shows.
    func seatLabels() {
        for kind in [UICollectionView.elementKindSectionHeader, UICollectionView.elementKindSectionFooter] {
            for path in collection.indexPathsForVisibleSupplementaryElements(ofKind: kind) {
                guard let cell = collection.supplementaryView(forElementKind: kind, at: path) as? GroupedLabelCell else { continue }
                seat(cell, header: kind == UICollectionView.elementKindSectionHeader, section: path.section)
            }
        }
    }

    /// Where layout's `place` draws a button this list carries in a label:
    /// in its seat. Nil for any other view.
    func seatFrame(of view: NodeView, at place: GridPlace) -> CGRect? {
        guard let seat = view.superview as? LabelSeat, carried[view.id] != nil else { return nil }
        return place.drawn.offsetBy(dx: -seat.origin.x, dy: -seat.origin.y)
    }

    /// The header or footer that shows `node`, one of this list's label
    /// buttons or a view in one, whether or not the label is on screen.
    func labelPath(holding node: NodeView) -> (kind: String, path: IndexPath)? {
        for (index, section) in model.sections.enumerated() {
            for (kind, ids) in [(UICollectionView.elementKindSectionHeader, section.headerButtons), (UICollectionView.elementKindSectionFooter, section.footerButtons)] {
                guard ids.contains(where: { id in host.presenter.views[id].map { node === $0 || node.isDescendant(of: $0) } ?? false }) else { continue }
                return (kind, IndexPath(item: 0, section: index))
            }
        }
        return nil
    }

    /// The agent's reveal of a label's button: its label scrolled into
    /// `port` as a row is, and its buttons carried into it.
    func reveal(label node: NodeView, port: CGRect) -> Bool {
        guard let label = labelPath(holding: node) else { return false }
        return reveal(port: port) {
            self.collection.layoutAttributesForSupplementaryElement(ofKind: label.kind, at: label.path).map { CGPoint(x: $0.frame.midX, y: $0.frame.midY) }
        }
    }

    func collectionView(_ view: UICollectionView, willDisplaySupplementaryView supplementary: UICollectionReusableView, forElementKind kind: String, at path: IndexPath) {
        guard let cell = supplementary as? GroupedLabelCell else { return }
        seat(cell, header: kind == UICollectionView.elementKindSectionHeader, section: path.section)
        if resized { host.presenter.requestProjectionSync() }
    }

    /// A label that leaves the screen sends its buttons home, as a cell
    /// sends its row.
    func collectionView(_ view: UICollectionView, didEndDisplayingSupplementaryView supplementary: UICollectionReusableView, forElementOfKind kind: String, at path: IndexPath) {
        guard let seat = (supplementary as? GroupedLabelCell)?.seat else { return }
        for case let button as NodeView in seat.subviews where carried[button.id] != nil { sendHome(button) }
    }
}
#endif
