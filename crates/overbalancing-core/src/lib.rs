//! Deterministic game domain, independent of rendering and platform services.
#![forbid(unsafe_code)]

pub mod flight;
pub mod matching;
pub mod resolution;
pub mod scoring;
pub mod supply;

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

/// A zero-based color identity in a level's palette.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ColorId(u32);
impl ColorId {
    /// The first palette color, retained for simple fixtures.
    pub const RED: Self = Self(0);
    /// The second palette color, retained for simple fixtures.
    pub const BLUE: Self = Self(1);
    /// The third palette color, retained for simple fixtures.
    pub const YELLOW: Self = Self(2);
    /// Construct a palette identity; level boundaries validate membership.
    pub const fn new(index: u32) -> Self {
        Self(index)
    }
    /// Read this color's zero-based palette index.
    pub const fn index(self) -> u32 {
        self.0
    }
}

/// Canonical palette: one to three colors for ordinary balls, empty for specials.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BallColors(Vec<ColorId>);
impl BallColors {
    /// Empty palette reserved for colorless special balls.
    pub fn none() -> Self {
        Self(Vec::new())
    }
    /// Construct a one-color ball palette.
    pub fn single(color: ColorId) -> Self {
        Self(vec![color])
    }
    /// Normalize duplicates and order, rejecting zero or more than three distinct colors.
    pub fn new(colors: &[ColorId]) -> Option<Self> {
        let mut unique = Vec::with_capacity(3);
        for color in colors {
            if !unique.contains(color) {
                if unique.len() == 3 {
                    return None;
                }
                unique.push(*color);
            }
        }
        if unique.is_empty() {
            return None;
        }
        unique.sort_unstable();
        Some(Self(unique))
    }
    /// Whether the ball contains a color identity.
    pub fn contains(&self, color: ColorId) -> bool {
        self.0.binary_search(&color).is_ok()
    }
    /// Number of distinct colors, zero for colorless special balls.
    pub fn count(&self) -> usize {
        self.0.len()
    }
    /// Iterate identities in canonical ascending order.
    pub fn iter(&self) -> impl Iterator<Item = ColorId> + '_ {
        self.0.iter().copied()
    }
    /// Toggle a color, refusing empty or more-than-three-color results.
    pub fn toggle(&mut self, color: ColorId) -> bool {
        match self.0.binary_search(&color) {
            Ok(_) if self.0.len() == 1 => false,
            Ok(index) => {
                self.0.remove(index);
                true
            }
            Err(_) if self.0.len() == 3 => false,
            Err(index) => {
                self.0.insert(index, color);
                true
            }
        }
    }
}

/// A game ball with kind-specific mass and color invariants.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Ball {
    /// Rules identity, independent of visual presentation.
    pub kind: BallKind,
    /// Weight units, independent of the selected color.
    pub weight: u32,
    /// One to three ordinary colors; empty for special balls.
    pub colors: BallColors,
}

/// Deterministic identity of an ordinary or special ball.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BallKind {
    /// Colored ordinary ball, including multicolor jokers.
    Normal,
    /// Weightless earned level reward.
    SilverStar,
    /// Two vertically combined silver stars.
    GoldStar,
    /// Weight-ten contact explosive.
    Bomb,
    /// Inert weight-five obstacle.
    Stone,
}
impl Ball {
    /// Construct an ordinary colored ball; placement still validates its weight.
    pub fn normal(weight: u32, colors: BallColors) -> Self {
        Self {
            kind: BallKind::Normal,
            weight,
            colors,
        }
    }
    /// Create a weightless silver star.
    pub fn silver_star() -> Self {
        Self {
            kind: BallKind::SilverStar,
            weight: 0,
            colors: BallColors::none(),
        }
    }
    /// Create a weightless golden star.
    pub fn gold_star() -> Self {
        Self {
            kind: BallKind::GoldStar,
            weight: 0,
            colors: BallColors::none(),
        }
    }
    /// Create a contact bomb with the exceptional weight of ten.
    pub fn bomb() -> Self {
        Self {
            kind: BallKind::Bomb,
            weight: 10,
            colors: BallColors::none(),
        }
    }
    /// Create an inert weight-five obstacle.
    pub fn stone() -> Self {
        Self {
            kind: BallKind::Stone,
            weight: 5,
            colors: BallColors::none(),
        }
    }
    /// Validate kind-specific mass and palette invariants.
    pub fn is_valid(&self) -> bool {
        match self.kind {
            BallKind::Normal => self.weight > 0 && (1..=3).contains(&self.colors.count()),
            BallKind::SilverStar | BallKind::GoldStar => {
                self.weight == 0 && self.colors.count() == 0
            }
            BallKind::Bomb => self.weight == 10 && self.colors.count() == 0,
            BallKind::Stone => self.weight == 5 && self.colors.count() == 0,
        }
    }
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
/// Each stack is ordered bottom to top. Raw placement only appends a valid ball;
/// use the resolution module for landing effects. Session expansions extend all lanes.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Board {
    stacks: [Vec<Ball>; LANE_COUNT],
    bonus_rows: usize,
}
impl Board {
    /// Inspect a stack bottom to top, or return `None` for an invalid lane.
    pub fn stack(&self, lane: usize) -> Option<&[Ball]> {
        self.stacks.get(lane).map(Vec::as_slice)
    }

    /// Append a kind-valid ball to a valid lane; otherwise leave the board unchanged.
    pub fn place(&mut self, lane: usize, ball: Ball) -> bool {
        if !ball.is_valid() {
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

    /// Visible capacity: heavy side nine, balanced eight, rising side seven.
    pub fn capacity(&self, lane: usize) -> Option<usize> {
        if lane >= LANE_COUNT {
            return None;
        }
        Some(
            self.bonus_rows
                + match (self.balance(lane / 2), lane.is_multiple_of(2)) {
                    (Some(Balance::Balanced), _) => 8,
                    (Some(Balance::LeftHeavy), true) | (Some(Balance::RightHeavy), false) => 9,
                    _ => 7,
                },
        )
    }
    /// Permanent rows earned this session, capped at three.
    pub const fn bonus_rows(&self) -> usize {
        self.bonus_rows
    }
    /// Whether all three permanent row expansions have been earned.
    pub const fn expansion_achievement(&self) -> bool {
        self.bonus_rows == 3
    }
    /// Number of balls beyond visible capacity, without rejection or deletion.
    pub fn overflow(&self, lane: usize) -> Option<usize> {
        Some(self.stack(lane)?.len().saturating_sub(self.capacity(lane)?))
    }

    /// Whether any stack exceeds its current weight-dependent capacity.
    ///
    /// Capacity is re-evaluated for every side: a tilt may overflow the rising
    /// stack without placing a ball directly onto it. This query never removes
    /// balls; the application stops further player drops after game over.
    pub fn is_game_over(&self) -> bool {
        (0..LANE_COUNT).any(|lane| self.overflow(lane).is_some_and(|count| count > 0))
    }

    /// Begin a fresh session: empty stacks and reset earned expansions.
    pub fn clear(&mut self) {
        self.bonus_rows = 0;
        for stack in &mut self.stacks {
            stack.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ball(weight: u32, color: ColorId) -> Ball {
        Ball {
            kind: crate::BallKind::Normal,
            weight,
            colors: BallColors::single(color),
        }
    }

    #[test]
    fn exact_capacity_is_safe_and_any_overflow_ends_the_game() {
        let mut board = Board::default();
        assert!(!board.is_game_over());
        for lane in [0, 1] {
            for _ in 0..8 {
                assert!(board.place(lane, ball(1, ColorId::RED)));
            }
        }
        assert_eq!(board.capacity(0), Some(8));
        assert_eq!(board.overflow(0), Some(0));
        assert!(!board.is_game_over());
        // Right becomes heavier; the untouched left side shrinks from eight to seven.
        assert!(board.place(1, ball(1, ColorId::BLUE)));
        assert_eq!(board.capacity(1), Some(9));
        assert_eq!(board.overflow(1), Some(0));
        assert_eq!(board.capacity(0), Some(7));
        assert_eq!(board.overflow(0), Some(1));
        assert!(board.is_game_over());
        assert_eq!(board.stack(0).map(<[Ball]>::len), Some(8));
        board.clear();
        assert!(!board.is_game_over());
    }

    #[test]
    fn heavy_capacity_nine_and_light_capacity_seven_allow_the_exact_boundary() {
        let mut board = Board::default();
        for _ in 0..9 {
            assert!(board.place(0, ball(2, ColorId::RED)));
        }
        for _ in 0..7 {
            assert!(board.place(1, ball(1, ColorId::BLUE)));
        }
        assert_eq!(board.overflow(0), Some(0));
        assert_eq!(board.overflow(1), Some(0));
        assert!(!board.is_game_over());
        assert!(board.place(0, ball(1, ColorId::RED)));
        assert!(board.is_game_over());
    }

    #[test]
    fn arbitrary_palette_ids_are_owned_canonical_and_bounded() {
        let Some(mut colors) = BallColors::new(&[
            ColorId::new(u32::MAX),
            ColorId::new(50),
            ColorId::new(7),
            ColorId::new(50),
        ]) else {
            panic!("valid colors");
        };
        assert_eq!(
            colors.iter().map(ColorId::index).collect::<Vec<_>>(),
            vec![7, 50, u32::MAX]
        );
        assert!(!colors.toggle(ColorId::new(99)));
        assert_eq!(
            BallColors::new(&[
                ColorId::new(0),
                ColorId::new(1),
                ColorId::new(2),
                ColorId::new(3)
            ]),
            None
        );
        let original = colors.clone();
        assert!(colors.toggle(ColorId::new(50)));
        assert!(original.contains(ColorId::new(50)));
        assert!(!colors.contains(ColorId::new(50)));
        let original_ball = Ball {
            kind: crate::BallKind::Normal,
            weight: 9,
            colors: original,
        };
        let mut cloned_ball = original_ball.clone();
        assert!(cloned_ball.colors.toggle(ColorId::new(7)));
        assert_eq!(original_ball.colors.count(), 3);
        assert_eq!(cloned_ball.weight, original_ball.weight);
    }

    #[test]
    fn all_nonempty_palettes_have_canonical_order_and_no_duplicates() {
        let palette = [ColorId::RED, ColorId::BLUE, ColorId::YELLOW];
        assert_eq!(BallColors::new(&[]), None);
        for mask in 1u8..8 {
            let selected: Vec<_> = palette
                .into_iter()
                .enumerate()
                .filter_map(|(index, color)| (mask & (1 << index) != 0).then_some(color))
                .collect();
            let mut duplicated = selected.clone();
            duplicated.reverse();
            duplicated.extend_from_slice(&selected);
            let Some(colors) = BallColors::new(&duplicated) else {
                panic!("nonempty palette");
            };
            assert_eq!(colors.count(), selected.len());
            assert_eq!(colors.iter().collect::<Vec<_>>(), selected);
            for (index, color) in palette.into_iter().enumerate() {
                assert_eq!(colors.contains(color), mask & (1 << index) != 0);
            }
        }
    }

    #[test]
    fn toggling_never_removes_the_last_color() {
        let mut colors = BallColors::single(ColorId::RED);
        assert!(!colors.toggle(ColorId::RED));
        assert_eq!(colors, BallColors::single(ColorId::RED));
        assert!(colors.toggle(ColorId::YELLOW));
        assert!(colors.toggle(ColorId::BLUE));
        assert_eq!(colors.count(), 3);
        assert!(colors.toggle(ColorId::RED));
        assert!(colors.toggle(ColorId::BLUE));
        assert!(!colors.toggle(ColorId::YELLOW));
        assert_eq!(colors, BallColors::single(ColorId::YELLOW));
    }

    #[test]
    fn multicolor_placement_preserves_palette_and_does_not_multiply_weight() {
        let Some(colors) = BallColors::new(&[ColorId::YELLOW, ColorId::RED, ColorId::BLUE]) else {
            panic!("nonempty palette");
        };
        let multi = Ball {
            kind: crate::BallKind::Normal,
            weight: 3,
            colors,
        };
        let mut board = Board::default();
        assert!(board.place(0, multi.clone()));
        assert!(board.place(0, multi.clone()));
        assert!(board.place(1, ball(6, ColorId::BLUE)));
        assert_eq!(board.total(0), 6);
        assert_eq!(board.balance(0), Some(Balance::Balanced));
        assert_eq!(board.stack(0), Some([multi.clone(), multi].as_slice()));
        // Neither matching colors nor three-color appearance resolves any ball.
        assert_eq!(board.stack(1).map(<[Ball]>::len), Some(1));
    }

    #[test]
    fn placement_preserves_order_weight_color_and_other_stacks() {
        let mut board = Board::default();
        let first = ball(2, ColorId::RED);
        let second = ball(3, ColorId::BLUE);
        assert!(board.place(5, first.clone()));
        assert!(board.place(5, second.clone()));
        assert_eq!(board.stack(5), Some([first, second].as_slice()));
        assert_eq!(board.total(5), 5);
        for lane in [0, 1, 2, 3, 4, 6, 7] {
            assert_eq!(board.stack(lane), Some([].as_slice()));
        }
    }

    #[test]
    fn invalid_placement_is_atomic_and_indices_are_bounded() {
        let mut board = Board::default();
        assert!(board.place(0, ball(1, ColorId::YELLOW)));
        let before = board.clone();
        assert!(!board.place(LANE_COUNT, ball(1, ColorId::RED)));
        assert!(!board.place(usize::MAX, ball(1, ColorId::RED)));
        assert!(!board.place(0, ball(0, ColorId::BLUE)));
        assert_eq!(board, before);
        assert_eq!(board.stack(usize::MAX), None);
        assert_eq!(board.total(usize::MAX), 0);
        assert_eq!(board.balance(usize::MAX), None);
    }

    #[test]
    fn cited_weight_examples_only_change_balance_never_remove_balls() {
        for right_weights in [vec![1], vec![1, 1], vec![2], vec![3]] {
            let mut board = Board::default();
            assert!(board.place(0, ball(1, ColorId::RED)));
            for weight in &right_weights {
                assert!(board.place(1, ball(*weight, ColorId::BLUE)));
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
        assert!(board.place(2, ball(u32::MAX, ColorId::RED)));
        assert!(board.place(2, ball(u32::MAX, ColorId::YELLOW)));
        assert_eq!(board.total(2), u64::from(u32::MAX) * 2);
        assert_eq!(board.balance(1), Some(Balance::LeftHeavy));
        for _ in 0..2 {
            assert!(board.place(3, ball(u32::MAX, ColorId::BLUE)));
        }
        assert_eq!(board.balance(1), Some(Balance::Balanced));
        assert!(board.place(3, ball(1, ColorId::RED)));
        assert_eq!(board.balance(1), Some(Balance::RightHeavy));
        assert_eq!(board.balance(0), Some(Balance::Balanced));
    }

    #[test]
    fn clear_resets_every_stack_and_allows_another_experiment() {
        let mut board = Board::default();
        for lane in 0..LANE_COUNT {
            assert!(board.place(lane, ball(lane as u32 + 1, ColorId::YELLOW)));
        }
        board.clear();
        assert_eq!(board, Board::default());
        for seesaw in 0..SEESAW_COUNT {
            assert_eq!(board.balance(seesaw), Some(Balance::Balanced));
        }
        assert!(board.place(7, ball(3, ColorId::RED)));
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
