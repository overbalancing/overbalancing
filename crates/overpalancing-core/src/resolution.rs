//! Transactional compression, joker cascades, scoring and seesaw reactions.
use crate::{
    Balance, Ball, Board, LANE_COUNT, SEESAW_COUNT, matching,
    scoring::{self, Progression, ScoreError},
};

/// Rejected transactions leave both board and progression unchanged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolutionError {
    /// No lane exists at this index.
    InvalidLane,
    /// Landing requires a positive ball weight.
    ZeroWeight,
    /// Five compressed weights cannot be represented by a ball's u32 weight.
    WeightOverflow,
    /// Points or progression cannot be represented.
    Score(ScoreError),
}
/// One owned emitted ball, with its historical launch position.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedFlight {
    /// Ball removed from the rising stack.
    pub ball: Ball,
    /// Origin lane.
    pub origin: usize,
    /// Wrapped destination on this eight-lane board.
    pub destination: usize,
    /// Full difference of the side totals before launching.
    pub distance: u64,
    /// Bottom-to-top origin index before removal.
    pub origin_index: usize,
    /// Origin capacity after the reversal and before removal.
    pub origin_capacity: usize,
}
/// All committed effects of one explicit landing; flights are not landed recursively.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Outcome {
    /// Weight-based points awarded by clearing, excluding compression.
    pub points: u64,
    /// Number of individual balls cleared.
    pub cleared_count: usize,
    /// Number of five-to-one compression operations.
    pub compressed_count: usize,
    /// Owned in-transit balls, ordered by resolution phase then seesaw index.
    pub flights: Vec<ResolvedFlight>,
    /// Overflow after all local resolution and launch removals.
    pub game_over: bool,
}
/// Marked balls remain visible and scoreless until an explicit clear commit.
/// Membership survives platform movement and is remapped through compression;
/// launched balls leave the pending board group and remain owned by their flight.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PendingClear {
    cells: Vec<matching::Cell>,
}
impl PendingClear {
    /// Current bottom-to-top identities; read again after every prepare or commit.
    pub fn cells(&self) -> &[matching::Cell] {
        &self.cells
    }
    /// Whether there is no pending clear animation.
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
    fn compressed(&mut self, lane: usize, start: usize) {
        for cell in &mut self.cells {
            if cell.lane == lane && cell.index >= start {
                cell.index = if cell.index < start + 5 {
                    start
                } else {
                    cell.index - 4
                };
            }
        }
        self.cells.sort_unstable();
        self.cells.dedup();
    }
}
fn empty_outcome() -> Outcome {
    Outcome {
        points: 0,
        cleared_count: 0,
        compressed_count: 0,
        flights: Vec::new(),
        game_over: false,
    }
}
fn compress(board: &mut Board, pending: &mut PendingClear) -> Result<usize, ResolutionError> {
    let mut count = 0;
    for (lane, stack) in board.stacks.iter_mut().enumerate() {
        let mut index = 0;
        while index + 5 <= stack.len() {
            let colors = stack[index].colors.clone();
            if colors.count() != 1
                || !stack[index + 1..index + 5]
                    .iter()
                    .all(|ball| ball.colors == colors)
            {
                index += 1;
                continue;
            }
            let weight = stack[index..index + 5]
                .iter()
                .try_fold(0u32, |sum, ball| sum.checked_add(ball.weight))
                .ok_or(ResolutionError::WeightOverflow)?;
            stack.splice(index..index + 5, [Ball { weight, colors }]);
            pending.compressed(lane, index);
            count += 1;
            index = 0;
        }
    }
    Ok(count)
}
fn balances(board: &Board) -> [Option<Balance>; SEESAW_COUNT] {
    std::array::from_fn(|seesaw| board.balance(seesaw))
}
fn launch_reversals(
    board: &mut Board,
    before: [Option<Balance>; SEESAW_COUNT],
    flights: &mut Vec<ResolvedFlight>,
    pending: &mut PendingClear,
) {
    for (seesaw, old) in before.into_iter().enumerate() {
        let rightward = match (old, board.balance(seesaw)) {
            (Some(Balance::LeftHeavy), Some(Balance::RightHeavy)) => true,
            (Some(Balance::RightHeavy), Some(Balance::LeftHeavy)) => false,
            _ => continue,
        };
        let left = seesaw * 2;
        let origin = left + usize::from(!rightward);
        let distance = board.total(left).abs_diff(board.total(left + 1));
        let origin_capacity = board.capacity(origin).unwrap_or(8);
        let Some(origin_index) = board.stacks[origin].len().checked_sub(1) else {
            continue;
        };
        let Some(ball) = board.stacks[origin].pop() else {
            continue;
        };
        pending
            .cells
            .retain(|cell| !(cell.lane == origin && cell.index == origin_index));
        let steps = (distance % LANE_COUNT as u64) as usize;
        let destination = if rightward {
            (origin + steps) % LANE_COUNT
        } else {
            (origin + LANE_COUNT - steps) % LANE_COUNT
        };
        flights.push(ResolvedFlight {
            ball,
            origin,
            destination,
            distance,
            origin_index,
            origin_capacity,
        });
    }
}
fn prepare_phase(
    board: &mut Board,
    pending: &mut PendingClear,
    before: [Option<Balance>; SEESAW_COUNT],
) -> Result<Outcome, ResolutionError> {
    let mut outcome = empty_outcome();
    outcome.compressed_count = compress(board, pending)?;
    launch_reversals(board, before, &mut outcome.flights, pending);
    let geometry = matching::capacities(board);
    let mut seeds = pending.cells.clone();
    seeds.extend(matching::cascade_at(board, &geometry));
    pending.cells = matching::expand_at(board, &geometry, &seeds);
    outcome.game_over = pending.is_empty() && board.is_game_over();
    Ok(outcome)
}
/// Prepare a real landing while keeping marked clearing balls present and scoreless.
/// New arrivals may expand an already marked group without requiring a new horizontal
/// seed. Compression remaps identities and launch removal removes only its exact mark.
/// The caller controls a fixed animation deadline; this core owns no clock.
pub fn prepare_landing(
    board: &mut Board,
    pending: &mut PendingClear,
    lane: usize,
    ball: Ball,
) -> Result<Outcome, ResolutionError> {
    if lane >= LANE_COUNT {
        return Err(ResolutionError::InvalidLane);
    }
    if ball.weight == 0 {
        return Err(ResolutionError::ZeroWeight);
    }
    let mut candidate = board.clone();
    let mut next_pending = pending.clone();
    let before = balances(&candidate);
    candidate.stacks[lane].push(ball);
    let outcome = prepare_phase(&mut candidate, &mut next_pending, before)?;
    *board = candidate;
    *pending = next_pending;
    Ok(outcome)
}
/// Commit marked removals and points atomically, then prepare the next clear phase.
/// Sticky membership uses identities maintained only by this API; callers must not
/// mutate board stacks between preparation and commit except via prepare_landing.
pub fn commit_clear(
    board: &mut Board,
    progression: &mut Progression,
    pending: &mut PendingClear,
) -> Result<Outcome, ResolutionError> {
    if pending.is_empty() {
        let mut outcome = empty_outcome();
        outcome.game_over = board.is_game_over();
        return Ok(outcome);
    }
    let mut candidate = board.clone();
    let mut next_progression = progression.clone();
    let mut next_pending = pending.clone();
    let before = balances(&candidate);
    let points =
        scoring::score_cells(&candidate, &next_pending.cells).map_err(ResolutionError::Score)?;
    next_progression
        .award(points)
        .map_err(ResolutionError::Score)?;
    let cleared_count = next_pending.cells.len();
    for cell in next_pending.cells.drain(..).rev() {
        candidate.stacks[cell.lane].remove(cell.index);
    }
    let mut outcome = prepare_phase(&mut candidate, &mut next_pending, before)?;
    outcome.points = points;
    outcome.cleared_count = cleared_count;
    *board = candidate;
    *progression = next_progression;
    *pending = next_pending;
    Ok(outcome)
}
/// Synchronous compatibility transaction, committing each prepared clear phase.
/// Animated applications use prepare_landing and commit_clear separately instead.
pub fn land(
    board: &mut Board,
    progression: &mut Progression,
    lane: usize,
    ball: Ball,
) -> Result<Outcome, ResolutionError> {
    let mut candidate = board.clone();
    let mut next_progression = progression.clone();
    let mut pending = PendingClear::default();
    let mut outcome = prepare_landing(&mut candidate, &mut pending, lane, ball)?;
    while !pending.is_empty() {
        let next = commit_clear(&mut candidate, &mut next_progression, &mut pending)?;
        outcome.points = outcome
            .points
            .checked_add(next.points)
            .ok_or(ResolutionError::Score(ScoreError::Overflow))?;
        outcome.cleared_count += next.cleared_count;
        outcome.compressed_count += next.compressed_count;
        outcome.flights.extend(next.flights);
        outcome.game_over = next.game_over;
    }
    *board = candidate;
    *progression = next_progression;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BallColors, ColorId};
    use std::num::NonZeroU32;
    fn ball(weight: u32, ids: &[u32]) -> Ball {
        let colors: Vec<_> = ids.iter().copied().map(ColorId::new).collect();
        let Some(colors) = BallColors::new(&colors) else {
            panic!("valid colors");
        };
        Ball { weight, colors }
    }
    fn progress() -> Progression {
        Progression::new(NonZeroU32::MIN)
    }
    fn weight(board: &Board) -> u64 {
        (0..LANE_COUNT).map(|lane| board.total(lane)).sum()
    }
    fn marked_four() -> (Board, PendingClear) {
        let mut board = Board::default();
        let mut pending = PendingClear::default();
        for lane in 0..4 {
            assert!(prepare_landing(&mut board, &mut pending, lane, ball(1, &[0])).is_ok());
        }
        assert_eq!(pending.cells().len(), 4);
        (board, pending)
    }
    #[test]
    fn every_two_or_three_color_joker_position_breaks_compression() {
        for joker_colors in [&[0, 1][..], &[0, 1, 2]] {
            for joker_index in 0..5 {
                let mut board = Board::default();
                for index in 0..4 {
                    assert!(board.place(
                        0,
                        ball(
                            1,
                            if index == joker_index {
                                joker_colors
                            } else {
                                &[0]
                            }
                        )
                    ));
                }
                let mut pending = PendingClear::default();
                let Ok(outcome) = prepare_landing(
                    &mut board,
                    &mut pending,
                    0,
                    ball(1, if joker_index == 4 { joker_colors } else { &[0] }),
                ) else {
                    panic!("valid landing");
                };
                assert_eq!(outcome.compressed_count, 0);
                assert_eq!(board.stack(0).map(<[Ball]>::len), Some(5));
                assert_eq!(board.total(0), 5);
                assert!(pending.is_empty());
            }
        }
        let mut board = Board::default();
        for _ in 0..4 {
            assert!(board.place(0, ball(1, &[0, 1])));
        }
        let Ok(outcome) = land(&mut board, &mut progress(), 0, ball(1, &[0, 1])) else {
            panic!("valid landing");
        };
        assert_eq!(outcome.compressed_count, 0);
        assert_eq!(board.stack(0).map(<[Ball]>::len), Some(5));
    }
    #[test]
    fn resolved_flights_wrap_outside_each_board_edge_in_both_directions() {
        for (origin, landing, destination) in [(6, 7, 0), (1, 0, 7)] {
            let mut board = Board::default();
            let mut pending = PendingClear::default();
            assert!(board.place(origin, ball(1, &[0])));
            let Ok(outcome) = prepare_landing(&mut board, &mut pending, landing, ball(3, &[1]))
            else {
                panic!("strict flip");
            };
            assert_eq!(outcome.flights.len(), 1);
            let flight = &outcome.flights[0];
            assert_eq!(
                (flight.origin, flight.destination, flight.distance),
                (origin, destination, 2)
            );
            assert_eq!(flight.ball.weight, 1);
            assert_eq!(weight(&board) + u64::from(flight.ball.weight), 4);
        }
    }

    #[test]
    fn marked_balls_stay_visible_scoreless_and_accept_followup_despite_new_tilt() {
        let (mut board, mut pending) = marked_four();
        let mut progression = progress();
        assert_eq!(weight(&board), 4);
        assert_eq!(progression.score(), 0);
        let Ok(prepared) = prepare_landing(&mut board, &mut pending, 0, ball(1, &[0])) else {
            panic!("followup");
        };
        assert_eq!(prepared.points, 0);
        assert_eq!(prepared.cleared_count, 0);
        assert_eq!(pending.cells().len(), 5);
        assert_eq!(weight(&board), 5);
        // The original horizontal seed is gone after this tilt, but committed
        // animation membership is retained and its vertical neighbor joins.
        assert!(matching::cascade_cells(&board).is_empty());
        let Ok(committed) = commit_clear(&mut board, &mut progression, &mut pending) else {
            panic!("commit");
        };
        assert_eq!(committed.points, 500);
        assert_eq!(committed.cleared_count, 5);
        assert_eq!(progression.score(), 500);
        assert_eq!(weight(&board), 0);
        assert!(pending.is_empty());
        assert_eq!(
            commit_clear(&mut board, &mut progression, &mut pending).map(|outcome| outcome.points),
            Ok(0)
        );
        assert_eq!(progression.score(), 500);
    }
    #[test]
    fn compression_remaps_marked_cells_and_never_clears_a_stale_index() {
        let (mut board, mut pending) = marked_four();
        let mut progression = progress();
        let mut compressed = 0;
        for _ in 0..4 {
            let Ok(outcome) = prepare_landing(&mut board, &mut pending, 0, ball(1, &[0])) else {
                panic!("followup");
            };
            compressed += outcome.compressed_count;
        }
        assert_eq!(compressed, 1);
        assert_eq!(pending.cells().len(), 4);
        assert_eq!(board.stack(0), Some([ball(5, &[0])].as_slice()));
        assert!(pending.cells().iter().all(|cell| {
            board
                .stack(cell.lane)
                .is_some_and(|stack| cell.index < stack.len())
        }));
        let Ok(outcome) = commit_clear(&mut board, &mut progression, &mut pending) else {
            panic!("commit");
        };
        assert_eq!(outcome.points, 800);
        assert_eq!(outcome.cleared_count, 4);
        assert_eq!(weight(&board), 0);
    }
    #[test]
    fn emitting_a_marked_top_ball_removes_only_its_mark_and_preserves_ownership() {
        let (mut board, mut pending) = marked_four();
        let mut progression = progress();
        assert!(prepare_landing(&mut board, &mut pending, 0, ball(1, &[0])).is_ok());
        let Ok(outcome) = prepare_landing(&mut board, &mut pending, 1, ball(3, &[0])) else {
            panic!("followup");
        };
        assert_eq!(outcome.flights.len(), 1);
        assert_eq!(outcome.flights[0].origin_index, 1);
        assert!(
            !pending
                .cells()
                .contains(&matching::Cell { lane: 0, index: 1 })
        );
        assert_eq!(
            weight(&board) + u64::from(outcome.flights[0].ball.weight),
            8
        );
        let Ok(clear) = commit_clear(&mut board, &mut progression, &mut pending) else {
            panic!("commit");
        };
        assert_eq!(clear.points, 700);
        assert_eq!(weight(&board), 0);
    }
    #[test]
    fn failed_prepare_and_commit_preserve_pending_membership_and_state() {
        let (mut board, mut pending) = marked_four();
        let before = board.clone();
        let marks = pending.clone();
        assert_eq!(
            prepare_landing(&mut board, &mut pending, 9, ball(1, &[0])),
            Err(ResolutionError::InvalidLane)
        );
        assert_eq!(board, before);
        assert_eq!(pending, marks);
        let mut progression = progress();
        assert_eq!(progression.award(u64::MAX), Ok(()));
        let score = progression.clone();
        assert_eq!(
            commit_clear(&mut board, &mut progression, &mut pending),
            Err(ResolutionError::Score(ScoreError::Overflow))
        );
        assert_eq!(board, before);
        assert_eq!(pending, marks);
        assert_eq!(progression, score);
        // A different-color followup does not automatically enter a sticky group.
        assert!(prepare_landing(&mut board, &mut pending, 0, ball(1, &[99])).is_ok());
        assert_eq!(pending.cells().len(), 4);
    }

    #[test]
    fn landing_on_a_balanced_beam_uses_its_new_tilt_before_matching() {
        let mut board = Board::default();
        let mut progression = progress();
        // First pair is balanced on physical row one. The third arrival lowers
        // lane two to row zero, so these three equal colors cannot clear yet.
        for lane in 0..3 {
            let Ok(outcome) = land(&mut board, &mut progression, lane, ball(1, &[0])) else {
                panic!("valid landing");
            };
            assert_eq!(outcome.cleared_count, 0);
            assert_eq!(outcome.points, 0);
        }
        assert_eq!(board.capacity(2), Some(9));
        assert_eq!(progression.score(), 0);
        // The fourth arrival balances pair two: all four balls now share row one.
        let Ok(outcome) = land(&mut board, &mut progression, 3, ball(1, &[0])) else {
            panic!("valid landing");
        };
        assert_eq!(outcome.cleared_count, 4);
        assert_eq!(outcome.points, 400);
        assert_eq!(weight(&board), 0);
        assert!(outcome.flights.is_empty());
    }

    #[test]
    fn new_alignment_after_a_launch_exposes_and_resolves_another_seed() {
        let mut board = Board::default();
        for ball in [ball(1, &[0]), ball(3, &[40])] {
            assert!(board.place(0, ball));
        }
        for ball in [ball(1, &[10]), ball(1, &[10]), ball(1, &[0])] {
            assert!(board.place(1, ball));
        }
        for ball in [ball(1, &[20]), ball(1, &[0])] {
            assert!(board.place(2, ball));
        }
        assert!(board.place(3, ball(2, &[30])));
        assert!(matching::cascade_cells(&board).is_empty());
        let initial = weight(&board);
        let Ok(outcome) = land(&mut board, &mut progress(), 1, ball(2, &[50])) else {
            panic!("valid resolution");
        };
        assert_eq!(outcome.flights.len(), 1);
        assert_eq!(outcome.flights[0].ball.weight, 3);
        assert_eq!(outcome.cleared_count, 3);
        assert_eq!(outcome.points, 300);
        assert_eq!(weight(&board) + 3 + outcome.points / 100, initial + 2);
    }

    #[test]
    fn gravity_exposes_a_new_horizontal_seed_and_repeats_scoring() {
        let mut board = Board::default();
        for ball in [ball(1, &[0]), ball(1, &[0]), ball(1, &[1])] {
            assert!(board.place(0, ball));
        }
        for lane in [1, 2] {
            assert!(board.place(lane, ball(2, &[0])));
            assert!(board.place(lane, ball(1, &[1])));
        }
        assert!(board.place(3, ball(2, &[0])));
        assert!(board.place(3, ball(1, &[20])));
        let initial = weight(&board);
        let Ok(outcome) = land(&mut board, &mut progress(), 7, ball(1, &[99])) else {
            panic!("valid resolution");
        };
        assert_eq!(outcome.cleared_count, 8);
        assert_eq!(outcome.points, 1100);
        assert_eq!(board.total(0), 0);
        assert_eq!(board.total(1), 0);
        assert_eq!(board.total(2), 0);
        assert_eq!(weight(&board) + outcome.points / 100, initial + 1);
    }

    #[test]
    fn combined_review_fixture_compresses_and_cascades_in_one_landing() {
        let mut board = Board::default();
        for (lane, ids) in [
            (0, vec![0]),
            (1, vec![0]),
            (2, vec![0, 1]),
            (3, vec![1]),
            (4, vec![1, 2]),
            (5, vec![2]),
        ] {
            assert!(board.place(lane, ball(1, &ids)));
        }
        for _ in 0..4 {
            assert!(board.place(6, ball(1, &[2])));
        }
        let Ok(outcome) = land(&mut board, &mut progress(), 6, ball(2, &[2])) else {
            panic!("review fixture");
        };
        assert_eq!(outcome.compressed_count, 1);
        assert_eq!(outcome.cleared_count, 6);
        assert_eq!(outcome.points, 600);
        assert!(outcome.flights.is_empty());
        assert_eq!(board.stack(6), Some([ball(6, &[2])].as_slice()));
        assert_eq!(weight(&board), 6);
    }

    #[test]
    fn one_clear_can_reverse_multiple_seesaws_and_emit_every_owned_ball() {
        let mut board = Board::default();
        for (left, identity) in [(0, 10), (2, 20)] {
            assert!(board.place(left, ball(1, &[identity])));
            assert!(board.place(left, ball(1, &[identity])));
            assert!(board.place(left, ball(5, &[0])));
            assert!(board.place(left + 1, ball(1, &[0])));
            assert!(board.place(left + 1, ball(5, &[identity + 1])));
        }
        let initial_weight = weight(&board);
        let Ok(outcome) = land(&mut board, &mut progress(), 7, ball(1, &[99])) else {
            panic!("valid resolution");
        };
        assert_eq!(outcome.cleared_count, 4);
        assert_eq!(outcome.points, 1200);
        assert_eq!(outcome.flights.len(), 2);
        assert_eq!(
            (
                outcome.flights[0].origin,
                outcome.flights[0].destination,
                outcome.flights[0].distance
            ),
            (0, 3, 3)
        );
        assert_eq!(
            (
                outcome.flights[1].origin,
                outcome.flights[1].destination,
                outcome.flights[1].distance
            ),
            (2, 5, 3)
        );
        assert_eq!(
            weight(&board)
                + outcome
                    .flights
                    .iter()
                    .map(|flight| u64::from(flight.ball.weight))
                    .sum::<u64>()
                + outcome.points / 100,
            initial_weight + 1
        );
    }

    #[test]
    fn five_single_color_balls_compress_before_flight_and_preserve_mass() {
        let mut board = Board::default();
        for ids in [&[0][..], &[0], &[0], &[0]] {
            assert!(board.place(0, ball(1, ids)));
        }
        let mut progression = progress();
        let Ok(outcome) = land(&mut board, &mut progression, 0, ball(1, &[0])) else {
            panic!("valid resolution");
        };
        assert_eq!(outcome.compressed_count, 1);
        assert_eq!(outcome.points, 0);
        assert!(outcome.flights.is_empty());
        assert_eq!(board.stack(0), Some([ball(5, &[0])].as_slice()));
        assert_eq!(weight(&board), 5);
    }
    #[test]
    fn compression_repeats_bottom_up_until_no_five_remain() {
        let mut board = Board::default();
        for _ in 0..9 {
            assert!(board.place(0, ball(1, &[0])));
        }
        let Ok(outcome) = land(&mut board, &mut progress(), 0, ball(1, &[0])) else {
            panic!("valid resolution");
        };
        assert_eq!(outcome.compressed_count, 2);
        assert_eq!(board.total(0), 10);
        assert_eq!(
            board.stack(0),
            Some([ball(9, &[0]), ball(1, &[0])].as_slice())
        );
    }
    #[test]
    fn joker_cascade_changes_color_and_scores_every_ball_once() {
        let mut board = Board::default();
        for (lane, ids) in [
            (0, vec![0]),
            (1, vec![0]),
            (2, vec![0, 1]),
            (3, vec![1]),
            (4, vec![1, 2]),
            (5, vec![2]),
            (6, vec![9]),
        ] {
            assert!(board.place(lane, ball(1, &ids)));
        }
        let mut progression = progress();
        let Ok(outcome) = land(&mut board, &mut progression, 7, ball(1, &[9])) else {
            panic!("valid resolution");
        };
        assert_eq!(outcome.cleared_count, 6);
        assert_eq!(outcome.points, 600);
        assert_eq!(progression.score(), 600);
        assert_eq!(weight(&board), 2);
        assert_eq!(board.total(6), 1);
        assert_eq!(board.total(7), 1);
        let mut no_seed = Board::default();
        for (lane, ids) in [(0, vec![0]), (1, vec![0, 1]), (2, vec![1]), (3, vec![2])] {
            assert!(no_seed.place(lane, ball(1, &ids)));
        }
        assert!(matching::cascade_cells(&no_seed).is_empty());
    }
    #[test]
    fn compression_precedes_clear_and_can_suppress_an_old_horizontal_seed() {
        let mut board = Board::default();
        for _ in 0..4 {
            assert!(board.place(0, ball(1, &[0])));
        }
        assert!(board.place(1, ball(1, &[0])));
        assert!(board.place(2, ball(1, &[8])));
        assert!(board.place(2, ball(1, &[8])));
        assert!(board.place(2, ball(1, &[0])));
        let mut uncompressed = board.clone();
        assert!(uncompressed.place(0, ball(1, &[0])));
        assert!(!matching::cascade_cells(&uncompressed).is_empty());
        let mut progression = progress();
        let Ok(outcome) = land(&mut board, &mut progression, 0, ball(1, &[0])) else {
            panic!("valid resolution");
        };
        assert_eq!(outcome.compressed_count, 1);
        assert_eq!(outcome.cleared_count, 0);
        assert_eq!(progression.score(), 0);
    }
    #[test]
    fn clearing_before_overflow_can_save_a_temporarily_full_stack() {
        let mut board = Board::default();
        for lane in 0..4 {
            for index in 0..9 {
                // Alternate colors vertically to avoid five-ball compression.
                assert!(board.place(lane, ball(1, &[if index % 2 == 0 { 0 } else { 1 }])));
            }
        }
        assert!(board.is_game_over());
        let Ok(outcome) = land(&mut board, &mut progress(), 7, ball(1, &[20])) else {
            panic!("valid resolution");
        };
        assert!(outcome.cleared_count >= 36);
        assert!(!outcome.game_over);
        assert_eq!(board.total(7), 1);
    }
    #[test]
    fn compression_and_score_errors_roll_back_entire_transaction() {
        let mut board = Board::default();
        for _ in 0..4 {
            assert!(board.place(0, ball(u32::MAX, &[0])));
        }
        let before = board.clone();
        let mut progression = progress();
        let before_score = progression.clone();
        assert_eq!(
            land(&mut board, &mut progression, 0, ball(u32::MAX, &[0])),
            Err(ResolutionError::WeightOverflow)
        );
        assert_eq!(board, before);
        assert_eq!(progression, before_score);
        let mut board = Board::default();
        for lane in 0..3 {
            assert!(board.place(lane, ball(1, &[0])));
        }
        let before = board.clone();
        assert_eq!(progression.award(u64::MAX), Ok(()));
        let before_score = progression.clone();
        assert_eq!(
            land(&mut board, &mut progression, 3, ball(1, &[0])),
            Err(ResolutionError::Score(ScoreError::Overflow))
        );
        assert_eq!(board, before);
        assert_eq!(progression, before_score);
        assert_eq!(
            land(&mut board, &mut progression, 8, ball(1, &[0])),
            Err(ResolutionError::InvalidLane)
        );
        assert_eq!(
            land(&mut board, &mut progression, 0, ball(0, &[0])),
            Err(ResolutionError::ZeroWeight)
        );
        assert_eq!(board, before);
    }
    #[test]
    fn plain_flip_preserves_board_plus_owned_flight_and_snapshot() {
        let mut board = Board::default();
        assert!(board.place(0, ball(1, &[0])));
        assert!(board.place(0, ball(2, &[2])));
        assert!(board.place(1, ball(2, &[1])));
        let initial = weight(&board);
        let Ok(outcome) = land(&mut board, &mut progress(), 1, ball(5, &[3])) else {
            panic!("valid resolution");
        };
        assert_eq!(outcome.flights.len(), 1);
        let flight = &outcome.flights[0];
        assert_eq!(
            (
                flight.origin,
                flight.destination,
                flight.distance,
                flight.origin_index,
                flight.origin_capacity
            ),
            (0, 4, 4, 1, 7)
        );
        assert_eq!(
            weight(&board)
                + outcome
                    .flights
                    .iter()
                    .map(|flight| u64::from(flight.ball.weight))
                    .sum::<u64>()
                + outcome.points / scoring::POINTS_PER_WEIGHT,
            initial + 5
        );
    }
}
