//! Checked score calculation and a configurable, increasing level curve.
use crate::{Board, matching::Cell};
use std::{collections::BTreeSet, num::NonZeroU32};

/// Score awarded for each removed weight unit.
pub const POINTS_PER_WEIGHT: u64 = 100;
/// Additional points required per current level (level L requires 1000 * L).
pub const LEVEL_POINTS: u64 = 1000;
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
        u64::from(self.level.get()) * LEVEL_POINTS - self.progress
    }
    /// Atomically award points and carry surplus through every earned level.
    /// A bounded arithmetic search avoids iterating millions of levels for large awards.
    pub fn award(&mut self, points: u64) -> Result<(), ScoreError> {
        let score = self.score.checked_add(points).ok_or(ScoreError::Overflow)?;
        let available = u128::from(self.progress) + u128::from(points);
        let start = u64::from(self.level.get());
        let maximum = u64::from(u32::MAX) - start;
        let cost = |count: u64| -> u128 {
            u128::from(LEVEL_POINTS)
                * u128::from(count)
                * u128::from(2 * start + count.saturating_sub(1))
                / 2
        };
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
        let level = start + low;
        let progress = available - cost(low);
        if progress >= u128::from(level) * u128::from(LEVEL_POINTS) {
            return Err(ScoreError::Overflow);
        }
        let Some(level) = NonZeroU32::new(level as u32) else {
            return Err(ScoreError::Overflow);
        };
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
                weight: 3,
                colors: BallColors::single(ColorId::RED)
            }
        ));
        assert!(board.place(
            1,
            Ball {
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
        assert_eq!(progression.remaining(), 2500);
        let mut start_three = Progression::new(level(3));
        assert_eq!(start_three.remaining(), 3000);
        assert_eq!(start_three.award(7500), Ok(()));
        assert_eq!(start_three.level(), level(5));
        assert_eq!(start_three.remaining(), 4500);
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
            last.award(u64::from(u32::MAX) * LEVEL_POINTS),
            Err(ScoreError::Overflow)
        );
        assert_eq!(last, before);
    }
}
