// What a native container covers of a box: navigation's bars, toolbars,
// sheets and status items report it, and the kernel lays out around it.
import CoreGraphics

/// What a native container covers of a box (LLP 1075.003 §3.5): bars over
/// its edges, added to its padding, or the whole box, which a native bar
/// replaces (`display: none`).
package enum HostCover: Equatable {
    package struct Edges: Equatable {
        package var top: CGFloat, right: CGFloat, bottom: CGFloat, left: CGFloat
        package init(top: CGFloat, right: CGFloat, bottom: CGFloat, left: CGFloat) {
            self.top = top; self.right = right; self.bottom = bottom; self.left = left
        }
    }
    case edges(Edges)
    case whole
    /// A scroller UIKit insets (a route's collapsing or bottom-edge
    /// scroller): the kernel lays its content out between these insets
    /// (LLP 1116.000 D1.2).
    case inset(top: CGFloat, bottom: CGFloat)
    /// Space occupied by native scrollers, measured after tiling.
    case gutter(right: CGFloat, bottom: CGFloat)
}

/// A content owner's values (LLP 1123 D9), in its content root's points:
/// the safe area left there (top, right, bottom, left), D10's twenty region
/// insets (`nil`: each is the safe area), the segments a fold makes of the
/// content root, and its size, which container units read (D12).
struct OwnerValues: Equatable {
    var safe: [CGFloat]
    var regions: [CGFloat]?
    var fold: ViewportFold
    var size: CGSize
}
