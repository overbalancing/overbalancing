//! Horizontal seeds and connected joker-aware color cascades.
use crate::{Ball, BallKind, Board, ColorId, LANE_COUNT};
use std::collections::BTreeSet;

/// Stable identity of a ball for one immutable board snapshot.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Cell {
    /// Stack lane, numbered left to right.
    pub lane: usize,
    /// Bottom-to-top index inside the stack.
    pub index: usize,
}
/// One connected component of a single color containing a horizontal run of three.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchGroup {
    /// Color used for every edge in this component.
    pub color: ColorId,
    /// Sorted ball identities; multi-color balls can occur in multiple groups.
    pub cells: Vec<Cell>,
}
/// Shared ordinary-color edge, including universal jokers but excluding colorless specials.
pub(crate) fn shares_color(first: &Ball, second: &Ball) -> bool {
    if !matches!(first.kind, BallKind::Normal | BallKind::UltimateJoker)
        || !matches!(second.kind, BallKind::Normal | BallKind::UltimateJoker)
    {
        return false;
    }
    first.kind == BallKind::UltimateJoker
        || second.kind == BallKind::UltimateJoker
        || first
            .colors
            .iter()
            .any(|color| second.colors.contains(color))
}
fn has_color(ball: &Ball, color: ColorId) -> bool {
    ball.kind == BallKind::UltimateJoker
        || (ball.kind == BallKind::Normal && ball.colors.contains(color))
}
fn ball(board: &Board, cell: Cell) -> Option<&Ball> {
    board.stack(cell.lane)?.get(cell.index)
}
fn row(capacities: &[usize; LANE_COUNT], cell: Cell) -> usize {
    cell.index + (9 - capacities[cell.lane])
}
fn neighbors(board: &Board, capacities: &[usize; LANE_COUNT], cell: Cell) -> Vec<Cell> {
    let mut result = Vec::with_capacity(4);
    if cell.index > 0 {
        result.push(Cell {
            index: cell.index - 1,
            ..cell
        });
    }
    if let Some(index) = cell.index.checked_add(1) {
        let next = Cell { index, ..cell };
        if ball(board, next).is_some() {
            result.push(next);
        }
    }
    let physical_row = row(capacities, cell);
    for lane in [
        cell.lane.checked_sub(1),
        cell.lane.checked_add(1).filter(|lane| *lane < LANE_COUNT),
    ]
    .into_iter()
    .flatten()
    {
        let offset = 9 - capacities[lane];
        if let Some(index) = physical_row.checked_sub(offset) {
            let next = Cell { lane, index };
            if ball(board, next).is_some() {
                result.push(next);
            }
        }
    }
    result
}
fn horizontal_seed(capacities: &[usize; LANE_COUNT], cells: &BTreeSet<Cell>) -> bool {
    cells.iter().any(|cell| {
        if cell.lane + 2 >= LANE_COUNT {
            return false;
        }
        let physical_row = row(capacities, *cell);
        [cell.lane + 1, cell.lane + 2].into_iter().all(|lane| {
            physical_row
                .checked_sub(9 - capacities[lane])
                .is_some_and(|index| cells.contains(&Cell { lane, index }))
        })
    })
}
/// Find qualifying components independently for each color on the current board.
///
/// Horizontal adjacency uses global physical rows: stack index plus nine minus
/// current lane capacity. Vertical adjacency uses successive stack indices.
/// A component qualifies only if it contains three consecutive horizontal cells.
/// Universal jokers participate in every concrete-color component. A universal-only
/// triple has no concrete color and is represented by cascade_cells instead.
/// This function neither removes balls nor chooses cross-color joker behavior.
pub fn detect(board: &Board) -> Vec<MatchGroup> {
    detect_at(board, &capacities(board))
}
fn detect_at(board: &Board, capacities: &[usize; LANE_COUNT]) -> Vec<MatchGroup> {
    let mut visited = BTreeSet::new();
    let mut groups = Vec::new();
    for lane in 0..LANE_COUNT {
        for (index, current) in board.stack(lane).unwrap_or_default().iter().enumerate() {
            let start = Cell { lane, index };
            for color in current.colors.iter() {
                if !visited.insert((color, start)) {
                    continue;
                }
                let mut pending = vec![start];
                let mut cells = BTreeSet::from([start]);
                while let Some(cell) = pending.pop() {
                    for next in neighbors(board, capacities, cell) {
                        if ball(board, next).is_some_and(|ball| has_color(ball, color))
                            && visited.insert((color, next))
                        {
                            cells.insert(next);
                            pending.push(next);
                        }
                    }
                }
                if horizontal_seed(capacities, &cells) {
                    groups.push(MatchGroup {
                        color,
                        cells: cells.into_iter().collect(),
                    });
                }
            }
        }
    }
    groups.sort_by_key(|group| (group.color, group.cells.first().copied()));
    groups
}
/// Union matched ball identities so overlapping color groups never double-score a ball.
pub fn unique_cells(groups: &[MatchGroup]) -> Vec<Cell> {
    groups
        .iter()
        .flat_map(|group| group.cells.iter().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Snapshot platform capacities for one frozen-geometry resolution phase.
pub(crate) fn capacities(board: &Board) -> [usize; LANE_COUNT] {
    std::array::from_fn(|lane| board.capacity(lane).unwrap_or(8) - board.bonus_rows())
}
/// Expand every horizontal seed through orthogonal shared-color edges.
/// A joker may switch the traversed color; ball identities are returned once.
pub fn cascade_cells(board: &Board) -> Vec<Cell> {
    cascade_at(board, &capacities(board))
}
pub(crate) fn cascade_at(board: &Board, capacities: &[usize; LANE_COUNT]) -> Vec<Cell> {
    let groups = detect_at(board, capacities);
    let mut seeds = unique_cells(&groups);
    // A universal-only horizontal triple has no concrete ColorId to enumerate.
    // Seed those identities directly instead of inventing a reserved color value.
    for lane in 0..LANE_COUNT.saturating_sub(2) {
        for (index, ball) in board.stack(lane).unwrap_or_default().iter().enumerate() {
            if ball.kind != BallKind::UltimateJoker {
                continue;
            }
            let start = Cell { lane, index };
            let physical_row = row(capacities, start);
            let mut run = vec![start];
            for (next_lane, capacity) in capacities.iter().enumerate().skip(lane + 1).take(2) {
                let Some(next_index) = physical_row.checked_sub(9 - capacity) else {
                    break;
                };
                let next = Cell {
                    lane: next_lane,
                    index: next_index,
                };
                if self::ball(board, next).is_some_and(|ball| ball.kind == BallKind::UltimateJoker)
                {
                    run.push(next);
                } else {
                    break;
                }
            }
            if run.len() == 3 {
                seeds.extend(run);
            }
        }
    }
    expand_at(board, capacities, &seeds)
}
pub(crate) fn expand_at(
    board: &Board,
    capacities: &[usize; LANE_COUNT],
    seeds: &[Cell],
) -> Vec<Cell> {
    let mut selected: BTreeSet<_> = seeds
        .iter()
        .copied()
        .filter(|cell| ball(board, *cell).is_some())
        .collect();
    let mut pending: Vec<_> = selected.iter().copied().collect();
    while let Some(cell) = pending.pop() {
        let Some(current) = ball(board, cell) else {
            continue;
        };
        for next in neighbors(board, capacities, cell) {
            if ball(board, next).is_some_and(|neighbor| shares_color(current, neighbor))
                && selected.insert(next)
            {
                pending.push(next);
            }
        }
    }
    selected.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BallColors, ColorId};
    fn add(board: &mut Board, lane: usize, weight: u32, colors: &[ColorId]) {
        let Some(colors) = BallColors::new(colors) else {
            panic!("valid palette");
        };
        assert!(board.place(
            lane,
            Ball {
                kind: crate::BallKind::Normal,
                weight,
                colors
            }
        ));
    }
    #[test]
    fn horizontal_seed_clears_detection_for_entire_same_color_component() {
        let mut board = Board::default();
        // Equal pair totals keep all three base cells on global row one.
        for lane in 0..4 {
            add(&mut board, lane, 1, &[ColorId::RED]);
        }
        for lane in 0..2 {
            add(&mut board, lane, 1, &[ColorId::RED]);
        }
        let groups = detect(&board);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].cells.len(), 6);
        assert_eq!(groups[0].color, ColorId::RED);
    }
    #[test]
    fn vertical_only_and_two_horizontal_balls_do_not_qualify() {
        let mut board = Board::default();
        for _ in 0..4 {
            add(&mut board, 0, 1, &[ColorId::RED]);
        }
        assert!(detect(&board).is_empty());
        add(&mut board, 1, 4, &[ColorId::RED]);
        assert!(detect(&board).is_empty());
    }
    #[test]
    fn physical_rows_account_for_raised_and_lowered_platforms() {
        let mut board = Board::default();
        // Lanes zero and two are heavy (row zero), lane one light (row two).
        add(&mut board, 0, 3, &[ColorId::RED]);
        add(&mut board, 1, 1, &[ColorId::RED]);
        add(&mut board, 2, 3, &[ColorId::RED]);
        assert!(detect(&board).is_empty());
        // Raise heavy-lane stack heights to reach the same physical row two.
        for lane in [0, 2] {
            for _ in 0..2 {
                add(&mut board, lane, 3, &[ColorId::RED]);
            }
        }
        let groups = detect(&board);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].cells.len(), 7);
    }
    #[test]
    fn joker_membership_is_per_color_and_union_counts_shared_balls_once() {
        let mut board = Board::default();
        for lane in 0..4 {
            add(&mut board, lane, 1, &[ColorId::RED, ColorId::BLUE]);
        }
        let groups = detect(&board);
        assert_eq!(groups.len(), 2);
        assert_eq!(unique_cells(&groups).len(), 4);
        // A red/blue ball does not let a red component traverse a blue-only ball.
        let mut bridge = Board::default();
        for (lane, colors) in [
            (0, vec![ColorId::RED]),
            (1, vec![ColorId::RED, ColorId::BLUE]),
            (2, vec![ColorId::BLUE]),
            (3, vec![ColorId::BLUE]),
        ] {
            add(&mut bridge, lane, 1, &colors);
        }
        let groups = detect(&bridge);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].color, ColorId::BLUE);
        assert!(!groups[0].cells.contains(&Cell { lane: 0, index: 0 }));
    }
}
