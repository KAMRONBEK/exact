// How far a grouped list extends its cards' row into a window's side safe area or a fold band's middle where it meets
// one (`GroupedListView.sideReach`), which its layout's section insets read.
import ExactKit
#if os(iOS)
import UIKit

extension GroupedListView {
    /// Extend to a window's side safe area or a fold band's middle where the list meets it,
    /// unless an ancestor transforms or clips that extension (LLP 1078, LLP 1123).
    /// UIKit places inset-grouped cards 20 points inside a safe edge and 16 past it when
    /// the collection spans the screen; a fold's half band keeps the card clear of the band.
    func sideReach() -> (left: CGFloat, right: CGFloat) {
        foldReach = (0, 0)
        guard let window = owner.window else { return (0, 0) }
        let safe = window.safeAreaInsets, r = owner.convert(owner.bounds, to: window)
        let meets = { (inset: CGFloat, edge: CGFloat) in inset > 0 && abs(edge - inset) < 0.5 ? inset : 0 }
        var reach = (left: meets(safe.left, r.minX), right: meets(safe.right, window.bounds.width - r.maxX))
        for band in (ReservedRegions.divisions(of: window) ?? []).filter(\.active).map(\.frame) where band.height > band.width {
            if abs(r.maxX - band.minX) < 0.5 { reach.right = band.width / 2; foldReach.right = reach.right }
            if abs(r.minX - band.maxX) < 0.5 { reach.left = band.width / 2; foldReach.left = reach.left }
        }
        guard reach.left > 0 || reach.right > 0 else { return reach }
        let reached = r.inset(by: UIEdgeInsets(top: 0, left: -reach.left, bottom: 0, right: -reach.right))
        var at: UIView? = owner.superview
        while let v = at, v !== window {
            if !v.transform.isIdentity || v.clipsToBounds && !v.convert(v.bounds, to: window).insetBy(dx: -0.5, dy: -0.5).contains(reached) {
                foldReach = (0, 0)
                return (0, 0)
            }
            at = v.superview
        }
        return reach
    }
}
#endif
