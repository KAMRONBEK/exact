//! PROTO (rule 4, "Exact describes, UIKit drives"): the far pass. For each route whose top edge UIKit moves (a large
//! title collapsing, a bar minimizing), the frames the boxes that depend on that edge take with it at the block's top,
//! which the host moves them along as the edge moves.
use super::*;

impl<D: DataSource> Host<D> {
    /// PROTO (rule 4): `route`'s clearance's top edge moves, or no longer does.
    pub fn set_moving_top(&mut self, route: ViewId, on: bool) {
        self.moving_tops.retain(|r| *r != route);
        if on {
            self.moving_tops.push(route);
            self.far_owed = true;
        }
    }

    /// PROTO (rule 4, "Exact describes, UIKit drives"): for each route whose
    /// top edge moves, the frame each box that depends on the edge would take
    /// with the edge at the block's top, and where a box is not linear in the
    /// edge, its frames at the knots that make it so (`halve`): the host moves
    /// those boxes along them as UIKit moves the edge.
    pub(super) fn emit_far(
        &mut self,
        batch: &mut Batch,
        owed: bool,
        touched: &[NodeKey],
        changed: &[NodeKey],
    ) {
        for route in self.moving_tops.clone() {
            // A layout confined to the content the last pass froze changes nothing a trial lays out
            if !owed && !self.reaches_trial(route, touched, changed) {
                continue;
            }
            let started = std::time::Instant::now(); // PROTO: the far pass timed
                                                     // One trial session per pass: the kernel copies once and lays out each place in the copy
            let Ok(mut trial) = self.runner.kernel_mut().clearance_trial(route, 0.0) else {
                // Clear retained frames when a trial cannot start
                batch.far_reset(route, 0, 0, [0, 0, 0]);
                continue;
            };
            self.far_frozen.insert(route, trial.frozen().to_vec());
            let Ok(all) = trial.frames(1.0) else {
                continue;
            };
            let moving: Vec<_> = all
                .into_iter()
                .filter(|(_, now, far)| !near(now, far))
                .collect();
            let ids: Vec<ViewId> = moving.iter().map(|m| m.0).collect();
            let (rest, far): (Vec<Frame>, Vec<Frame>) = moving.iter().map(|m| (m.1, m.2)).unzip();
            // How far the edge travels across the range, read off the boxes it moves furthest
            let travel = rest
                .iter()
                .zip(&far)
                .map(|(a, b)| (a.y - b.y).abs().max((a.height - b.height).abs()))
                .fold(0.0, f32::max);
            let mut knots = Vec::new();
            halve(
                &mut trial,
                &ids,
                (0.0, &rest),
                (1.0, &far),
                travel,
                &mut knots,
            );
            drop(trial);
            knots.sort_by(|a, b| a.0.total_cmp(&b.0));
            let (copy, lay, collect, _) = exact_kernel::kernel::take_trial_phases(); // PROTO: the trials' phases
            batch.far_reset(
                route,
                started.elapsed().as_micros() as u64,
                TRIALS.with(|n| n.replace(0)) + 1,
                [copy, lay, collect],
            );
            for (i, (id, now, f)) in moving.iter().enumerate() {
                let inner: Vec<(f32, [f32; 4])> = knots
                    .iter()
                    .map(|(t, frames)| (*t, rect(frames[i])))
                    .collect();
                batch.far(*id, route, rect(*now), rect(*f), &inner);
            }
        }
    }

    /// PROTO (rule 4): whether a box this emission touched lies under `route` outside the content its last far
    /// pass froze, or is a frozen scroller whose own frame changed: then the route's far frames may have changed.
    fn reaches_trial(&self, route: ViewId, touched: &[NodeKey], changed: &[NodeKey]) -> bool {
        let Some(frozen) = self.far_frozen.get(&route) else {
            return true;
        };
        let kernel = self.runner.kernel();
        for &key in touched.iter().chain(changed) {
            let Some(node) = kernel.node_by_key(key) else {
                return true;
            };
            if frozen.contains(&node.id) {
                if changed.contains(&key) {
                    return true;
                }
                continue;
            }
            if node.id == route {
                return true;
            }
            let mut at = node.parent;
            while let Some(id) = at {
                if frozen.contains(&id) {
                    break;
                }
                if id == route {
                    return true;
                }
                at = kernel.node(id).and_then(|n| n.parent);
            }
        }
        false
    }
}

/// PROTO (rule 4): how far apart two frames may be, in points, and count as one
const SLACK: f32 = 0.25;

thread_local! {
    /// PROTO (rule 4): the trial layouts `halve` ran, for the far pass's log
    static TRIALS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// PROTO (rule 4): whether two frames are the same within `SLACK`
fn near(a: &Frame, b: &Frame) -> bool {
    [a.x - b.x, a.y - b.y, a.width - b.width, a.height - b.height]
        .iter()
        .all(|d| d.abs() <= SLACK)
}

fn rect(f: Frame) -> [f32; 4] {
    [f.x, f.y, f.width, f.height]
}

/// PROTO (rule 4): the knots between `(t0, f0)` and `(t1, f1)` at which the
/// boxes `ids` of `route` are laid out, so that each is linear in the edge
/// between neighbouring knots: the midpoint is laid out (`ClearanceTrial::frames`)
/// and, where any box strays from the line between the ends by more than
/// `SLACK`, it becomes a knot and each half is halved again, until a range
/// spans no more than `SLACK` of the edge's `travel`, where a box's error
/// stays within `SLACK` too. A box between a range's ends that is straight at
/// its middle is taken to be straight throughout: a `min-*` or `max-*` bound
/// reached once bends it off the line at the middle.
fn halve(
    trial: &mut exact_kernel::kernel::ClearanceTrial<'_>,
    ids: &[ViewId],
    (t0, f0): (f32, &[Frame]),
    (t1, f1): (f32, &[Frame]),
    travel: f32,
    knots: &mut Vec<(f32, Vec<Frame>)>,
) {
    if ids.is_empty() || (t1 - t0) * travel <= SLACK {
        return;
    }
    let tm = (t0 + t1) / 2.0;
    TRIALS.with(|n| n.set(n.get() + 1));
    let Ok(all) = trial.frames(tm) else {
        return;
    };
    let at: std::collections::HashMap<ViewId, Frame> =
        all.into_iter().map(|(id, _, f)| (id, f)).collect();
    let mid: Vec<Frame> = ids
        .iter()
        .zip(f0)
        .map(|(id, a)| at.get(id).copied().unwrap_or(*a))
        .collect();
    let line = |a: &Frame, b: &Frame| Frame {
        x: (a.x + b.x) / 2.0,
        y: (a.y + b.y) / 2.0,
        width: (a.width + b.width) / 2.0,
        height: (a.height + b.height) / 2.0,
    };
    if mid
        .iter()
        .zip(f0.iter().zip(f1))
        .all(|(m, (a, b))| near(m, &line(a, b)))
    {
        return;
    }
    knots.push((tm, mid.clone()));
    halve(trial, ids, (t0, f0), (tm, &mid), travel, knots);
    halve(trial, ids, (tm, &mid), (t1, f1), travel, knots);
}
