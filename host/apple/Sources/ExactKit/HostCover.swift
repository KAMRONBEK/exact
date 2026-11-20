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

/// What the iOS host reports of a content owner beside its covers (LLP 1123
/// D2's owner update): its margins as UIKit gives them, in its block's points
/// (D8), nil where it reports none, and the segments a fold makes of its
/// content root, in the content root's coordinates (D16). Its band is its
/// root's cover, which the kernel reads as the band until a host gives one.
package struct OwnerReport: Equatable {
    /// UIKit's margins for an owner (the kernel's `OwnerMargins`), each four
    /// top, right, bottom and left.
    package struct Margins: Equatable {
        /// The controller root view's margins guide as insets from the block's edges.
        package var root: [CGFloat]
        /// A plain view's default margins (a new `UIView`'s `layoutMargins`).
        package var plain: [CGFloat]
        /// The controller's `systemMinimumLayoutMargins`.
        package var minimum: [CGFloat]
        /// The width the root view's `readableContentGuide` keeps text to.
        package var readableWidth: CGFloat

        package init(root: [CGFloat], plain: [CGFloat], minimum: [CGFloat], readableWidth: CGFloat) {
            (self.root, self.plain, self.minimum, self.readableWidth) = (root, plain, minimum, readableWidth)
        }
    }
    package var margins: Margins?
    package var fold: ViewportFold

    package init(margins: Margins?, fold: ViewportFold) {
        (self.margins, self.fold) = (margins, fold)
    }
}
