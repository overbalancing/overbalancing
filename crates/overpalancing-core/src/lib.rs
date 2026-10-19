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

#[cfg(test)]
mod tests {
    use super::*;
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
