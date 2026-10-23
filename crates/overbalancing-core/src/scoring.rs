//! Checked score calculation and a configurable, increasing level curve.
use crate::{Board, matching::Cell};
use std::{collections::BTreeSet, num::NonZeroU32};

/// Score awarded for each removed weight unit.
pub const POINTS_PER_WEIGHT: u64 = 100;
/// Base points required per current level before the later-level multiplier.
pub const LEVEL_POINTS: u64 = 1000;
/// First current level whose advancement threshold uses the slower pace.
pub const SLOW_LEVEL_START: u32 = 3;
/// Multiplier applied from level three onward, preserving the first two thresholds.
pub const LATE_LEVEL_FACTOR: u64 = 2;
/// Additional eligible points required to complete the current level.
pub const fn level_threshold(level: NonZeroU32) -> u64 {
    let factor = if level.get() >= SLOW_LEVEL_START {
        LATE_LEVEL_FACTOR
    } else {
        1
    };
    level.get() as u64 * LEVEL_POINTS * factor
}
// Sum thresholds for current levels 1..=last. Full triangular late pricing
// minus the first levels' discount keeps large awards logarithmic in level range.
fn prefix_cost(last: u64) -> u128 {
    let triangular = |count: u64| u128::from(count) * u128::from(count + 1) / 2;
    let early = last.min(u64::from(SLOW_LEVEL_START - 1));
    u128::from(LEVEL_POINTS)
        * (u128::from(LATE_LEVEL_FACTOR) * triangular(last)
            - u128::from(LATE_LEVEL_FACTOR - 1) * triangular(early))
}
/// A rejected operation does not change progression or board state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScoreError {
    /// A requested cell does not exist in the immutable board snapshot.
    InvalidCell,
    /// Weight multiplication, score addition or level range cannot be represented.
    Overflow,
}
/// Calculate a score for explicit ball identities, without applying clear policy.
/// Duplicate identities are counted once, even when several color groups overlap.
pub fn score_cells(board: &Board, cells: &[Cell]) -> Result<u64, ScoreError> {
    let mut weight = 0u64;
    for cell in cells.iter().copied().collect::<BTreeSet<_>>() {
        let ball = board
            .stack(cell.lane)
            .and_then(|stack| stack.get(cell.index))
            .ok_or(ScoreError::InvalidCell)?;
        weight = weight
            .checked_add(u64::from(ball.weight))
            .ok_or(ScoreError::Overflow)?;
    }
    score_weight(weight)
}
/// Checked score conversion, useful before committing a resolution transaction.
pub fn score_weight(weight: u64) -> Result<u64, ScoreError> {
    weight
        .checked_mul(POINTS_PER_WEIGHT)
        .ok_or(ScoreError::Overflow)
}
/// Score starts at zero at the selected initial level; earned surplus carries forward.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Progression {
    level: NonZeroU32,
    score: u64,
    progress: u64,
}
impl Progression {
    /// Begin a zero-score session at the selected level.
    pub const fn new(level: NonZeroU32) -> Self {
        Self {
            level,
            score: 0,
            progress: 0,
        }
    }
    /// Current level, always positive.
    pub const fn level(&self) -> NonZeroU32 {
        self.level
    }
    /// Total awarded score in this session.
    pub const fn score(&self) -> u64 {
        self.score
    }
    /// Points still required to advance from the current level.
    pub fn remaining(&self) -> u64 {
        level_threshold(self.level) - self.progress
    }
    /// Award visible points without contributing toward level advancement.
    pub fn award_score_only(&mut self, points: u64) -> Result<(), ScoreError> {
        self.score = self.score.checked_add(points).ok_or(ScoreError::Overflow)?;
        Ok(())
    }
    /// Atomically award points and carry surplus through every earned level.
    /// A bounded arithmetic search avoids iterating millions of levels for large awards.
    pub fn award(&mut self, points: u64) -> Result<(), ScoreError> {
        let score = self.score.checked_add(points).ok_or(ScoreError::Overflow)?;
        let available = u128::from(self.progress) + u128::from(points);
        let start = u64::from(self.level.get());
        let maximum = u64::from(u32::MAX) - start;
        let cost = |count: u64| -> u128 { prefix_cost(start + count - 1) - prefix_cost(start - 1) };
        let mut low = 0;
        let mut high = maximum;
        while low < high {
            let middle = low + (high - low).div_ceil(2);
            if cost(middle) <= available {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        let level = NonZeroU32::new((start + low) as u32).ok_or(ScoreError::Overflow)?;
        let progress = available - cost(low);
        if progress >= u128::from(level_threshold(level)) {
            return Err(ScoreError::Overflow);
        }
        self.level = level;
        self.progress = progress as u64;
        self.score = score;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Ball, BallColors, ColorId};
    fn level(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).unwrap_or(NonZeroU32::MIN)
    }
    #[test]
    fn score_is_weight_times_one_hundred_and_deduplicates_identity() {
        let mut board = Board::default();
        assert!(board.place(
            0,
            Ball {
                kind: crate::BallKind::Normal,
                weight: 3,
                colors: BallColors::single(ColorId::RED)
            }
        ));
        assert!(board.place(
            1,
            Ball {
                kind: crate::BallKind::Normal,
                weight: 7,
                colors: BallColors::single(ColorId::BLUE)
            }
        ));
        let cells = [
            Cell { lane: 0, index: 0 },
            Cell { lane: 1, index: 0 },
            Cell { lane: 0, index: 0 },
        ];
        assert_eq!(score_cells(&board, &cells), Ok(1000));
        assert_eq!(
            score_cells(&board, &[Cell { lane: 8, index: 0 }]),
            Err(ScoreError::InvalidCell)
        );
        assert_eq!(score_weight(u64::MAX), Err(ScoreError::Overflow));
        assert_eq!(board.total(0), 3);
        assert_eq!(board.total(1), 7);
    }
    #[test]
    fn curve_carries_surplus_from_any_selected_start_level() {
        let mut progression = Progression::new(level(1));
        assert_eq!(progression.award(999), Ok(()));
        assert_eq!(progression.remaining(), 1);
        assert_eq!(progression.award(2501), Ok(()));
        assert_eq!(progression.level(), level(3));
        assert_eq!(progression.score(), 3500);
        assert_eq!(progression.remaining(), 5500);
        let mut start_three = Progression::new(level(3));
        assert_eq!(start_three.remaining(), 6000);
        assert_eq!(start_three.award(7500), Ok(()));
        assert_eq!(start_three.level(), level(4));
        assert_eq!(start_three.remaining(), 6500);
    }
    #[test]
    fn large_awards_are_bounded_and_overflow_rejects_without_partial_update() {
        let mut progression = Progression::new(level(1));
        assert_eq!(progression.award(u64::MAX), Ok(()));
        let before = progression.clone();
        assert_eq!(progression.award(1), Err(ScoreError::Overflow));
        assert_eq!(progression, before);
        let mut last = Progression::new(level(u32::MAX));
        let before = last.clone();
        assert_eq!(
            last.award(level_threshold(level(u32::MAX))),
            Err(ScoreError::Overflow)
        );
        assert_eq!(last, before);
    }
    #[test]
    fn early_levels_stay_fast_then_every_later_threshold_doubles() {
        let mut progression = Progression::new(level(1));
        for (points, next, remaining) in [
            (1000, 2, 2000),
            (2000, 3, 6000),
            (6000, 4, 8000),
            (8000, 5, 10000),
        ] {
            assert_eq!(progression.award(points), Ok(()));
            assert_eq!(progression.level(), level(next));
            assert_eq!(progression.remaining(), remaining);
        }
        assert_eq!(progression.score(), 17000);
    }
    #[test]
    fn surplus_crosses_the_discount_boundary_and_score_only_does_not_advance() {
        let mut progression = Progression::new(level(2));
        assert_eq!(progression.award(9500), Ok(()));
        assert_eq!(progression.level(), level(4));
        assert_eq!(progression.remaining(), 6500);
        let before = (progression.level(), progression.remaining());
        assert_eq!(progression.award_score_only(100000), Ok(()));
        assert_eq!((progression.level(), progression.remaining()), before);
        assert_eq!(progression.score(), 109500);
    }
    #[test]
    fn maximum_level_uses_doubled_threshold_and_rejects_next_level_atomically() {
        let mut progression = Progression::new(level(u32::MAX));
        let threshold = u64::from(u32::MAX) * LEVEL_POINTS * LATE_LEVEL_FACTOR;
        assert_eq!(progression.remaining(), threshold);
        assert_eq!(progression.award(threshold - 1), Ok(()));
        assert_eq!(progression.remaining(), 1);
        let before = progression.clone();
        assert_eq!(progression.award(1), Err(ScoreError::Overflow));
        assert_eq!(progression, before);
    }
    #[test]
    fn closed_form_cost_matches_small_linear_reference_for_selected_starts() {
        for start in 1..8 {
            for points in [0, 999, 1000, 2999, 3000, 8999, 9000, 17000, 99999] {
                let mut expected = start;
                let mut progress = points;
                while progress >= level_threshold(level(expected)) {
                    progress -= level_threshold(level(expected));
                    expected += 1;
                }
                let mut actual = Progression::new(level(start));
                assert_eq!(actual.award(points), Ok(()));
                assert_eq!(actual.level(), level(expected));
                assert_eq!(
                    actual.remaining(),
                    level_threshold(level(expected)) - progress
                );
            }
        }
    }
}
