//! Transactional compression, joker cascades, scoring and seesaw reactions.
use crate::{
    Balance, Ball, BallKind, Board, LANE_COUNT, SEESAW_COUNT, matching,
    scoring::{self, Progression, ScoreError},
};

/// Rejected transactions leave both board and progression unchanged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolutionError {
    /// No lane exists at this index.
    InvalidLane,
    /// Landing requires kind-valid weight and palette invariants.
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
    /// Bonus rows at emission, before any later expansion.
    pub origin_bonus_rows: usize,
    /// Original star identity until its first visual crossing; authoritative ball is converted.
    pub edge_from: Option<BallKind>,
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
    /// Earned balls inserted at the front of their triggering supply lane.
    pub rewards: Vec<Reward>,
    /// Number of silver stars earned through actual level advancement.
    pub stars_earned: u32,
    /// Number of balls removed by bomb blasts.
    pub blasted_count: usize,
    /// Whole-board silver-star clears.
    pub silver_clears: usize,
    /// Permanent row expansions earned in this phase.
    pub expansions: usize,
    /// Snapshot coordinates of every bomb detonated.
    pub blast_centers: Vec<PhysicalPosition>,
    /// Activated utilities, with their original contact geometry.
    pub utility_effects: Vec<UtilityEffect>,
}
/// One consumed utility's authoritative activation feedback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UtilityEffect {
    /// Activated row laser, column laser or feather.
    pub kind: BallKind,
    /// Utility's physical position before area removals or weight changes.
    pub position: PhysicalPosition,
    /// Other balls targeted by the effect, excluding the consumed utility.
    pub affected_count: usize,
}
/// Earned supply ball with immutable causal lane provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reward {
    /// Supply lane that caused the reward.
    pub lane: usize,
    /// Earned ball.
    pub ball: Ball,
    /// Compact number of identical rewards, without allocation per earned level.
    pub count: u32,
}
/// Immutable physical blast position, before any removals or gravity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalPosition {
    /// Horizontal lane.
    pub lane: usize,
    /// Global bottom-relative row.
    pub row: usize,
}
/// Convert an emitted star exactly once when its unwrapped path crosses an edge.
pub fn convert_edge(ball: Ball, origin: usize, distance: u64, rightward: bool) -> Ball {
    if origin >= LANE_COUNT {
        return ball;
    }
    let crossed = if rightward {
        distance > (LANE_COUNT - 1 - origin) as u64
    } else {
        distance > origin as u64
    };
    if crossed && matches!(ball.kind, BallKind::SilverStar | BallKind::GoldStar) {
        Ball::bomb()
    } else {
        ball
    }
}
/// Marked balls remain visible and scoreless until an explicit clear commit.
/// Membership survives platform movement and is remapped through compression;
/// launched balls leave the pending board group and remain owned by their flight.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PendingClear {
    cells: Vec<matching::Cell>,
    eligible_points: u64,
    score_only: u64,
    trigger_lane: usize,
}
impl PendingClear {
    /// Current bottom-to-top identities; read again after every prepare or commit.
    pub fn cells(&self) -> &[matching::Cell] {
        &self.cells
    }
    /// Whether there is no pending clear animation.
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty() && self.eligible_points == 0 && self.score_only == 0
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
        rewards: Vec::new(),
        stars_earned: 0,
        blasted_count: 0,
        silver_clears: 0,
        expansions: 0,
        blast_centers: Vec::new(),
        utility_effects: Vec::new(),
    }
}
fn compress(board: &mut Board, pending: &mut PendingClear) -> Result<usize, ResolutionError> {
    let mut count = 0;
    for (lane, stack) in board.stacks.iter_mut().enumerate() {
        let mut index = 0;
        while index + 5 <= stack.len() {
            let colors = stack[index].colors.clone();
            if stack[index].kind != BallKind::Normal
                || colors.count() != 1
                || !stack[index + 1..index + 5]
                    .iter()
                    .all(|ball| ball.kind == BallKind::Normal && ball.colors == colors)
            {
                index += 1;
                continue;
            }
            let weight = stack[index..index + 5]
                .iter()
                .try_fold(0u32, |sum, ball| sum.checked_add(ball.weight))
                .ok_or(ResolutionError::WeightOverflow)?;
            stack.splice(
                index..index + 5,
                [Ball {
                    kind: crate::BallKind::Normal,
                    weight,
                    colors,
                }],
            );
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
        let original_kind = ball.kind;
        let ball = convert_edge(ball, origin, distance, rightward);
        let edge_from = (original_kind != ball.kind).then_some(original_kind);
        flights.push(ResolvedFlight {
            ball,
            edge_from,
            origin,
            destination,
            distance,
            origin_index,
            origin_capacity,
            origin_bonus_rows: board.bonus_rows(),
        });
    }
}
fn position(board: &Board, cell: matching::Cell) -> PhysicalPosition {
    PhysicalPosition {
        lane: cell.lane,
        row: cell.index + 9 + board.bonus_rows() - board.capacity(cell.lane).unwrap_or(8),
    }
}
fn all_cells(board: &Board) -> Vec<matching::Cell> {
    board
        .stacks
        .iter()
        .enumerate()
        .flat_map(|(lane, stack)| (0..stack.len()).map(move |index| matching::Cell { lane, index }))
        .collect()
}
fn remove_cells(board: &mut Board, pending: &mut PendingClear, cells: &[matching::Cell]) {
    let selected: std::collections::BTreeSet<_> = cells.iter().copied().collect();
    pending.cells.retain(|cell| !selected.contains(cell));
    for cell in &mut pending.cells {
        cell.index -= selected
            .iter()
            .filter(|removed| removed.lane == cell.lane && removed.index < cell.index)
            .count();
    }
    for cell in selected.into_iter().rev() {
        board.stacks[cell.lane].remove(cell.index);
    }
}
fn defer_score(
    board: &Board,
    pending: &mut PendingClear,
    cells: &[matching::Cell],
    eligible: bool,
) -> Result<(), ResolutionError> {
    let points = scoring::score_cells(board, cells).map_err(ResolutionError::Score)?;
    let target = if eligible {
        &mut pending.eligible_points
    } else {
        &mut pending.score_only
    };
    *target = target
        .checked_add(points)
        .ok_or(ResolutionError::Score(ScoreError::Overflow))?;
    Ok(())
}
fn horizontal_special(board: &Board, kind: BallKind, minimum: usize) -> Vec<Vec<matching::Cell>> {
    let mut groups = Vec::new();
    let mut visited = std::collections::BTreeSet::new();
    for cell in all_cells(board)
        .into_iter()
        .filter(|cell| board.stacks[cell.lane][cell.index].kind == kind)
    {
        if !visited.insert(cell) {
            continue;
        }
        let row = position(board, cell).row;
        let mut run = vec![cell];
        for lane in cell.lane + 1..LANE_COUNT {
            let offset = 9 + board.bonus_rows() - board.capacity(lane).unwrap_or(8);
            let Some(index) = row.checked_sub(offset) else {
                break;
            };
            if board.stacks[lane]
                .get(index)
                .is_some_and(|ball| ball.kind == kind)
            {
                let next = matching::Cell { lane, index };
                visited.insert(next);
                run.push(next);
            } else {
                break;
            }
        }
        if run.len() >= minimum {
            groups.push(run);
        }
    }
    groups
}
fn touches_positive(board: &Board, cell: matching::Cell) -> bool {
    let stack = &board.stacks[cell.lane];
    cell.index
        .checked_sub(1)
        .and_then(|index| stack.get(index))
        .is_some_and(|ball| ball.weight > 0)
        || stack
            .get(cell.index + 1)
            .is_some_and(|ball| ball.weight > 0)
}
fn remap_snapshot(
    cell: matching::Cell,
    removed: &std::collections::BTreeSet<matching::Cell>,
) -> Option<matching::Cell> {
    if removed.contains(&cell) {
        return None;
    }
    Some(matching::Cell {
        lane: cell.lane,
        index: cell.index
            - removed
                .iter()
                .filter(|old| old.lane == cell.lane && old.index < cell.index)
                .count(),
    })
}
struct UtilityContact {
    source: matching::Cell,
    kind: BallKind,
    position: PhysicalPosition,
    targets: Vec<matching::Cell>,
}
// Capture simultaneous contacts and physical targets before modifying the board.
// Original identities are remapped through each prior effect; destroying a queued
// utility discards its event rather than synthesizing a second activation.
fn activate_utilities(
    board: &mut Board,
    pending: &mut PendingClear,
    outcome: &mut Outcome,
) -> Result<bool, ResolutionError> {
    let snapshot = all_cells(board);
    let contacts: Vec<_> = snapshot
        .iter()
        .copied()
        .filter_map(|source| {
            let kind = board.stacks[source.lane][source.index].kind;
            if !matches!(
                kind,
                BallKind::RowLaser | BallKind::ColumnLaser | BallKind::Feather
            ) || !touches_positive(board, source)
            {
                return None;
            }
            let position = position(board, source);
            let targets = snapshot
                .iter()
                .copied()
                .filter(|cell| match kind {
                    BallKind::RowLaser => self::position(board, *cell).row == position.row,
                    BallKind::ColumnLaser | BallKind::Feather => cell.lane == source.lane,
                    _ => false,
                })
                .collect();
            Some(UtilityContact {
                source,
                kind,
                position,
                targets,
            })
        })
        .collect();
    let activated = !contacts.is_empty();
    let mut removed = std::collections::BTreeSet::new();
    for contact in contacts {
        let Some(source) = remap_snapshot(contact.source, &removed) else {
            continue;
        };
        let current: Vec<_> = contact
            .targets
            .iter()
            .filter_map(|cell| remap_snapshot(*cell, &removed))
            .collect();
        if contact.kind == BallKind::Feather {
            remove_cells(board, pending, &[source]);
            removed.insert(contact.source);
            for original in &contact.targets {
                if let Some(cell) = remap_snapshot(*original, &removed) {
                    board.stacks[cell.lane][cell.index].weight = 0;
                }
            }
            outcome.cleared_count += 1;
        } else {
            defer_score(board, pending, &current, false)?;
            remove_cells(board, pending, &current);
            removed.extend(contact.targets.iter().copied());
            outcome.cleared_count += current.len();
        }
        outcome.utility_effects.push(UtilityEffect {
            kind: contact.kind,
            position: contact.position,
            affected_count: current.len().saturating_sub(1),
        });
    }
    Ok(activated)
}
// Immediate special effects never dispatch the effects of destroyed non-bomb specials.
fn resolve_specials(
    board: &mut Board,
    pending: &mut PendingClear,
    outcome: &mut Outcome,
) -> Result<bool, ResolutionError> {
    let mut changed = false;
    loop {
        let snapshot = all_cells(board);
        let mut bombs = Vec::new();
        for cell in snapshot
            .iter()
            .copied()
            .filter(|cell| board.stacks[cell.lane][cell.index].kind == BallKind::Bomb)
        {
            if touches_positive(board, cell) {
                bombs.push(cell);
            }
        }
        let mut detonated = std::collections::BTreeSet::new();
        let mut destroyed = std::collections::BTreeSet::new();
        while let Some(bomb) = bombs.pop() {
            if !detonated.insert(bomb) {
                continue;
            }
            let center = position(board, bomb);
            outcome.blast_centers.push(center);
            for neighbor in &snapshot {
                let near = position(board, *neighbor);
                if center.lane.abs_diff(near.lane) <= 1 && center.row.abs_diff(near.row) <= 1 {
                    destroyed.insert(*neighbor);
                    if board.stacks[neighbor.lane][neighbor.index].kind == BallKind::Bomb {
                        bombs.push(*neighbor);
                    }
                }
            }
        }
        let exploded = !destroyed.is_empty();
        if exploded {
            let cells: Vec<_> = destroyed.into_iter().collect();
            defer_score(board, pending, &cells, true)?;
            outcome.blasted_count += cells.len();
            outcome.cleared_count += cells.len();
            remove_cells(board, pending, &cells);
            changed = true;
            continue;
        }
        if activate_utilities(board, pending, outcome)? {
            changed = true;
            continue;
        }
        let mut merged = false;
        for lane in 0..LANE_COUNT {
            let mut index = 0;
            while index + 1 < board.stacks[lane].len() {
                if board.stacks[lane][index].kind == BallKind::SilverStar
                    && board.stacks[lane][index + 1].kind == BallKind::SilverStar
                {
                    remove_cells(
                        board,
                        pending,
                        &[matching::Cell {
                            lane,
                            index: index + 1,
                        }],
                    );
                    board.stacks[lane][index] = Ball::gold_star();
                    pending
                        .cells
                        .retain(|cell| !(cell.lane == lane && cell.index == index));
                    merged = true;
                }
                index += 1;
            }
        }
        let silver = horizontal_special(board, BallKind::SilverStar, 3);
        if !silver.is_empty() {
            let cells = all_cells(board);
            defer_score(board, pending, &cells, false)?;
            outcome.cleared_count += cells.len();
            outcome.silver_clears += 1;
            remove_cells(board, pending, &cells);
            changed = true;
            continue;
        }
        let gold = horizontal_special(board, BallKind::GoldStar, 5);
        let consumed_gold = !gold.is_empty();
        if consumed_gold {
            let expansions = gold.len().min(3 - board.bonus_rows);
            let cells: Vec<_> = gold.into_iter().flatten().collect();
            remove_cells(board, pending, &cells);
            outcome.cleared_count += cells.len();
            board.bonus_rows += expansions;
            outcome.expansions += expansions;
        }
        if !(merged || exploded || consumed_gold) {
            break;
        }
        changed = true;
    }
    Ok(changed)
}
fn mark_clears(board: &Board, pending: &mut PendingClear) {
    let geometry = matching::capacities(board);
    let mut seeds = pending.cells.clone();
    seeds.extend(matching::cascade_at(board, &geometry));
    pending.cells = matching::expand_at(board, &geometry, &seeds);
}
fn prepare_phase(
    board: &mut Board,
    pending: &mut PendingClear,
    mut before: [Option<Balance>; SEESAW_COUNT],
) -> Result<Outcome, ResolutionError> {
    let mut outcome = empty_outcome();
    loop {
        resolve_specials(board, pending, &mut outcome)?;
        // Current capacities already reflect the landed weight's final tilt.
        // Give those physical-row matches priority while the complete stacks
        // still exist, before compression can erase a qualifying alignment.
        mark_clears(board, pending);
        if pending.cells.is_empty() {
            outcome.compressed_count += compress(board, pending)?;
        }
        launch_reversals(board, before, &mut outcome.flights, pending);
        before = balances(board);
        // A launch can change physical alignment. New special effects are resolved
        // against that geometry, with removal-induced flips measured from here.
        if !resolve_specials(board, pending, &mut outcome)? {
            break;
        }
    }
    mark_clears(board, pending);
    outcome.game_over = pending.is_empty() && board.is_game_over();
    Ok(outcome)
}
/// Preview whether this exact landing emits a flight from its selected seesaw.
/// Runs the real deterministic contact/compression/launch transaction on clones;
/// invalid or unrepresentable landings return false without mutating any state.
pub fn would_reverse(board: &Board, pending: &PendingClear, lane: usize, ball: &Ball) -> bool {
    let mut candidate = board.clone();
    let mut pending = pending.clone();
    prepare_landing(&mut candidate, &mut pending, lane, ball.clone()).is_ok_and(|outcome| {
        outcome
            .flights
            .iter()
            .any(|flight| flight.origin / 2 == lane / 2)
    })
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
    if !ball.is_valid() {
        return Err(ResolutionError::ZeroWeight);
    }
    let mut candidate = board.clone();
    let mut next_pending = pending.clone();
    let before = balances(&candidate);
    if next_pending.is_empty() {
        next_pending.trigger_lane = lane;
    }
    candidate.stacks[lane].push(ball);
    let outcome = prepare_phase(&mut candidate, &mut next_pending, before)?;
    *board = candidate;
    *pending = next_pending;
    Ok(outcome)
}
fn count_runs(values: impl IntoIterator<Item = usize>, minimum: usize) -> usize {
    let mut previous: Option<usize> = None;
    let mut length = 0;
    let mut count = 0;
    for value in values {
        if previous.and_then(|old| old.checked_add(1)) != Some(value) {
            count += usize::from(length >= minimum);
            length = 0;
        }
        length += 1;
        previous = Some(value);
    }
    count + usize::from(length >= minimum)
}
fn clear_rewards(
    board: &Board,
    selected: &[matching::Cell],
    lane: usize,
) -> Result<Vec<Reward>, ResolutionError> {
    use std::collections::{BTreeMap, BTreeSet};
    if selected.is_empty() {
        return Ok(Vec::new());
    }
    let cells: BTreeSet<_> = selected.iter().copied().collect();
    let mut rows = BTreeMap::<usize, BTreeMap<usize, matching::Cell>>::new();
    let mut columns = BTreeMap::<usize, BTreeSet<usize>>::new();
    for cell in &cells {
        rows.entry(position(board, *cell).row)
            .or_default()
            .insert(cell.lane, *cell);
        columns.entry(cell.lane).or_default().insert(cell.index);
    }
    // Adjacent selected balls still need a shared-color edge: independent red
    // and blue clears touching in one row do not form a five-ball matching run.
    let mut row_count = 0usize;
    for row in rows.values() {
        let mut previous: Option<matching::Cell> = None;
        let mut length = 0;
        for cell in row.values().copied() {
            let linked = previous.is_some_and(|old| {
                old.lane + 1 == cell.lane
                    && board.stacks[old.lane][old.index]
                        .colors
                        .iter()
                        .any(|color| board.stacks[cell.lane][cell.index].colors.contains(color))
            });
            if !linked {
                row_count += usize::from(length >= 5);
                length = 0;
            }
            length += 1;
            previous = Some(cell);
        }
        row_count += usize::from(length >= 5);
    }
    let qualified_columns = columns.iter().filter_map(|(lane, indices)| {
        (count_runs(indices.iter().copied(), 4) > 0).then_some(*lane)
    });
    let column_count = count_runs(qualified_columns, 3);
    let feather = cells
        .iter()
        .all(|cell| board.balance(cell.lane / 2) == Some(Balance::Balanced));
    let mut rewards = Vec::new();
    for (ball, count) in [
        (Ball::row_laser(), row_count),
        (Ball::column_laser(), column_count),
        (Ball::feather(), usize::from(feather)),
    ] {
        if count > 0 {
            rewards.push(Reward {
                lane,
                ball,
                count: u32::try_from(count)
                    .map_err(|_| ResolutionError::Score(ScoreError::Overflow))?,
            });
        }
    }
    Ok(rewards)
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
    let rewards = clear_rewards(&candidate, &next_pending.cells, next_pending.trigger_lane)?;
    let ordinary =
        scoring::score_cells(&candidate, &next_pending.cells).map_err(ResolutionError::Score)?;
    let eligible = ordinary
        .checked_add(next_pending.eligible_points)
        .ok_or(ResolutionError::Score(ScoreError::Overflow))?;
    let score_only = next_pending.score_only;
    let old_level = next_progression.level().get();
    next_progression
        .award(eligible)
        .map_err(ResolutionError::Score)?;
    next_progression
        .award_score_only(score_only)
        .map_err(ResolutionError::Score)?;
    let points = eligible
        .checked_add(score_only)
        .ok_or(ResolutionError::Score(ScoreError::Overflow))?;
    let reward_count = next_progression.level().get() - old_level;
    let reward_lane = next_pending.trigger_lane;
    let cleared_count = next_pending.cells.len();
    let cells = std::mem::take(&mut next_pending.cells);
    remove_cells(&mut candidate, &mut next_pending, &cells);
    next_pending.eligible_points = 0;
    next_pending.score_only = 0;
    let mut outcome = prepare_phase(&mut candidate, &mut next_pending, before)?;
    outcome.points = points;
    outcome.cleared_count += cleared_count;
    outcome.stars_earned = reward_count;
    if reward_count > 0 {
        outcome.rewards.push(Reward {
            lane: reward_lane,
            ball: Ball::silver_star(),
            count: reward_count,
        });
    }
    outcome.rewards.extend(rewards);
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
        outcome.rewards.extend(next.rewards);
        outcome.stars_earned += next.stars_earned;
        outcome.blasted_count += next.blasted_count;
        outcome.silver_clears += next.silver_clears;
        outcome.expansions += next.expansions;
        outcome.blast_centers.extend(next.blast_centers);
        outcome.utility_effects.extend(next.utility_effects);
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
        Ball {
            kind: crate::BallKind::Normal,
            weight,
            colors,
        }
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
    fn pending_clear_protects_five_followup_balls_from_premature_compression() {
        let (mut board, mut pending) = marked_four();
        let mut progression = progress();
        let mut compressed = 0;
        for _ in 0..4 {
            let Ok(outcome) = prepare_landing(&mut board, &mut pending, 0, ball(1, &[0])) else {
                panic!("followup");
            };
            compressed += outcome.compressed_count;
        }
        assert_eq!(compressed, 0);
        assert_eq!(pending.cells().len(), 8);
        assert_eq!(board.stack(0), Some(vec![ball(1, &[0]); 5].as_slice()));
        assert!(pending.cells().iter().all(|cell| {
            board
                .stack(cell.lane)
                .is_some_and(|stack| cell.index < stack.len())
        }));
        let Ok(outcome) = commit_clear(&mut board, &mut progression, &mut pending) else {
            panic!("commit");
        };
        assert_eq!(outcome.points, 800);
        assert_eq!(outcome.cleared_count, 8);
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
    fn combined_review_fixture_clears_the_full_uncompressed_joker_cascade() {
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
        assert_eq!(outcome.compressed_count, 0);
        assert_eq!(outcome.cleared_count, 11);
        assert_eq!(outcome.points, 1200);
        assert!(outcome.flights.is_empty());
        assert_eq!(board.stack(6), Some([].as_slice()));
        assert_eq!(weight(&board), 0);
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
    fn horizontal_clear_wins_when_landing_also_completes_vertical_five() {
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
        assert_eq!(outcome.compressed_count, 0);
        assert_eq!(outcome.cleared_count, 7);
        assert_eq!(progression.score(), 700);
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
            land(
                &mut board,
                &mut progression,
                0,
                Ball::normal(0, BallColors::none())
            ),
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

#[cfg(test)]
mod special_tests {
    use super::*;
    use crate::{BallColors, ColorId};
    use std::num::NonZeroU32;
    fn ordinary(weight: u32, color: u32) -> Ball {
        Ball::normal(weight, BallColors::single(ColorId::new(color)))
    }
    fn progression() -> Progression {
        Progression::new(NonZeroU32::MIN)
    }
    fn count(board: &Board) -> usize {
        board.stacks.iter().map(Vec::len).sum()
    }

    #[test]
    fn bomb_is_inert_on_empty_and_weightless_contact_but_fires_from_above() {
        let mut board = Board::default();
        let mut score = progression();
        assert_eq!(
            land(&mut board, &mut score, 0, Ball::bomb())
                .unwrap_or_else(|error| panic!("{error:?}"))
                .blasted_count,
            0
        );
        assert_eq!(
            land(&mut board, &mut score, 0, Ball::silver_star())
                .unwrap_or_else(|error| panic!("{error:?}"))
                .blasted_count,
            0
        );
        // The star shields the bomb from direct vertical contact.
        assert_eq!(
            land(&mut board, &mut score, 0, ordinary(1, 0))
                .unwrap_or_else(|error| panic!("{error:?}"))
                .blasted_count,
            0
        );
        let mut board = Board::default();
        board.place(0, Ball::bomb());
        let effect = land(&mut board, &mut score, 0, ordinary(1, 0))
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(effect.blasted_count, 2);
        assert_eq!(effect.points, 1100);
        assert_eq!(effect.stars_earned, 1);
        assert_eq!(
            effect.rewards,
            vec![Reward {
                lane: 0,
                ball: Ball::silver_star(),
                count: 1
            }]
        );
        assert_eq!(count(&board), 0);
    }
    #[test]
    fn bomb_landing_on_positive_weight_also_fires_from_below() {
        let mut board = Board::default();
        board.place(3, Ball::stone());
        let effect = land(&mut board, &mut progression(), 3, Ball::bomb())
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(effect.blasted_count, 2);
        assert_eq!(effect.points, 1500);
        assert_eq!(
            effect.blast_centers,
            vec![PhysicalPosition { lane: 3, row: 1 }]
        );
    }
    #[test]
    fn bomb_on_bomb_uses_both_snapshot_centers_and_scores_union_once() {
        let mut board = Board::default();
        board.place(0, Ball::bomb());
        let effect = land(&mut board, &mut progression(), 0, Ball::bomb())
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(effect.blasted_count, 2);
        assert_eq!(effect.blast_centers.len(), 2);
        assert_eq!(effect.points, 2000);
    }
    #[test]
    fn blasts_chain_diagonally_using_tilted_snapshot_geometry() {
        let mut board = Board::default();
        board.place(0, ordinary(5, 0));
        board.place(1, Ball::bomb());
        board.place(1, Ball::stone());
        board.place(2, ordinary(1, 2));
        let effect = land(&mut board, &mut progression(), 0, Ball::bomb())
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(effect.blast_centers.len(), 2);
        assert!(
            effect
                .blast_centers
                .contains(&PhysicalPosition { lane: 0, row: 2 })
        );
        assert!(
            effect
                .blast_centers
                .contains(&PhysicalPosition { lane: 1, row: 1 })
        );
        assert_eq!(effect.blasted_count, 5);
        assert_eq!(effect.points, 3100);
        assert_eq!(count(&board), 0);
    }
    #[test]
    fn blast_respects_real_height_and_does_not_wrap_at_edges() {
        let mut board = Board::default();
        board.place(0, Ball::bomb()); // heavy row zero
        board.place(1, Ball::silver_star()); // light row two, outside center+1
        board.place(7, ordinary(1, 7));
        let effect = land(&mut board, &mut progression(), 0, ordinary(1, 0))
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(effect.blasted_count, 2);
        assert_eq!(
            board.stack(1).unwrap_or_else(|| panic!("valid lane")),
            &[Ball::silver_star()]
        );
        assert_eq!(
            board.stack(7).unwrap_or_else(|| panic!("valid lane")).len(),
            1
        );
    }
    #[test]
    fn blast_destroys_gold_without_expanding_or_activating_other_specials() {
        let mut board = Board::default();
        board.place(0, ordinary(10, 1));
        board.place(1, Ball::bomb()); // balanced pair, row one
        for lane in 2..7 {
            board.place(lane, Ball::gold_star());
        }
        // Bomb's upper neighbor is at physical row two; adjacent gold starts row one.
        let effect = land(&mut board, &mut progression(), 1, ordinary(1, 2))
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(effect.expansions, 0);
        assert_eq!(board.bonus_rows(), 0);
        assert!(
            board
                .stack(2)
                .unwrap_or_else(|| panic!("valid lane"))
                .is_empty()
        );
        assert_eq!(
            board.stack(3).unwrap_or_else(|| panic!("valid lane")),
            &[Ball::gold_star()]
        );
    }
    #[test]
    fn silver_vertical_contact_merges_without_mass_score_or_compression() {
        let mut board = Board::default();
        let mut score = progression();
        land(&mut board, &mut score, 0, Ball::silver_star())
            .unwrap_or_else(|error| panic!("{error:?}"));
        let effect = land(&mut board, &mut score, 0, Ball::silver_star())
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(
            board.stack(0).unwrap_or_else(|| panic!("valid lane")),
            &[Ball::gold_star()]
        );
        assert_eq!(effect.compressed_count, 0);
        assert_eq!(effect.points, 0);
        assert_eq!(score.score(), 0);
        assert_eq!(board.total(0), 0);
    }
    #[test]
    fn three_silver_clear_everything_without_level_progress_or_bomb_activation() {
        let mut board = Board::default();
        for lane in [0, 1] {
            board.place(lane, Ball::silver_star());
        }
        board.place(4, Ball::stone());
        board.place(5, ordinary(7, 9));
        board.place(6, Ball::bomb()); // inert and destroyed without detonation
        let mut score = progression();
        score.award(900).unwrap_or_else(|error| panic!("{error:?}"));
        let effect = land(&mut board, &mut score, 2, Ball::silver_star())
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(effect.silver_clears, 1);
        assert_eq!(effect.blasted_count, 0);
        assert_eq!(effect.points, 2200);
        assert_eq!(score.score(), 3100);
        assert_eq!(score.level(), NonZeroU32::MIN);
        assert_eq!(score.remaining(), 100);
        assert_eq!(effect.stars_earned, 0);
        assert_eq!(count(&board), 0);
    }
    #[test]
    fn gold_expansions_are_permanent_capped_and_preserve_physical_rows() {
        let mut board = Board::default();
        let mut score = progression();
        board.place(7, ordinary(4, 2));
        let before = position(&board, matching::Cell { lane: 7, index: 0 });
        for expected in 1..=3 {
            for lane in 0..4 {
                board.place(lane, Ball::gold_star());
            }
            let effect = land(&mut board, &mut score, 4, Ball::gold_star())
                .unwrap_or_else(|error| panic!("{error:?}"));
            assert_eq!(effect.expansions, 1);
            assert_eq!(board.bonus_rows(), expected);
            assert_eq!(board.capacity(7), Some(9 + expected));
            assert_eq!(
                position(&board, matching::Cell { lane: 7, index: 0 }),
                before
            );
            assert_eq!(board.expansion_achievement(), expected == 3);
            assert_eq!(score.score(), 0);
        }
        for lane in 0..4 {
            board.place(lane, Ball::gold_star());
        }
        assert_eq!(
            land(&mut board, &mut score, 4, Ball::gold_star())
                .unwrap_or_else(|error| panic!("{error:?}"))
                .expansions,
            0
        );
        assert_eq!(board.bonus_rows(), 3);
        assert!(
            board
                .stack(0)
                .unwrap_or_else(|| panic!("valid lane"))
                .is_empty()
        );
        board.clear();
        assert_eq!(board.bonus_rows(), 0);
        assert!(!board.expansion_achievement());
    }
    #[test]
    fn six_gold_in_one_row_are_all_consumed_for_one_expansion() {
        let mut board = Board::default();
        for lane in 0..5 {
            board.place(lane, Ball::gold_star());
        }
        let effect = land(&mut board, &mut progression(), 5, Ball::gold_star())
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(effect.expansions, 1);
        assert_eq!(effect.cleared_count, 6);
        assert_eq!(count(&board), 0);
    }
    #[test]
    fn star_edges_convert_only_when_unwrapped_path_really_crosses() {
        for kind in [Ball::silver_star(), Ball::gold_star()] {
            assert_eq!(convert_edge(kind.clone(), 0, 7, true), kind);
            assert_eq!(convert_edge(kind.clone(), 7, 7, false), kind);
            assert_eq!(convert_edge(kind.clone(), 0, 8, true), Ball::bomb());
            assert_eq!(convert_edge(kind.clone(), 7, 8, false), Ball::bomb());
            assert_eq!(convert_edge(kind.clone(), 7, u64::MAX, true), Ball::bomb());
            assert_eq!(convert_edge(kind, 0, u64::MAX, false), Ball::bomb());
        }
        assert_eq!(convert_edge(Ball::stone(), 0, 20, false), Ball::stone());
        assert_eq!(convert_edge(Ball::bomb(), 0, 20, false), Ball::bomb());
    }
    #[test]
    fn launched_star_conversion_is_owned_once_with_original_visual_kind() {
        let mut board = Board::default();
        board.place(0, ordinary(2, 0));
        board.place(0, Ball::silver_star());
        board.place(1, ordinary(1, 1));
        let effect = land(&mut board, &mut progression(), 1, ordinary(10, 2))
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(effect.flights.len(), 1);
        let flight = &effect.flights[0];
        assert_eq!(flight.origin, 0);
        assert_eq!(flight.distance, 9);
        assert_eq!(flight.destination, 1);
        assert_eq!(flight.ball, Ball::bomb());
        assert_eq!(flight.edge_from, Some(BallKind::SilverStar));
        assert_eq!(flight.origin_bonus_rows, 0);
    }
    #[test]
    fn invalid_special_mass_and_palette_reject_atomically() {
        let mut board = Board::default();
        let mut pending = PendingClear::default();
        for invalid in [
            Ball {
                kind: BallKind::Bomb,
                weight: 9,
                colors: BallColors::none(),
            },
            Ball {
                kind: BallKind::Stone,
                weight: 5,
                colors: BallColors::single(ColorId::RED),
            },
            Ball {
                kind: BallKind::Normal,
                weight: 0,
                colors: BallColors::none(),
            },
        ] {
            assert!(!board.place(0, invalid.clone()));
            let before = (board.clone(), pending.clone());
            assert_eq!(
                prepare_landing(&mut board, &mut pending, 0, invalid),
                Err(ResolutionError::ZeroWeight)
            );
            assert_eq!((board.clone(), pending.clone()), before);
        }
    }
    #[test]
    fn deferred_explosion_score_failure_preserves_pending_and_board() {
        let mut board = Board::default();
        board.place(0, Ball::bomb());
        let mut pending = PendingClear::default();
        prepare_landing(&mut board, &mut pending, 0, ordinary(1, 0))
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert!(!pending.is_empty());
        assert!(pending.cells().is_empty());
        let mut score = progression();
        score
            .award_score_only(u64::MAX)
            .unwrap_or_else(|error| panic!("{error:?}"));
        let before = (board.clone(), pending.clone(), score.clone());
        assert_eq!(
            commit_clear(&mut board, &mut score, &mut pending),
            Err(ResolutionError::Score(ScoreError::Overflow))
        );
        assert_eq!((board, pending, score), before);
    }
    #[test]
    fn earned_stars_keep_initial_trigger_lane_across_followup_window() {
        let mut board = Board::default();
        let mut pending = PendingClear::default();
        for lane in 0..3 {
            board.place(lane, ordinary(4, 0));
        }
        prepare_landing(&mut board, &mut pending, 3, ordinary(4, 0))
            .unwrap_or_else(|error| panic!("{error:?}"));
        prepare_landing(&mut board, &mut pending, 2, ordinary(4, 0))
            .unwrap_or_else(|error| panic!("{error:?}"));
        let mut score = progression();
        let effect = commit_clear(&mut board, &mut score, &mut pending)
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(effect.stars_earned, 1);
        assert_eq!(
            effect.rewards,
            vec![Reward {
                lane: 3,
                ball: Ball::silver_star(),
                count: 1
            }]
        );
    }
    #[test]
    fn synchronous_blast_overflow_rolls_back_landing_and_score() {
        let mut board = Board::default();
        board.place(0, Ball::bomb());
        let mut score = progression();
        score
            .award_score_only(u64::MAX)
            .unwrap_or_else(|error| panic!("{error:?}"));
        let before = (board.clone(), score.clone());
        assert_eq!(
            land(&mut board, &mut score, 0, ordinary(1, 0)),
            Err(ResolutionError::Score(ScoreError::Overflow))
        );
        assert_eq!((board, score), before);
    }
    #[test]
    fn large_real_clear_earns_one_compact_reward_run() {
        let mut board = Board::default();
        for lane in 0..3 {
            board.place(lane, ordinary(u32::MAX, 0));
        }
        let mut score = progression();
        let effect = land(&mut board, &mut score, 3, ordinary(u32::MAX, 0))
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert!(effect.stars_earned > 1000);
        assert_eq!(
            effect
                .rewards
                .iter()
                .filter(|reward| reward.ball.kind == BallKind::SilverStar)
                .count(),
            1
        );
        assert_eq!(effect.rewards[0].count, effect.stars_earned);
        assert_eq!(effect.rewards[0].lane, 3);
        assert_eq!(effect.stars_earned, score.level().get() - 1);
    }
    #[test]
    fn inert_stones_and_stars_never_match_or_compress_as_normal_balls() {
        let mut board = Board::default();
        for _ in 0..5 {
            board.place(0, Ball::stone());
        }
        let effect = land(&mut board, &mut progression(), 0, Ball::stone())
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(effect.compressed_count, 0);
        assert_eq!(count(&board), 6);
        assert!(matching::cascade_cells(&board).is_empty());
        assert_eq!(effect.points, 0);
    }
    #[test]
    fn independent_gold_rows_each_earn_one_expansion_but_long_run_only_one() {
        let mut board = Board::default();
        for lane in 0..5 {
            for _ in 0..2 {
                board.place(lane, Ball::gold_star());
            }
        }
        let effect = land(&mut board, &mut progression(), 7, Ball::silver_star())
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(effect.expansions, 2);
        assert_eq!(board.bonus_rows(), 2);
        assert_eq!(effect.cleared_count, 10);
        let mut board = Board::default();
        for lane in 0..7 {
            board.place(lane, Ball::gold_star());
        }
        let effect = land(&mut board, &mut progression(), 7, Ball::gold_star())
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(effect.expansions, 1);
        assert_eq!(effect.cleared_count, 8);
    }
    #[test]
    fn existing_bomb_contact_uses_uncompressed_physical_snapshot() {
        let mut board = Board::default();
        for _ in 0..5 {
            board.place(0, ordinary(1, 0));
        }
        board.place(0, Ball::bomb());
        let effect = land(&mut board, &mut progression(), 7, Ball::silver_star())
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(effect.compressed_count, 0);
        assert_eq!(effect.blasted_count, 2);
        assert_eq!(board.stack(0).map(<[Ball]>::len), Some(4));
        assert_eq!(
            effect.blast_centers,
            vec![PhysicalPosition { lane: 0, row: 5 }]
        );
    }
    #[test]
    fn reversal_preview_uses_real_effects_and_never_mutates_the_board() {
        let mut board = Board::default();
        board.place(0, ordinary(2, 0));
        board.place(1, ordinary(1, 1));
        let before = board.clone();
        assert!(!would_reverse(
            &board,
            &PendingClear::default(),
            1,
            &ordinary(1, 2)
        )); // equal end weights
        assert!(would_reverse(
            &board,
            &PendingClear::default(),
            1,
            &ordinary(2, 2)
        )); // strict reversal
        assert!(!would_reverse(
            &board,
            &PendingClear::default(),
            1,
            &Ball::silver_star()
        ));
        // A bomb contacting the existing ball destroys both sides instead of flipping.
        assert!(!would_reverse(
            &board,
            &PendingClear::default(),
            1,
            &Ball::bomb()
        ));
        assert!(!would_reverse(
            &board,
            &PendingClear::default(),
            LANE_COUNT,
            &ordinary(2, 2)
        ));
        assert_eq!(board, before);
        let empty = Board::default();
        assert!(!would_reverse(
            &empty,
            &PendingClear::default(),
            0,
            &ordinary(10, 0)
        )); // balanced -> heavy
        assert!(!would_reverse(
            &empty,
            &PendingClear::default(),
            0,
            &Ball::bomb()
        ));
    }
    #[test]
    fn reversal_preview_handles_owned_star_launches_and_invalid_ball_invariants() {
        let mut board = Board::default();
        board.place(0, ordinary(2, 0));
        board.place(0, Ball::gold_star());
        board.place(1, ordinary(1, 1));
        assert!(would_reverse(
            &board,
            &PendingClear::default(),
            1,
            &ordinary(10, 2)
        ));
        let mut invalid = Ball::silver_star();
        invalid.weight = 1;
        assert!(!would_reverse(
            &board,
            &PendingClear::default(),
            1,
            &invalid
        ));
        assert_eq!(board.stack(0).map(<[Ball]>::len), Some(2));
    }
    #[test]
    fn simultaneous_match_and_five_stay_uncompressed_until_animation_commit() {
        let mut board = Board::default();
        for _ in 0..4 {
            board.place(0, ordinary(1, 0));
        }
        board.place(1, ordinary(1, 0));
        board.place(2, ordinary(1, 8));
        board.place(2, ordinary(1, 8));
        board.place(2, ordinary(1, 0));
        let mut pending = PendingClear::default();
        let mut score = progression();
        let effect = prepare_landing(&mut board, &mut pending, 0, ordinary(1, 0))
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(effect.compressed_count, 0);
        assert_eq!(pending.cells().len(), 7);
        assert_eq!(board.stack(0).map(<[Ball]>::len), Some(5));
        assert_eq!(score.score(), 0);
        let committed = commit_clear(&mut board, &mut score, &mut pending)
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(committed.compressed_count, 0);
        assert_eq!(committed.cleared_count, 7);
        assert_eq!(score.score(), 700);
        assert_eq!(board.total(0), 0);
    }
    #[test]
    fn unrelated_five_waits_for_pending_clear_then_compresses_after_commit() {
        let mut board = Board::default();
        for lane in 0..3 {
            board.place(lane, ordinary(1, 0));
        }
        for _ in 0..4 {
            board.place(6, ordinary(1, 1));
        }
        let mut pending = PendingClear::default();
        prepare_landing(&mut board, &mut pending, 3, ordinary(1, 0))
            .unwrap_or_else(|error| panic!("{error:?}"));
        let incoming = prepare_landing(&mut board, &mut pending, 6, ordinary(1, 1))
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(incoming.compressed_count, 0);
        assert_eq!(board.stack(6).map(<[Ball]>::len), Some(5));
        let committed = commit_clear(&mut board, &mut progression(), &mut pending)
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(committed.points, 400);
        assert_eq!(committed.compressed_count, 1);
        assert_eq!(board.stack(6), Some([ordinary(5, 1)].as_slice()));
    }
    #[test]
    fn no_match_still_compresses_rising_stack_before_launching_its_top() {
        let mut board = Board::default();
        for _ in 0..5 {
            board.place(0, ordinary(1, 0));
        }
        board.place(1, ordinary(4, 1));
        let effect = land(&mut board, &mut progression(), 1, ordinary(2, 2))
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(effect.compressed_count, 1);
        assert_eq!(effect.flights.len(), 1);
        assert_eq!(effect.flights[0].ball, ordinary(5, 0));
        assert_eq!(effect.flights[0].distance, 1);
        assert_eq!(effect.flights[0].destination, 1);
        assert_eq!(board.total(0), 0);
    }
    #[test]
    fn preview_clones_live_sticky_membership_instead_of_recompressing_protected_balls() {
        let mut board = Board::default();
        for _ in 0..4 {
            board.place(0, ordinary(u32::MAX, 0));
        }
        board.place(1, ordinary(u32::MAX, 0));
        board.place(2, ordinary(1, 1));
        board.place(2, ordinary(1, 2));
        board.place(2, ordinary(1, 0));
        let mut pending = PendingClear::default();
        prepare_landing(&mut board, &mut pending, 0, ordinary(u32::MAX, 0))
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert!(!pending.cells().is_empty());
        for (color, weight) in [
            (10, u32::MAX),
            (11, u32::MAX),
            (12, u32::MAX),
            (13, u32::MAX - 1),
        ] {
            prepare_landing(&mut board, &mut pending, 1, ordinary(weight, color))
                .unwrap_or_else(|error| panic!("{error:?}"));
        }
        let before = (board.clone(), pending.clone());
        let held = ordinary(2, 20);
        assert!(would_reverse(&board, &pending, 1, &held));
        assert!(!would_reverse(&board, &PendingClear::default(), 1, &held));
        assert_eq!((board.clone(), pending.clone()), before);
        let real = prepare_landing(&mut board, &mut pending, 1, held)
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(real.flights.len(), 1);
        assert_eq!(real.compressed_count, 0);
        assert_eq!(real.flights[0].ball.weight, u32::MAX);
    }
}

#[cfg(test)]
#[path = "utility_tests.rs"]
mod utility_tests;
