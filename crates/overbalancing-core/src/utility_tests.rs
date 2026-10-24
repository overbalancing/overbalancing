//! Utility domain scenarios: rewards, direct contacts, snapshot areas and rollback.
use super::*;
use crate::{BallColors, ColorId};
use std::num::NonZeroU32;
fn normal(weight: u32, color: u32) -> Ball {
    Ball::normal(weight, BallColors::single(ColorId::new(color)))
}
fn progress() -> Progression {
    Progression::new(NonZeroU32::MIN)
}
fn apply(board: &mut Board, score: &mut Progression, lane: usize, ball: Ball) -> Outcome {
    land(board, score, lane, ball).unwrap_or_else(|error| panic!("{error:?}"))
}
fn reward_count(rewards: &[Reward], kind: BallKind) -> u32 {
    rewards
        .iter()
        .filter(|reward| reward.ball.kind == kind)
        .map(|reward| reward.count)
        .sum()
}
fn selected(board: &Board) -> Vec<matching::Cell> {
    all_cells(board)
}
fn reward_fixture(lanes: usize, height: usize) -> Board {
    let mut board = Board::default();
    for lane in 0..lanes {
        for _ in 0..height {
            board.place(lane, normal(1, 0));
        }
    }
    board
}
fn activation_fixture() -> Board {
    let mut board = Board::default();
    for (lane, ball) in [
        (0, Ball::stone()),
        (0, normal(2, 1)),
        (1, normal(7, 2)),
        (2, normal(1, 3)),
        (2, normal(1, 4)),
        (2, Ball::stone()),
        (3, normal(7, 5)),
    ] {
        board.place(lane, ball);
    }
    board
}
#[test]
fn horizontal_four_vs_five_and_joker_overlap_have_one_matching_run_reward() {
    for (lanes, expected) in [(4, 0), (5, 1), (8, 1)] {
        let mut board = reward_fixture(lanes, 1);
        if lanes % 2 == 1 {
            board.place(lanes, normal(1, 9));
        }
        let cells: Vec<_> = selected(&board)
            .into_iter()
            .filter(|cell| cell.lane < lanes)
            .collect();
        let rewards = clear_rewards(&board, &cells, 6).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(reward_count(&rewards, BallKind::RowLaser), expected);
        assert!(rewards.iter().all(|reward| reward.lane == 6));
    }
    let mut board = Board::default();
    for lane in 0..6 {
        board.place(
            lane,
            Ball::normal(
                1,
                BallColors::new(&[ColorId::RED, ColorId::BLUE])
                    .unwrap_or_else(|| panic!("palette")),
            ),
        );
    }
    let mut cells = selected(&board);
    cells.extend(selected(&board));
    let rewards = clear_rewards(&board, &cells, 0).unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(reward_count(&rewards, BallKind::RowLaser), 1);
}
#[test]
fn touching_unrelated_color_clears_do_not_combine_into_row_reward() {
    let mut board = Board::default();
    for lane in 0..6 {
        board.place(lane, normal(1, u32::from(lane >= 3)));
    }
    let rewards =
        clear_rewards(&board, &selected(&board), 0).unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(reward_count(&rewards, BallKind::RowLaser), 0);
    let joker = Ball::normal(
        1,
        BallColors::new(&[ColorId::RED, ColorId::BLUE]).unwrap_or_else(|| panic!("palette")),
    );
    board.stacks[2][0] = joker;
    let rewards =
        clear_rewards(&board, &selected(&board), 0).unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(reward_count(&rewards, BallKind::RowLaser), 1);
}
#[test]
fn column_reward_requires_three_adjacent_removed_four_runs_not_stack_height() {
    for (lanes, expected) in [(2, 0), (3, 1), (4, 1)] {
        let board = reward_fixture(lanes, 4);
        let rewards =
            clear_rewards(&board, &selected(&board), 1).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(reward_count(&rewards, BallKind::ColumnLaser), expected);
    }
    let board = reward_fixture(4, 5);
    let without_middle: Vec<_> = selected(&board)
        .into_iter()
        .filter(|cell| cell.lane != 1)
        .collect();
    let rewards =
        clear_rewards(&board, &without_middle, 0).unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(reward_count(&rewards, BallKind::ColumnLaser), 0);
    let incomplete: Vec<_> = selected(&board)
        .into_iter()
        .filter(|cell| cell.index != 2)
        .collect();
    let rewards = clear_rewards(&board, &incomplete, 0).unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(reward_count(&rewards, BallKind::ColumnLaser), 0);
}
#[test]
fn feather_reward_uses_all_affected_preclear_balances_and_nonempty_selection() {
    let mut board = reward_fixture(4, 1);
    assert_eq!(
        reward_count(
            &clear_rewards(&board, &selected(&board), 0)
                .unwrap_or_else(|error| panic!("{error:?}")),
            BallKind::Feather
        ),
        1
    );
    board.place(0, normal(1, 9));
    assert_eq!(
        reward_count(
            &clear_rewards(&board, &selected(&board), 0)
                .unwrap_or_else(|error| panic!("{error:?}")),
            BallKind::Feather
        ),
        0
    );
    assert!(
        clear_rewards(&board, &[], 0)
            .unwrap_or_else(|error| panic!("{error:?}"))
            .is_empty()
    );
}
#[test]
fn real_clear_rewards_stay_on_original_trigger_lane_after_followups() {
    let mut board = reward_fixture(5, 1);
    let mut pending = PendingClear::default();
    prepare_landing(&mut board, &mut pending, 5, normal(1, 0))
        .unwrap_or_else(|error| panic!("{error:?}"));
    prepare_landing(&mut board, &mut pending, 1, normal(1, 0))
        .unwrap_or_else(|error| panic!("{error:?}"));
    let effect = commit_clear(&mut board, &mut progress(), &mut pending)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(reward_count(&effect.rewards, BallKind::RowLaser), 1);
    assert!(effect.rewards.iter().all(|reward| reward.lane == 5));
}
#[test]
fn real_seventeen_ball_clear_earns_column_and_level_star_without_feather() {
    let mut board = reward_fixture(4, 4);
    // Start already tilted so this scenario isolates clear rewards from launching.
    board.stacks[3][3] = normal(2, 0);
    let effect = apply(&mut board, &mut progress(), 3, normal(0, 0));
    assert_eq!(effect.cleared_count, 17);
    assert!(effect.flights.is_empty());
    assert_eq!(effect.points, 1700);
    assert_eq!(reward_count(&effect.rewards, BallKind::ColumnLaser), 1);
    assert_eq!(reward_count(&effect.rewards, BallKind::RowLaser), 0);
    assert_eq!(reward_count(&effect.rewards, BallKind::Feather), 0);
    assert_eq!(reward_count(&effect.rewards, BallKind::SilverStar), 1);
}
#[test]
fn empty_and_all_zero_stacks_keep_utilities_inert_until_positive_contact() {
    let mut board = Board::default();
    for utility in [Ball::row_laser(), Ball::column_laser(), Ball::feather()] {
        let effect = apply(&mut board, &mut progress(), 0, utility);
        assert!(effect.utility_effects.is_empty());
    }
    let effect = apply(&mut board, &mut progress(), 0, normal(0, 0));
    assert!(effect.utility_effects.is_empty());
    assert_eq!(board.stack(0).map(<[Ball]>::len), Some(4));
    let effect = apply(&mut board, &mut progress(), 0, normal(1, 1));
    assert!(effect.utility_effects.is_empty()); // zero ordinary ball shields utilities
}
#[test]
fn arriving_and_resting_column_lasers_both_activate_on_direct_positive_contact() {
    for resting in [false, true] {
        let mut board = Board::default();
        let mut score = progress();
        board.place(
            0,
            if resting {
                Ball::column_laser()
            } else {
                normal(5, 1)
            },
        );
        let effect = apply(
            &mut board,
            &mut score,
            0,
            if resting {
                normal(5, 1)
            } else {
                Ball::column_laser()
            },
        );
        assert_eq!(effect.utility_effects.len(), 1);
        assert_eq!(effect.utility_effects[0].kind, BallKind::ColumnLaser);
        assert_eq!(effect.points, 500);
        assert_eq!(score.level(), NonZeroU32::MIN);
        assert_eq!(score.remaining(), 1000);
        assert!(effect.rewards.is_empty());
        assert_eq!(board.stack(0).map(<[Ball]>::len), Some(0));
    }
}
#[test]
fn row_targets_use_physical_tilt_and_bonus_rows_with_score_only_damage() {
    for bonus in [0, 3] {
        let mut board = activation_fixture();
        board.bonus_rows = bonus;
        let mut score = progress();
        let effect = apply(&mut board, &mut score, 0, Ball::row_laser());
        assert_eq!(
            effect.utility_effects,
            vec![UtilityEffect {
                kind: BallKind::RowLaser,
                position: PhysicalPosition { lane: 0, row: 3 },
                affected_count: 1
            }]
        );
        assert_eq!(effect.points, 500);
        assert_eq!(score.remaining(), 1000);
        assert_eq!(score.level(), NonZeroU32::MIN);
        assert_eq!(board.stack(2).map(<[Ball]>::len), Some(1));
        assert_eq!(board.stack(0).map(<[Ball]>::len), Some(2));
        assert_eq!(effect.flights.len(), 1);
        assert_eq!(effect.flights[0].ball, normal(1, 4));
        assert!(effect.rewards.is_empty());
    }
}
#[test]
fn feather_preserves_palettes_and_bomb_stone_identity_at_zero_weight() {
    let mut board = Board::default();
    for ball in [
        Ball::stone(),
        Ball::silver_star(),
        Ball::bomb(),
        Ball::gold_star(),
        normal(2, 1),
    ] {
        board.place(0, ball);
    }
    let before = board.stacks[0].clone();
    let effect = apply(&mut board, &mut progress(), 0, Ball::feather());
    assert_eq!(effect.utility_effects.len(), 1);
    assert_eq!(effect.points, 0);
    assert_eq!(effect.blasted_count, 0);
    for (after, before) in board.stacks[0].iter().zip(before) {
        assert_eq!(after.weight, 0);
        assert_eq!(after.kind, before.kind);
        assert_eq!(after.colors, before.colors);
        assert!(after.is_valid());
    }
    let zero_bomb = board.stacks[0][2].clone();
    let mut contact = Board::default();
    contact.place(0, normal(1, 1));
    let blast = apply(&mut contact, &mut progress(), 0, zero_bomb);
    assert_eq!(blast.blasted_count, 2);
    assert_eq!(blast.points, 100);
}
#[test]
fn zero_weight_normal_balls_still_match_without_points_or_advancement() {
    let mut board = Board::default();
    for lane in 0..3 {
        board.place(lane, normal(0, 0));
    }
    let mut score = progress();
    let effect = apply(&mut board, &mut score, 3, normal(0, 0));
    assert_eq!(effect.cleared_count, 4);
    assert_eq!(effect.points, 0);
    assert_eq!(score.remaining(), 1000);
    assert_eq!(reward_count(&effect.rewards, BallKind::Feather), 1);
}
#[test]
fn first_laser_destroys_other_already_contacting_utility_without_activation() {
    let mut board = Board::default();
    for (lane, ball) in [
        (0, normal(2, 0)),
        (0, Ball::row_laser()),
        (1, normal(2, 1)),
        (1, Ball::column_laser()),
    ] {
        board.place(lane, ball);
    }
    let effect = apply(&mut board, &mut progress(), 7, Ball::silver_star());
    assert_eq!(effect.utility_effects.len(), 1);
    assert_eq!(effect.utility_effects[0].kind, BallKind::RowLaser);
    assert_eq!(effect.points, 0);
    assert_eq!(board.stack(1), Some([normal(2, 1)].as_slice()));
}
#[test]
fn queued_lasers_remap_snapshot_targets_and_never_double_score_overlap() {
    let mut board = Board::default();
    for (lane, ball) in [
        (0, normal(2, 0)),
        (0, Ball::row_laser()),
        (1, Ball::column_laser()),
        (1, normal(2, 1)),
    ] {
        board.place(lane, ball);
    }
    let effect = apply(&mut board, &mut progress(), 7, Ball::silver_star());
    assert_eq!(effect.utility_effects.len(), 2);
    assert_eq!(effect.points, 200);
    assert_eq!(
        effect.utility_effects[1].position,
        PhysicalPosition { lane: 1, row: 1 }
    );
    assert_eq!(board.stack(1).map(<[Ball]>::len), Some(0));
}
#[test]
fn area_destruction_of_bombs_and_utilities_never_dispatches_their_effects() {
    let mut board = Board::default();
    for ball in [
        normal(2, 0),
        Ball::silver_star(),
        Ball::bomb(),
        Ball::gold_star(),
        Ball::column_laser(),
    ] {
        board.place(0, ball);
    }
    let mut score = progress();
    let effect = apply(&mut board, &mut score, 0, normal(1, 1));
    assert_eq!(effect.utility_effects.len(), 1);
    assert_eq!(effect.points, 1300);
    assert_eq!(effect.blasted_count, 0);
    assert_eq!(score.level(), NonZeroU32::MIN);
    assert_eq!(score.remaining(), 1000);
    assert!(effect.rewards.is_empty());
    let mut board = Board::default();
    for (lane, ball) in [
        (0, Ball::row_laser()),
        (0, normal(10, 0)),
        (1, normal(1, 1)),
        (1, Ball::bomb()),
    ] {
        board.place(lane, ball);
    }
    let effect = apply(&mut board, &mut progress(), 7, Ball::silver_star());
    assert!(effect.utility_effects.is_empty());
    assert!(effect.blasted_count > 0);
}
#[test]
fn actual_new_gravity_contact_activates_resting_utility_and_bomb() {
    for top in [Ball::feather(), Ball::bomb()] {
        let mut board = Board::default();
        let left_weight = 2;
        let right_weight = left_weight + top.weight;
        for (lane, ball) in [
            (0, normal(left_weight, 0)),
            (0, Ball::silver_star()),
            (0, top.clone()),
            (1, normal(right_weight, 1)),
            (1, Ball::row_laser()),
        ] {
            board.place(lane, ball);
        }
        let effect = apply(&mut board, &mut progress(), 7, Ball::silver_star());
        assert_eq!(effect.utility_effects[0].kind, BallKind::RowLaser);
        if top.kind == BallKind::Feather {
            assert_eq!(effect.utility_effects.len(), 2);
            assert_eq!(effect.utility_effects[1].kind, BallKind::Feather);
            assert_eq!(board.total(0), 0);
        } else {
            assert_eq!(effect.blast_centers.len(), 1);
            assert!(effect.blasted_count >= 2);
        }
    }
}
#[test]
fn utility_failure_rolls_back_synchronous_landing_and_pending_preparation() {
    let mut board = Board::default();
    board.place(0, Ball::column_laser());
    let mut score = progress();
    score
        .award_score_only(u64::MAX)
        .unwrap_or_else(|error| panic!("{error:?}"));
    let before = (board.clone(), score.clone());
    assert_eq!(
        land(&mut board, &mut score, 0, normal(1, 0)),
        Err(ResolutionError::Score(ScoreError::Overflow))
    );
    assert_eq!((board.clone(), score), before);
    let mut pending = PendingClear::default();
    for _ in 0..5 {
        board.place(6, normal(u32::MAX, 0));
    }
    let before = (board.clone(), pending.clone());
    assert_eq!(
        prepare_landing(&mut board, &mut pending, 0, normal(1, 0)),
        Err(ResolutionError::WeightOverflow)
    );
    assert_eq!((board, pending), before);
}

#[test]
fn owned_flying_utility_uses_the_same_positive_landing_contact_rules() {
    let mut board = Board::default();
    for (lane, ball) in [
        (0, normal(2, 0)),
        (0, Ball::silver_star()),
        (0, Ball::column_laser()),
        (1, normal(1, 1)),
    ] {
        board.place(lane, ball);
    }
    let mut score = progress();
    let emitted = apply(&mut board, &mut score, 1, normal(2, 2));
    assert!(emitted.utility_effects.is_empty());
    assert_eq!(emitted.flights.len(), 1);
    let flight = emitted.flights[0].clone();
    assert_eq!(flight.ball, Ball::column_laser());
    assert_eq!(flight.destination, 1);
    let landed = apply(&mut board, &mut score, flight.destination, flight.ball);
    assert_eq!(landed.utility_effects.len(), 1);
    assert_eq!(landed.utility_effects[0].kind, BallKind::ColumnLaser);
    assert_eq!(landed.points, 300);
    assert_eq!(score.remaining(), 1000);
    assert_eq!(board.stack(1).map(<[Ball]>::len), Some(0));
}
#[test]
fn tilted_row_laser_hits_equal_physical_rows_not_equal_stack_indices() {
    let mut board = Board::default();
    for (lane, ball) in [
        (0, normal(5, 0)),
        (1, normal(1, 1)),
        (2, normal(2, 2)),
        (2, normal(2, 3)),
        (3, normal(1, 4)),
    ] {
        board.place(lane, ball);
    }
    let effect = apply(&mut board, &mut progress(), 0, Ball::row_laser());
    assert_eq!(
        effect.utility_effects[0].position,
        PhysicalPosition { lane: 0, row: 1 }
    );
    assert_eq!(effect.points, 200);
    assert_eq!(board.stack(2), Some([normal(2, 2)].as_slice()));
    assert_eq!(board.stack(1), Some([normal(1, 1)].as_slice()));
    assert_eq!(board.stack(3), Some([normal(1, 4)].as_slice()));
}
