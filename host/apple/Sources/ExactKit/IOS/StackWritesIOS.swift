// A navigation controller whose written stack is not lost. UIKit refuses a stack written while a transition or a
// presentation runs in the controller's hierarchy ("the navigation stack will not be updated"), and nothing asks
// again: the router's stack and the one shown then part for good (a fold's closing search dropped the pages an unfold
// asked for). The refused stack is written again once the transition ends; a newer write replaces it.
#if os(iOS) || os(tvOS)
import UIKit

final class ExactNavigationController: UINavigationController {
    /// The stack UIKit refused, still to write, and how many turns it has waited.
    private var refused: (stack: [UIViewController], turns: Int)?

    override func setViewControllers(_ viewControllers: [UIViewController], animated: Bool) {
        refused = nil
        super.setViewControllers(viewControllers, animated: animated)
        guard !self.viewControllers.elementsEqual(viewControllers, by: ===) else { return }
        refused = (viewControllers, 0)
        writeAgainLater()
    }

    private func write(_ stack: [UIViewController]) {
        super.setViewControllers(stack, animated: false)
        if viewControllers.elementsEqual(stack, by: ===) { refused = nil } else { writeAgainLater() }
    }

    /// Once the transition UIKit names ends, else at the next turn while one runs out of sight (a search's dismissal
    /// over another column), for at most two seconds of turns.
    private func writeAgainLater() {
        let again = { [weak self] in
            guard let self, let refused = self.refused else { return }
            guard refused.turns < 120 else { self.refused = nil; return }
            self.refused?.turns += 1
            self.write(refused.stack)
        }
        if let coordinator = transitionCoordinator { coordinator.animate(alongsideTransition: nil) { _ in DispatchQueue.main.async(execute: again) } }
        else { DispatchQueue.main.async(execute: again) }
    }
}
#endif
