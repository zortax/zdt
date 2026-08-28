//! Where in the painting order something goes.

/// Which way a set of elements is moved through the order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Order {
    /// One step nearer the front.
    Forward,
    /// One step nearer the back.
    Backward,
    /// In front of everything.
    Front,
    /// Behind everything.
    Back,
}

/// Where each of `moving` ends up in a list of `count` elements.
///
/// The answer names, for each place in the new order, which place in the old order goes there. A
/// set that is already as far as it can go stays where it is.
#[must_use]
pub fn reordered(count: usize, moving: &[usize], order: Order) -> Vec<usize> {
    let mut still: Vec<usize> = (0..count).filter(|at| !moving.contains(at)).collect();
    let mut moving: Vec<usize> = moving.iter().copied().filter(|at| *at < count).collect();
    moving.sort_unstable();

    match order {
        Order::Front => {
            still.extend(moving);
            still
        }
        Order::Back => {
            let mut out = moving;
            out.extend(still);
            out
        }
        Order::Forward | Order::Backward => {
            let mut out: Vec<usize> = (0..count).collect();
            // One step at a time, in the direction that keeps a run of elements together.
            if order == Order::Forward {
                for at in moving.iter().rev() {
                    let here = out.iter().position(|held| held == at).unwrap_or(0);
                    if here + 1 < out.len() && !moving.contains(&out[here + 1]) {
                        out.swap(here, here + 1);
                    }
                }
            } else {
                for at in &moving {
                    let here = out.iter().position(|held| held == at).unwrap_or(0);
                    if here > 0 && !moving.contains(&out[here - 1]) {
                        out.swap(here, here - 1);
                    }
                }
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_the_front_puts_it_last() {
        assert_eq!(reordered(4, &[0], Order::Front), [1, 2, 3, 0]);
    }

    #[test]
    fn to_the_back_puts_it_first() {
        assert_eq!(reordered(4, &[3], Order::Back), [3, 0, 1, 2]);
    }

    #[test]
    fn one_step_forward_swaps_with_the_one_in_front() {
        assert_eq!(reordered(4, &[1], Order::Forward), [0, 2, 1, 3]);
    }

    #[test]
    fn one_step_backward_swaps_with_the_one_behind() {
        assert_eq!(reordered(4, &[2], Order::Backward), [0, 2, 1, 3]);
    }

    #[test]
    fn something_already_at_the_end_stays_there() {
        assert_eq!(reordered(3, &[2], Order::Forward), [0, 1, 2]);
        assert_eq!(reordered(3, &[0], Order::Backward), [0, 1, 2]);
    }

    #[test]
    fn a_run_moves_together_and_keeps_its_own_order() {
        assert_eq!(reordered(4, &[0, 1], Order::Forward), [2, 0, 1, 3]);
        assert_eq!(reordered(4, &[0, 1], Order::Front), [2, 3, 0, 1]);
    }
}
