//! One confirmed landing reaction on a single eight-lane board.
//!
//! A flight remains in transit until the caller lands its ball explicitly.
//! This module does not recursively resolve chains or route multiplayer boards.
use crate::{Balance, Ball, Board, LANE_COUNT};

/// A rejected landing leaves every stack unchanged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LandingError {
    /// No stack exists at the requested lane.
    InvalidLane,
    /// A zero-weight ball cannot be placed.
    ZeroWeight,
}

/// A ball removed from a rising stack by one full weight reversal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Flight {
    /// Top ball removed from the rising stack.
    pub ball: Ball,
    /// Lane the ball left.
    pub origin: usize,
    /// Landing lane after wrapping within this single board.
    pub destination: usize,
    /// Unwrapped number of lane steps, before removal of the launched ball.
    pub distance: u64,
}

impl Board {
    /// Land one ball and return at most one flight caused by a strict reversal.
    ///
    /// Only left-heavy to right-heavy, or the reverse, launches a ball. The top
    /// ball of the rising side travels toward the newly heavier side by the
    /// absolute difference of the final side totals, wrapping over eight lanes.
    /// Equal states do not trigger a launch. Removal itself triggers nothing.
    /// The caller owns an emitted ball until explicitly landing it again.
    pub fn land(&mut self, lane: usize, ball: Ball) -> Result<Option<Flight>, LandingError> {
        if lane >= LANE_COUNT {
            return Err(LandingError::InvalidLane);
        }
        if ball.weight == 0 {
            return Err(LandingError::ZeroWeight);
        }
        let seesaw = lane / 2;
        let before = self.balance(seesaw);
        self.stacks[lane].push(ball);
        let after = self.balance(seesaw);
        let rightward = match (before, after) {
            (Some(Balance::LeftHeavy), Some(Balance::RightHeavy)) => true,
            (Some(Balance::RightHeavy), Some(Balance::LeftHeavy)) => false,
            _ => return Ok(None),
        };
        let left = seesaw * 2;
        let distance = self.total(left).abs_diff(self.total(left + 1));
        let origin = if rightward { left } else { left + 1 };
        // A previously heavy stack necessarily contains a positive-weight ball.
        let Some(ball) = self.stacks[origin].pop() else {
            return Ok(None);
        };
        let steps = (distance % LANE_COUNT as u64) as usize;
        let destination = if rightward {
            (origin + steps) % LANE_COUNT
        } else {
            (origin + LANE_COUNT - steps) % LANE_COUNT
        };
        Ok(Some(Flight {
            ball,
            origin,
            destination,
            distance,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BallColors, ColorId};
    fn ball(weight: u32) -> Ball {
        Ball {
            weight,
            colors: BallColors::single(ColorId::new(weight)),
        }
    }
    fn board_weight(board: &Board) -> u64 {
        (0..LANE_COUNT).map(|lane| board.total(lane)).sum()
    }
    fn board_count(board: &Board) -> usize {
        (0..LANE_COUNT)
            .filter_map(|lane| board.stack(lane))
            .map(<[Ball]>::len)
            .sum()
    }
    #[test]
    fn explicit_two_flight_chain_conserves_balls_and_weight_at_every_step() {
        let mut board = Board::default();
        for (lane, weight) in [(0, 1), (0, 2), (1, 2), (4, 1), (5, 2)] {
            assert!(board.place(lane, ball(weight)));
        }
        let expected_weight = board_weight(&board) + 5;
        let expected_count = board_count(&board) + 1;
        let Ok(Some(first)) = board.land(1, ball(5)) else {
            panic!("first flight");
        };
        assert_eq!((first.origin, first.destination, first.distance), (0, 4, 4));
        assert_eq!(
            board_weight(&board) + u64::from(first.ball.weight),
            expected_weight
        );
        assert_eq!(board_count(&board) + 1, expected_count);
        let Ok(Some(second)) = board.land(first.destination, first.ball) else {
            panic!("second flight");
        };
        assert_eq!(
            (second.origin, second.destination, second.distance),
            (5, 4, 1)
        );
        assert_eq!(
            board_weight(&board) + u64::from(second.ball.weight),
            expected_weight
        );
        assert_eq!(board_count(&board) + 1, expected_count);
        assert_eq!(board.land(second.destination, second.ball), Ok(None));
        assert_eq!(board_weight(&board), expected_weight);
        assert_eq!(board_count(&board), expected_count);
        assert_eq!(board.total(0), 1);
        assert_eq!(board.total(1), 7);
        assert_eq!(board.total(4), 5);
        assert_eq!(board.total(5), 0);
    }
    #[test]
    fn totals_above_u32_keep_exact_difference_and_maximum_possible_flight() {
        let mut board = Board::default();
        assert!(board.place(0, ball(u32::MAX)));
        assert!(board.place(0, ball(2)));
        assert!(board.place(1, ball(u32::MAX)));
        assert!(board.place(1, ball(1)));
        let before = board_weight(&board);
        let Ok(Some(flight)) = board.land(1, ball(u32::MAX)) else {
            panic!("strict reversal");
        };
        // Distance is arrival minus the previous lead; a positive lead means
        // no single u32 arrival can produce distance greater than u32::MAX-1.
        assert_eq!(flight.distance, u64::from(u32::MAX) - 1);
        assert_eq!(
            flight.destination,
            (flight.distance % LANE_COUNT as u64) as usize
        );
        assert_eq!(flight.ball, ball(2));
        assert!(board.total(1) > u64::from(u32::MAX));
        assert_eq!(
            board_weight(&board) + u64::from(flight.ball.weight),
            before + u64::from(u32::MAX)
        );
    }

    #[test]
    fn confirmed_example_launches_only_top_ball_after_full_reversal() {
        let mut board = Board::default();
        assert!(board.place(0, ball(2)));
        assert!(board.place(0, ball(1)));
        assert!(board.place(1, ball(2)));
        let Ok(Some(flight)) = board.land(1, ball(2)) else {
            panic!("full reversal");
        };
        assert_eq!(
            flight,
            Flight {
                ball: ball(1),
                origin: 0,
                destination: 1,
                distance: 1
            }
        );
        assert_eq!(board.stack(0), Some([ball(2)].as_slice()));
        assert_eq!(board.stack(1), Some([ball(2), ball(2)].as_slice()));
    }
    #[test]
    fn balanced_transitions_and_same_heavy_side_never_launch() {
        let mut board = Board::default();
        assert_eq!(board.land(0, ball(1)), Ok(None));
        assert_eq!(board.land(1, ball(1)), Ok(None));
        assert_eq!(board.land(1, ball(1)), Ok(None));
        assert_eq!(board.land(1, ball(3)), Ok(None));
        assert_eq!(board.stack(0).map(<[Ball]>::len), Some(1));
        assert_eq!(board.total(1), 5);
    }
    #[test]
    fn mirrored_flight_wraps_left_and_can_be_landed_explicitly() {
        let mut board = Board::default();
        assert!(board.place(1, ball(3)));
        assert!(board.place(0, ball(2)));
        let Ok(Some(flight)) = board.land(0, ball(3)) else {
            panic!("full reversal");
        };
        assert_eq!(flight.origin, 1);
        assert_eq!(flight.distance, 2);
        assert_eq!(flight.destination, 7);
        assert_eq!(board.total(1), 0);
        assert_eq!(board.total(7), 0);
        assert_eq!(board.land(flight.destination, flight.ball), Ok(None));
        assert_eq!(board.total(7), 3);
    }
    #[test]
    fn long_flights_keep_distance_and_wrap_in_both_directions() {
        for (origin, target, expected) in [(6, 7, 0), (1, 0, 7)] {
            let mut board = Board::default();
            assert!(board.place(origin, ball(1)));
            let Ok(Some(flight)) = board.land(target, ball(11)) else {
                panic!("full reversal");
            };
            assert_eq!(flight.distance, 10);
            assert_eq!(flight.destination, expected);
        }
        let mut board = Board::default();
        assert!(board.place(0, ball(1)));
        let Ok(Some(flight)) = board.land(1, ball(9)) else {
            panic!("full reversal");
        };
        assert_eq!(flight.distance, 8);
        assert_eq!(flight.destination, flight.origin);
    }
    #[test]
    fn invalid_landing_is_atomic_and_removal_has_no_second_reaction() {
        let mut board = Board::default();
        assert!(board.place(0, ball(3)));
        let before = board.clone();
        assert_eq!(
            board.land(usize::MAX, ball(1)),
            Err(LandingError::InvalidLane)
        );
        assert_eq!(board.land(1, ball(0)), Err(LandingError::ZeroWeight));
        assert_eq!(board, before);
        assert!(board.place(1, ball(2)));
        let Ok(Some(flight)) = board.land(1, ball(2)) else {
            panic!("full reversal");
        };
        assert_eq!(flight.ball.weight, 3);
        assert_eq!(board.total(0), 0);
        assert_eq!(board.total(1), 4);
    }
}
