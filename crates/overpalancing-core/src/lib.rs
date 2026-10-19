//! Deterministic game domain, independent of rendering and platform services.
#![forbid(unsafe_code)]

/// Number of seesaws in the game board.
pub const SEESAW_COUNT: usize = 4;
/// Each seesaw has two selectable sides.
pub const LANE_COUNT: usize = SEESAW_COUNT * 2;

/// Bounded dispenser selection. This does not implement dropping or game rules.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Dispenser {
    lane: usize,
}
impl Dispenser {
    /// Current selected side, numbered from the left.
    pub fn lane(self) -> usize {
        self.lane
    }
    /// Move one lane without wrapping at board edges.
    pub fn shift(&mut self, direction: i32) {
        self.lane =
            (self.lane as i32 + direction.signum()).clamp(0, LANE_COUNT as i32 - 1) as usize;
    }
    /// Select a lane from a normalized horizontal pointer position.
    pub fn select_fraction(&mut self, fraction: f32) {
        if fraction.is_finite() {
            self.lane =
                ((fraction.clamp(0.0, 1.0) * LANE_COUNT as f32) as usize).min(LANE_COUNT - 1);
        }
    }
}

/// Single-color presets available in the placement sandbox.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BallColor {
    /// Red, also identified with the letter R in presentation.
    Red,
    /// Blue, also identified with the letter B in presentation.
    Blue,
    /// Yellow, also identified with the letter Y in presentation.
    Yellow,
}

/// A configured sandbox ball; zero weight is rejected during placement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ball {
    /// Weight units, independent of the selected color.
    pub weight: u32,
    /// The sandbox's single selected color.
    pub color: BallColor,
}

/// Relative weight of the two sides, without any launch implication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Balance {
    /// The left stack has greater total weight.
    LeftHeavy,
    /// Both stacks have the same total weight.
    Balanced,
    /// The right stack has greater total weight.
    RightHeavy,
}

/// Eight fixed stacks, ordered left to right, paired into four seesaws.
///
/// Each stack is ordered bottom to top. Placement only appends a ball;
/// flights, matching, compression and capacity rules are deliberately absent.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Board {
    stacks: [Vec<Ball>; LANE_COUNT],
}
impl Board {
    /// Inspect a stack bottom to top, or return `None` for an invalid lane.
    pub fn stack(&self, lane: usize) -> Option<&[Ball]> {
        self.stacks.get(lane).map(Vec::as_slice)
    }

    /// Append a positive-weight ball to a valid lane; otherwise leave the board unchanged.
    pub fn place(&mut self, lane: usize, ball: Ball) -> bool {
        if ball.weight == 0 {
            return false;
        }
        let Some(stack) = self.stacks.get_mut(lane) else {
            return false;
        };
        stack.push(ball);
        true
    }

    /// Total weight of a lane; invalid lanes have zero total.
    pub fn total(&self, lane: usize) -> u64 {
        self.stack(lane)
            .unwrap_or_default()
            .iter()
            .map(|ball| u64::from(ball.weight))
            .sum()
    }

    /// Compare the two stacks belonging to a valid seesaw.
    pub fn balance(&self, seesaw: usize) -> Option<Balance> {
        if seesaw >= SEESAW_COUNT {
            return None;
        }
        Some(
            match self.total(seesaw * 2).cmp(&self.total(seesaw * 2 + 1)) {
                std::cmp::Ordering::Greater => Balance::LeftHeavy,
                std::cmp::Ordering::Equal => Balance::Balanced,
                std::cmp::Ordering::Less => Balance::RightHeavy,
            },
        )
    }

    /// Empty every stack while preserving allocated storage for repeated experiments.
    pub fn clear(&mut self) {
        for stack in &mut self.stacks {
            stack.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ball(weight: u32, color: BallColor) -> Ball {
        Ball { weight, color }
    }

    #[test]
    fn placement_preserves_order_weight_color_and_other_stacks() {
        let mut board = Board::default();
        let first = ball(2, BallColor::Red);
        let second = ball(3, BallColor::Blue);
        assert!(board.place(5, first));
        assert!(board.place(5, second));
        assert_eq!(board.stack(5), Some([first, second].as_slice()));
        assert_eq!(board.total(5), 5);
        for lane in [0, 1, 2, 3, 4, 6, 7] {
            assert_eq!(board.stack(lane), Some([].as_slice()));
        }
    }

    #[test]
    fn invalid_placement_is_atomic_and_indices_are_bounded() {
        let mut board = Board::default();
        assert!(board.place(0, ball(1, BallColor::Yellow)));
        let before = board.clone();
        assert!(!board.place(LANE_COUNT, ball(1, BallColor::Red)));
        assert!(!board.place(usize::MAX, ball(1, BallColor::Red)));
        assert!(!board.place(0, ball(0, BallColor::Blue)));
        assert_eq!(board, before);
        assert_eq!(board.stack(usize::MAX), None);
        assert_eq!(board.total(usize::MAX), 0);
        assert_eq!(board.balance(usize::MAX), None);
    }

    #[test]
    fn cited_weight_examples_only_change_balance_never_remove_balls() {
        for right_weights in [vec![1], vec![1, 1], vec![2], vec![3]] {
            let mut board = Board::default();
            assert!(board.place(0, ball(1, BallColor::Red)));
            for weight in &right_weights {
                assert!(board.place(1, ball(*weight, BallColor::Blue)));
            }
            assert_eq!(
                board.balance(0),
                Some(if right_weights == [1] {
                    Balance::Balanced
                } else {
                    Balance::RightHeavy
                })
            );
            assert_eq!(board.stack(0).map(<[Ball]>::len), Some(1));
            assert_eq!(board.stack(1).map(<[Ball]>::len), Some(right_weights.len()));
        }
    }

    #[test]
    fn balance_is_symmetric_and_totals_exceed_u32_without_wrapping() {
        let mut board = Board::default();
        assert!(board.place(2, ball(u32::MAX, BallColor::Red)));
        assert!(board.place(2, ball(u32::MAX, BallColor::Yellow)));
        assert_eq!(board.total(2), u64::from(u32::MAX) * 2);
        assert_eq!(board.balance(1), Some(Balance::LeftHeavy));
        for _ in 0..2 {
            assert!(board.place(3, ball(u32::MAX, BallColor::Blue)));
        }
        assert_eq!(board.balance(1), Some(Balance::Balanced));
        assert!(board.place(3, ball(1, BallColor::Red)));
        assert_eq!(board.balance(1), Some(Balance::RightHeavy));
        assert_eq!(board.balance(0), Some(Balance::Balanced));
    }

    #[test]
    fn clear_resets_every_stack_and_allows_another_experiment() {
        let mut board = Board::default();
        for lane in 0..LANE_COUNT {
            assert!(board.place(lane, ball(lane as u32 + 1, BallColor::Yellow)));
        }
        board.clear();
        assert_eq!(board, Board::default());
        for seesaw in 0..SEESAW_COUNT {
            assert_eq!(board.balance(seesaw), Some(Balance::Balanced));
        }
        assert!(board.place(7, ball(3, BallColor::Red)));
        assert_eq!(board.total(7), 3);
    }

    #[test]
    fn keyboard_cannot_leave_board() {
        let mut d = Dispenser::default();
        d.shift(-1);
        assert_eq!(d.lane(), 0);
        for _ in 0..20 {
            d.shift(1);
        }
        assert_eq!(d.lane(), 7);
        d.shift(-1);
        assert_eq!(d.lane(), 6);
    }
    #[test]
    fn pointer_boundaries_and_non_finite_values_are_safe() {
        let mut d = Dispenser::default();
        for (position, lane) in [(-1.0, 0), (0.125, 1), (0.999, 7), (1.0, 7), (2.0, 7)] {
            d.select_fraction(position);
            assert_eq!(d.lane(), lane);
        }
        d.select_fraction(f32::NAN);
        assert_eq!(d.lane(), 7);
    }
}
