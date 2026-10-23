//! Level-dependent balls and explicitly selected preview supply lanes.
use crate::{Ball, BallColors, ColorId, LANE_COUNT};
use std::{collections::VecDeque, num::NonZeroU32};

/// Level generation parameters; no random source is owned by the core.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeneratorConfig {
    /// Exactly this many color identities are available, numbered from zero.
    pub level: NonZeroU32,
    /// Percentage of multi-color balls, between zero and one hundred.
    pub joker_percent: u8,
    /// Percentage of all generated balls replaced by bombs, default three.
    pub bomb_percent: u8,
}
/// Invalid generation settings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GenerationError {
    /// Percentage exceeds one hundred.
    InvalidJokerPercent,
    /// Bomb percentage exceeds one hundred.
    InvalidBombPercent,
    /// Exclusive ball-category percentages add up to more than one hundred.
    InvalidCombinedPercent,
}
fn bounded(entropy: &mut impl FnMut() -> u64, bound: u64) -> u64 {
    let threshold = bound.wrapping_neg() % bound;
    loop {
        let value = entropy();
        if value >= threshold {
            return value % bound;
        }
    }
}
impl GeneratorConfig {
    /// Configure a level with the initial five percent joker rate.
    pub const fn new(level: NonZeroU32) -> Self {
        Self {
            level,
            joker_percent: 5,
            bomb_percent: 3,
        }
    }
    /// Generate a positive integer weight and one to three distinct colors.
    ///
    /// Integer weights in the mathematical interval L/2..=L use ceil(L/2).
    /// Level one cannot produce jokers; level two uses two colors. Higher
    /// levels choose two or three joker colors with equal probability.
    /// The supplied stream must eventually produce accepted rejection samples.
    pub fn generate(&self, entropy: &mut impl FnMut() -> u64) -> Result<Ball, GenerationError> {
        if self.joker_percent > 100 {
            return Err(GenerationError::InvalidJokerPercent);
        }
        if self.bomb_percent > 100 {
            return Err(GenerationError::InvalidBombPercent);
        }
        if u16::from(self.bomb_percent) + u16::from(self.joker_percent) > 100 {
            return Err(GenerationError::InvalidCombinedPercent);
        }
        let category = bounded(entropy, 100);
        if category < u64::from(self.bomb_percent) {
            return Ok(Ball::bomb());
        }
        let level = u64::from(self.level.get());
        let minimum = level.div_ceil(2).max(1);
        let weight = (minimum + bounded(entropy, level - minimum + 1)) as u32;
        let first = ColorId::new(bounded(entropy, level) as u32);
        let joker =
            category < u64::from(self.bomb_percent) + u64::from(self.joker_percent) && level >= 2;
        let count = if !joker {
            1
        } else if level == 2 {
            2
        } else {
            2 + bounded(entropy, 2) as usize
        };
        let mut colors = BallColors::single(first);
        while colors.count() < count {
            // Sample an index in the remaining palette without allocating it.
            let mut index = bounded(entropy, level - colors.count() as u64) as u32;
            for selected in colors.iter() {
                if selected.index() <= index {
                    index += 1;
                }
            }
            colors.toggle(ColorId::new(index));
        }
        Ok(Ball {
            kind: crate::BallKind::Normal,
            weight,
            colors,
        })
    }
}

/// Invalid explicit supply consumption, leaving supply unchanged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupplyError {
    /// No preview lane exists at that index.
    InvalidLane,
    /// The replacement violates kind-specific weight or palette invariants.
    ZeroWeight,
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct SupplyRun {
    ball: Ball,
    count: u32,
}
/// Sixteen previews and one ball held independently in the chute.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Supply {
    held: Ball,
    previews: [[Ball; 2]; LANE_COUNT],
    overflow: [VecDeque<SupplyRun>; LANE_COUNT],
}
impl Supply {
    /// Generate the held ball, then two previews per lane from left to right.
    pub fn new(
        config: GeneratorConfig,
        entropy: &mut impl FnMut() -> u64,
    ) -> Result<Self, GenerationError> {
        let held = config.generate(entropy)?;
        // Construction validates settings before the infallible repeated draws.
        let mut lanes = Vec::with_capacity(LANE_COUNT);
        for _ in 0..LANE_COUNT {
            lanes.push([config.generate(entropy)?, config.generate(entropy)?]);
        }
        let previews = std::array::from_fn(|_| lanes.remove(0));
        Ok(Self {
            held,
            previews,
            overflow: std::array::from_fn(|_| VecDeque::new()),
        })
    }
    /// Ball currently held in the chute; moving the chute does not change it.
    pub fn held(&self) -> &Ball {
        &self.held
    }
    /// Front and following preview for an explicitly selected lane.
    pub fn previews(&self, lane: usize) -> Option<&[Ball; 2]> {
        self.previews.get(lane)
    }
    /// Insert an earned reward before a lane's visible previews without losing any ball.
    /// The held ball stays unchanged; displaced previews remain in ordered backlog.
    pub fn insert_reward_front(&mut self, lane: usize, ball: Ball) -> Result<(), SupplyError> {
        self.insert_reward_front_count(lane, ball, 1)
    }
    /// Insert a compact reward run; even millions of earned stars use constant storage.
    /// All existing held, visible and deferred balls retain their order and identity.
    pub fn insert_reward_front_count(
        &mut self,
        lane: usize,
        ball: Ball,
        count: u32,
    ) -> Result<(), SupplyError> {
        if lane >= LANE_COUNT {
            return Err(SupplyError::InvalidLane);
        }
        if !ball.is_valid() {
            return Err(SupplyError::ZeroWeight);
        }
        if count == 0 {
            return Ok(());
        }
        self.overflow[lane].push_front(SupplyRun {
            ball: self.previews[lane][1].clone(),
            count: 1,
        });
        self.overflow[lane].push_front(SupplyRun {
            ball: self.previews[lane][0].clone(),
            count: 1,
        });
        self.overflow[lane].push_front(SupplyRun { ball, count });
        for index in 0..2 {
            self.previews[lane][index] = Self::pop_front(&mut self.overflow[lane]);
        }
        Ok(())
    }
    fn pop_front(queue: &mut VecDeque<SupplyRun>) -> Ball {
        let Some(run) = queue.front_mut() else {
            unreachable!("replacement queue is populated before consumption");
        };
        let ball = run.ball.clone();
        run.count -= 1;
        if run.count == 0 {
            queue.pop_front();
        }
        ball
    }
    /// Return the previous held ball, refill the chute from a chosen front
    /// preview and append exactly one supplied replacement to that lane.
    /// No relationship between source and destination is imposed here.
    pub fn take_and_refill(&mut self, source: usize, next: Ball) -> Result<Ball, SupplyError> {
        if source >= LANE_COUNT {
            return Err(SupplyError::InvalidLane);
        }
        if !next.is_valid() {
            return Err(SupplyError::ZeroWeight);
        }
        self.overflow[source].push_back(SupplyRun {
            ball: next,
            count: 1,
        });
        let next = Self::pop_front(&mut self.overflow[source]);
        let lane = &mut self.previews[source];
        let front = std::mem::replace(&mut lane[1], next);
        let front = std::mem::replace(&mut lane[0], front);
        Ok(std::mem::replace(&mut self.held, front))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn level(value: u32) -> NonZeroU32 {
        let Some(level) = NonZeroU32::new(value) else {
            panic!("positive level");
        };
        level
    }
    fn scripted(values: &[u64]) -> impl FnMut() -> u64 + '_ {
        let mut values = values.iter();
        move || values.next().copied().unwrap_or(u64::MAX)
    }
    #[test]
    fn rejection_sampling_skips_biased_prefix_and_accepts_full_u32_range() {
        assert_eq!(bounded(&mut scripted(&[0, 1]), 3), 1);
        assert_eq!(
            bounded(&mut scripted(&[u64::MAX]), u64::from(u32::MAX) + 1),
            u64::from(u32::MAX)
        );
    }
    #[test]
    fn capacities_follow_weight_direction_and_report_without_removing_overflow() {
        use crate::Board;
        let mut board = Board::default();
        assert_eq!(board.capacity(0), Some(8));
        assert_eq!(board.capacity(1), Some(8));
        let ball = Ball {
            kind: crate::BallKind::Normal,
            weight: 1,
            colors: BallColors::single(ColorId::RED),
        };
        for _ in 0..10 {
            assert!(board.place(0, ball.clone()));
        }
        assert_eq!(board.capacity(0), Some(9));
        assert_eq!(board.capacity(1), Some(7));
        assert_eq!(board.overflow(0), Some(1));
        assert_eq!(board.stack(0).map(<[Ball]>::len), Some(10));
        for _ in 0..10 {
            assert!(board.place(1, ball.clone()));
        }
        assert_eq!(board.capacity(0), Some(8));
        assert_eq!(board.overflow(0), Some(2));
        assert!(board.place(1, ball));
        assert_eq!(board.capacity(0), Some(7));
        assert_eq!(board.capacity(1), Some(9));
        assert_eq!(board.overflow(1), Some(2));
        assert_eq!(board.capacity(usize::MAX), None);
        assert_eq!(board.overflow(usize::MAX), None);
    }

    #[test]
    fn exact_joker_threshold_and_level_constraints() {
        for roll in [0, 4, 5, 99] {
            let mut config = GeneratorConfig::new(level(3));
            config.bomb_percent = 0;
            let Ok(ball) = config.generate(&mut scripted(&[100 + roll, 0, 3, 0, 0, 0])) else {
                panic!("valid config");
            };
            assert_eq!(ball.colors.count(), if roll < 5 { 2 } else { 1 });
        }
        for value in [1, 2, 3, 17] {
            let config = GeneratorConfig {
                level: level(value),
                joker_percent: 100,
                bomb_percent: 0,
            };
            let Ok(ball) = config.generate(&mut scripted(&[u64::MAX])) else {
                panic!("valid config");
            };
            assert_eq!(
                ball.colors.count(),
                if value == 1 {
                    1
                } else if value == 2 {
                    2
                } else {
                    3
                }
            );
            assert!(ball.colors.iter().all(|color| color.index() < value));
        }
    }
    #[test]
    fn weight_bounds_cover_odd_and_maximum_levels_without_overflow() {
        for value in [1, 2, 3, 5, u32::MAX] {
            let config = GeneratorConfig {
                level: level(value),
                joker_percent: 0,
                bomb_percent: 0,
            };
            for seed in [u64::MAX, u64::MAX - 1, 100, 101] {
                let Ok(ball) = config.generate(&mut scripted(&[seed])) else {
                    panic!("valid config");
                };
                assert!(ball.weight >= value.div_ceil(2));
                assert!(ball.weight <= value);
            }
        }
        let config = GeneratorConfig {
            level: level(2),
            joker_percent: 101,
            bomb_percent: 0,
        };
        assert_eq!(
            config.generate(&mut || panic!("must not draw")),
            Err(GenerationError::InvalidJokerPercent)
        );
    }
    #[test]
    fn explicit_consumption_retains_other_lanes_and_shifts_once() {
        let Ok(mut supply) = Supply::new(GeneratorConfig::new(level(3)), &mut || u64::MAX) else {
            panic!("valid config");
        };
        let before = supply.clone();
        let replacement = Ball {
            kind: crate::BallKind::Normal,
            weight: 99,
            colors: BallColors::single(ColorId::new(20)),
        };
        assert_eq!(
            supply.take_and_refill(4, replacement.clone()),
            Ok(before.held.clone())
        );
        assert_eq!(supply.held(), &before.previews[4][0]);
        assert_eq!(
            supply.previews(4),
            Some(&[before.previews[4][1].clone(), replacement])
        );
        for lane in [0, 1, 2, 3, 5, 6, 7] {
            assert_eq!(supply.previews(lane), before.previews(lane));
        }
        assert_eq!(supply.previews(usize::MAX), None);
        let before = supply.clone();
        assert_eq!(
            supply.take_and_refill(usize::MAX, before.held.clone()),
            Err(SupplyError::InvalidLane)
        );
        let mut zero = before.held.clone();
        zero.colors = BallColors::none();
        assert_eq!(
            supply.take_and_refill(0, zero),
            Err(SupplyError::ZeroWeight)
        );
        assert_eq!(supply, before);
    }
}

#[cfg(test)]
mod special_tests {
    use super::*;
    use crate::BallKind;
    #[test]
    fn bomb_rate_is_exactly_three_percent_of_all_draws() {
        let config = GeneratorConfig::new(NonZeroU32::MIN);
        let mut bombs = 0;
        for roll in 0..100 {
            let mut first = true;
            let ball = config
                .generate(&mut || {
                    if std::mem::take(&mut first) {
                        100 + roll
                    } else {
                        u64::MAX
                    }
                })
                .unwrap_or_else(|error| panic!("{error:?}"));
            bombs += usize::from(ball.kind == BallKind::Bomb);
            assert!(ball.is_valid());
        }
        assert_eq!(bombs, 3);
        let mut invalid = config;
        invalid.bomb_percent = 101;
        assert_eq!(
            invalid.generate(&mut || panic!("invalid config must not consume entropy")),
            Err(GenerationError::InvalidBombPercent)
        );
    }
    #[test]
    fn front_reward_keeps_held_original_previews_and_every_generated_tail() {
        let mut supply = Supply::new(GeneratorConfig::new(NonZeroU32::MIN), &mut || u64::MAX)
            .unwrap_or_else(|error| panic!("{error:?}"));
        let held = supply.held().clone();
        let old = supply
            .previews(2)
            .unwrap_or_else(|| panic!("valid lane"))
            .clone();
        supply
            .insert_reward_front(2, Ball::silver_star())
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(supply.held(), &held);
        assert_eq!(
            supply.previews(2),
            Some(&[Ball::silver_star(), old[0].clone()])
        );
        let tail = Ball::stone();
        assert_eq!(supply.take_and_refill(2, tail.clone()), Ok(held));
        assert_eq!(supply.held(), &Ball::silver_star());
        assert_eq!(supply.previews(2), Some(&old));
        supply
            .take_and_refill(2, Ball::bomb())
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(supply.previews(2), Some(&[old[1].clone(), tail]));
    }
    #[test]
    fn huge_reward_runs_use_bounded_storage_and_accept_zero_weight() {
        let mut supply = Supply::new(GeneratorConfig::new(NonZeroU32::MIN), &mut || u64::MAX)
            .unwrap_or_else(|error| panic!("{error:?}"));
        supply
            .insert_reward_front_count(0, Ball::silver_star(), u32::MAX)
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(
            supply.previews(0),
            Some(&[Ball::silver_star(), Ball::silver_star()])
        );
        assert!(supply.overflow[0].len() <= 3);
        assert_eq!(supply.overflow[0][0].count, u32::MAX - 2);
        supply
            .take_and_refill(0, Ball::gold_star())
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(supply.held(), &Ball::silver_star());
        assert_eq!(supply.overflow[0][0].count, u32::MAX - 3);
    }
}

#[cfg(test)]
mod category_tests {
    use super::*;
    use crate::BallKind;
    #[test]
    fn exclusive_buckets_preserve_five_percent_jokers_among_all_balls() {
        let config = GeneratorConfig::new(NonZeroU32::new(3).unwrap_or(NonZeroU32::MIN));
        let (mut bombs, mut jokers, mut plain) = (0, 0, 0);
        for roll in 0..100 {
            let mut first = true;
            let ball = config
                .generate(&mut || {
                    if std::mem::take(&mut first) {
                        100 + roll
                    } else {
                        u64::MAX
                    }
                })
                .unwrap_or_else(|error| panic!("{error:?}"));
            if ball.kind == BallKind::Bomb {
                bombs += 1
            } else if ball.colors.count() > 1 {
                jokers += 1
            } else {
                plain += 1
            }
        }
        assert_eq!((bombs, jokers, plain), (3, 5, 92));
        let mut invalid = config;
        invalid.joker_percent = 98;
        assert_eq!(
            invalid.generate(&mut || panic!("invalid rate must not draw")),
            Err(GenerationError::InvalidCombinedPercent)
        );
    }
}
