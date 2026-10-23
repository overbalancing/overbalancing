//! Universal matching and causal flight clear reward scenarios.
use super::*;
use crate::{BallColors, ColorId};
use std::num::NonZeroU32;
fn normal(weight: u32, color: u32) -> Ball {
    Ball::normal(weight, BallColors::single(ColorId::new(color)))
}
fn progress() -> Progression {
    Progression::new(NonZeroU32::MIN)
}
fn commit(board: &mut Board, score: &mut Progression, pending: &mut PendingClear) -> Outcome {
    commit_clear(board, score, pending).unwrap_or_else(|error| panic!("{error:?}"))
}
fn ultimates(outcome: &Outcome) -> u32 {
    outcome
        .rewards
        .iter()
        .filter(|reward| reward.ball.kind == BallKind::UltimateJoker)
        .map(|reward| reward.count)
        .sum()
}
fn cascade_fixture() -> Board {
    let mut board = Board::default();
    for ball in [normal(1, 0), normal(1, 0), normal(1, 1)] {
        board.place(0, ball);
    }
    for lane in [1, 2] {
        board.place(lane, normal(2, 0));
        board.place(lane, normal(1, 1));
    }
    board.place(3, normal(2, 0));
    board.place(3, normal(1, 20));
    board
}
#[test]
fn universal_connects_every_ordinary_color_but_no_colorless_specials() {
    let joker = Ball::ultimate_joker();
    assert!(joker.is_valid());
    assert_eq!(joker.weight, 0);
    assert_eq!(joker.colors.count(), 0);
    for color in [0, 1, 17, u32::MAX] {
        assert!(matching::shares_color(&joker, &normal(0, color)));
        assert!(matching::shares_color(&normal(2, color), &joker));
    }
    assert!(matching::shares_color(
        &joker,
        &Ball::normal(
            3,
            BallColors::new(&[ColorId::RED, ColorId::BLUE]).unwrap_or_else(|| panic!("palette"))
        )
    ));
    for special in [
        Ball::silver_star(),
        Ball::gold_star(),
        Ball::bomb(),
        Ball::stone(),
        Ball::row_laser(),
        Ball::column_laser(),
        Ball::feather(),
    ] {
        assert!(!matching::shares_color(&joker, &special));
        assert!(!matching::shares_color(&special, &joker));
    }
}
#[test]
fn common_color_seed_then_universal_bridge_clears_unlimited_palette_identities() {
    let mut board = Board::default();
    for (lane, ball) in [
        (0, normal(0, 0)),
        (1, normal(0, 0)),
        (2, Ball::ultimate_joker()),
        (3, normal(0, 800)),
        (4, normal(0, u32::MAX)),
        (5, Ball::stone()),
    ] {
        board.place(lane, ball);
    }
    // Pair 4/5 is tilted by stone; keep this test's bridge on aligned rows 0..3.
    assert!(matching::cascade_cells(&board).contains(&matching::Cell { lane: 3, index: 0 }));
    let mut board = Board::default();
    for (lane, ball) in [
        (0, normal(0, 0)),
        (1, Ball::ultimate_joker()),
        (2, normal(0, 1)),
    ] {
        board.place(lane, ball);
    }
    assert!(matching::cascade_cells(&board).is_empty()); // no common-color triple
}
#[test]
fn three_universal_jokers_clear_without_any_palette_color_or_fake_id() {
    let mut board = Board::default();
    for lane in 0..2 {
        board.place(lane, Ball::ultimate_joker());
    }
    let mut pending = PendingClear::default();
    prepare_flight_landing(&mut board, &mut pending, 2, Ball::ultimate_joker())
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(pending.cells().len(), 3);
    assert!(matching::detect(&board).is_empty());
    let effect = commit(&mut board, &mut progress(), &mut pending);
    assert_eq!(effect.cleared_count, 3);
    assert_eq!(effect.points, 0);
    assert_eq!(ultimates(&effect), 1);
}
#[test]
fn universal_never_compresses_and_survives_vertical_only_stack() {
    let mut board = Board::default();
    for _ in 0..4 {
        board.place(0, Ball::ultimate_joker());
    }
    let effect = land(&mut board, &mut progress(), 0, Ball::ultimate_joker())
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(effect.compressed_count, 0);
    assert_eq!(board.stack(0).map(<[Ball]>::len), Some(5));
}
#[test]
fn actual_emitted_flight_opens_clear_and_earns_one_reward_in_landing_lane() {
    let mut board = Board::default();
    for lane in [1, 4, 5, 6] {
        board.place(lane, normal(1, 0));
    }
    let mut pending = PendingClear::default();
    let mut score = progress();
    let dropped = prepare_landing(&mut board, &mut pending, 0, normal(3, 1))
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert!(pending.cells().is_empty());
    assert_eq!(dropped.flights.len(), 1);
    let flight = dropped.flights[0].clone();
    assert_eq!(flight.destination, 7);
    prepare_flight_landing(&mut board, &mut pending, flight.destination, flight.ball)
        .unwrap_or_else(|error| panic!("{error:?}"));
    let effect = commit(&mut board, &mut score, &mut pending);
    assert_eq!(effect.points, 400);
    assert_eq!(ultimates(&effect), 1);
    assert!(effect.rewards.iter().all(|reward| reward.lane == 7));
}
#[test]
fn every_flight_opened_gravity_clear_phase_earns_another_reward() {
    let mut board = cascade_fixture();
    let mut pending = PendingClear::default();
    let mut score = progress();
    prepare_flight_landing(&mut board, &mut pending, 7, normal(1, 99))
        .unwrap_or_else(|error| panic!("{error:?}"));
    let first = commit(&mut board, &mut score, &mut pending);
    assert_eq!(ultimates(&first), 1);
    assert!(!pending.cells().is_empty());
    let second = commit(&mut board, &mut score, &mut pending);
    assert_eq!(ultimates(&second), 1);
    assert!(pending.is_empty());
    assert_eq!(score.score(), 1100);
    assert!(
        first
            .rewards
            .iter()
            .chain(second.rewards.iter())
            .all(|reward| reward.lane == 7)
    );
}
#[test]
fn multiple_components_in_one_phase_still_earn_one_ultimate() {
    let mut board = Board::default();
    for lane in 0..7 {
        board.place(lane, normal(0, u32::from(lane >= 4)));
    }
    let mut pending = PendingClear::default();
    prepare_flight_landing(&mut board, &mut pending, 7, normal(0, 1))
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(matching::detect(&board).len(), 2);
    assert_eq!(
        ultimates(&commit(&mut board, &mut progress(), &mut pending)),
        1
    );
}
#[test]
fn chute_clears_and_unrelated_or_joined_flights_never_upgrade_existing_phase() {
    for flight in [normal(1, 99), normal(1, 0)] {
        let mut board = Board::default();
        for lane in 0..3 {
            board.place(lane, normal(1, 0));
        }
        let mut pending = PendingClear::default();
        prepare_landing(&mut board, &mut pending, 3, normal(1, 0))
            .unwrap_or_else(|error| panic!("{error:?}"));
        prepare_flight_landing(&mut board, &mut pending, 0, flight)
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(
            ultimates(&commit(&mut board, &mut progress(), &mut pending)),
            0
        );
    }
    let mut board = cascade_fixture();
    let mut pending = PendingClear::default();
    let mut score = progress();
    prepare_landing(&mut board, &mut pending, 7, normal(1, 99))
        .unwrap_or_else(|error| panic!("{error:?}"));
    while !pending.is_empty() {
        assert_eq!(ultimates(&commit(&mut board, &mut score, &mut pending)), 0);
    }
}
#[test]
fn new_independent_drop_resets_finished_flight_provenance() {
    let mut board = Board::default();
    let mut pending = PendingClear::default();
    let mut score = progress();
    for lane in 0..3 {
        board.place(lane, normal(0, 0));
    }
    prepare_flight_landing(&mut board, &mut pending, 3, normal(0, 0))
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(ultimates(&commit(&mut board, &mut score, &mut pending)), 1);
    assert!(pending.is_empty());
    for lane in 0..3 {
        board.place(lane, normal(0, 0));
    }
    prepare_landing(&mut board, &mut pending, 3, normal(0, 0))
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(ultimates(&commit(&mut board, &mut score, &mut pending)), 0);
    let reset = PendingClear::default();
    assert!(!reset.flight_caused);
}
#[test]
fn bomb_and_laser_damage_alone_never_generate_ultimate_rewards() {
    for source in [Ball::bomb(), Ball::column_laser()] {
        let mut board = Board::default();
        board.place(0, normal(2, 0));
        let mut pending = PendingClear::default();
        prepare_flight_landing(&mut board, &mut pending, 0, source)
            .unwrap_or_else(|error| panic!("{error:?}"));
        let effect = commit(&mut board, &mut progress(), &mut pending);
        assert_eq!(ultimates(&effect), 0);
    }
}
#[test]
fn failed_flight_and_score_commit_preserve_provenance_and_do_not_duplicate_rewards() {
    let mut board = Board::default();
    for lane in 0..3 {
        board.place(lane, normal(1, 0));
    }
    let mut pending = PendingClear::default();
    let before = (board.clone(), pending.clone());
    assert_eq!(
        prepare_flight_landing(&mut board, &mut pending, 8, normal(0, 0)),
        Err(ResolutionError::InvalidLane)
    );
    assert_eq!((board.clone(), pending.clone()), before);
    prepare_flight_landing(&mut board, &mut pending, 3, normal(1, 0))
        .unwrap_or_else(|error| panic!("{error:?}"));
    // Force score overflow while retaining the exact marked identities and flight flag.
    let mut score = progress();
    score
        .award_score_only(u64::MAX)
        .unwrap_or_else(|error| panic!("{error:?}"));
    let before = (board.clone(), pending.clone(), score.clone());
    assert_eq!(
        commit_clear(&mut board, &mut score, &mut pending),
        Err(ResolutionError::Score(ScoreError::Overflow))
    );
    assert_eq!((board.clone(), pending.clone(), score), before);
    assert_eq!(
        ultimates(&commit(&mut board, &mut progress(), &mut pending)),
        1
    );
}

#[test]
fn earlier_damage_points_do_not_steal_a_new_flight_clear_trigger_lane() {
    let mut board = Board::default();
    board.place(0, Ball::column_laser());
    for lane in [4, 5, 6] {
        board.place(lane, normal(1, 0));
    }
    let mut pending = PendingClear::default();
    prepare_landing(&mut board, &mut pending, 0, normal(1, 9))
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert!(pending.cells().is_empty());
    assert!(!pending.is_empty());
    prepare_flight_landing(&mut board, &mut pending, 7, normal(1, 0))
        .unwrap_or_else(|error| panic!("{error:?}"));
    let effect = commit(&mut board, &mut progress(), &mut pending);
    assert_eq!(effect.points, 500);
    assert_eq!(ultimates(&effect), 1);
    assert!(effect.rewards.iter().all(|reward| reward.lane == 7));
}
