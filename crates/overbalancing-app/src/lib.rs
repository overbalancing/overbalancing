//! Level-driven supply and fixed-stack board shared by desktop and mobile.
mod ball_visual;
use bevy::log::{error, info, warn};
use bevy::{prelude::*, window::PrimaryWindow};
use overbalancing_core::supply::{GenerationError, GeneratorConfig, Supply};
use overbalancing_core::{
    Ball, BallColors, BallKind, Board, ColorId, Dispenser, LANE_COUNT, SEESAW_COUNT,
};
use overbalancing_core::{matching::Cell, resolution, scoring::Progression};
use std::collections::VecDeque;
use std::{
    num::NonZeroU32,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
// SplitMix64 transition described by https://prng.di.unimi.it/splitmix64.c.
// Non-cryptographic gameplay samples; core receives samples rather than entropy.
#[derive(Clone, Copy)]
struct Random(u64);
impl Random {
    fn seeded() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        Self(time ^ COUNTER.fetch_add(0x9e3779b97f4a7c15, Ordering::Relaxed))
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
        value ^ (value >> 31)
    }
}

const WIDTH: f32 = 920.0;
const HEIGHT: f32 = 2100.0;
const LEFT: f32 = -400.0;
const LANE: f32 = 100.0;
const ROW_HEIGHT: f32 = 96.0;
const CLEAR_WINDOW: f64 = 0.45;
const INK: Color = Color::srgb(0.85, 0.91, 0.98);

#[derive(Resource)]
struct Playfield {
    board: Board,
    chute: Dispenser,
    supply: Supply,
    offset: usize,
    config: GeneratorConfig,
    progression: Progression,
    initial_level: NonZeroU32,
    feedback: Option<String>,
    feedback_until: f64,
    random: Random,
    game_over: bool,
    busy: bool,
    error: Option<&'static str>,
    undelivered_rewards: Vec<resolution::Reward>,
}
impl Playfield {
    fn new(level: NonZeroU32, mut random: Random) -> Result<Self, GenerationError> {
        let config = GeneratorConfig::new(level);
        let supply = Supply::new(config, &mut || random.next())?;
        Ok(Self {
            board: Board::default(),
            chute: Dispenser::default(),
            supply,
            offset: 0,
            config,
            progression: Progression::new(level),
            initial_level: level,
            feedback: None,
            feedback_until: 0.0,
            random,
            game_over: false,
            busy: false,
            error: None,
            undelivered_rewards: Vec::new(),
        })
    }
    fn drop_ball(&mut self, motion: &mut Motion) -> Result<(), &'static str> {
        if self.game_over || self.error.is_some() {
            return Ok(());
        }
        let destination = self.chute.lane();
        let mut random = self.random;
        let next = self
            .config
            .generate(&mut || random.next())
            .map_err(|_| "Unable to generate a ball")?;
        let mut supply = self.supply.clone();
        let held = supply
            .take_and_refill(destination, next)
            .map_err(|_| "Unable to refill this lane")?;
        info!(event="drop_started",lane=destination,kind=?held.kind,weight=held.weight,colors=?held.colors,queue=motion.queued.len(),"Drop started and supply refilled");
        motion.drop(Transit {
            ball: held,
            destination,
            path: Path::Drop {
                start_y: 570.0 + board_lift(&self.board),
            },
            elapsed: 0.0,
            duration: 0.5 / 3.0,
        });
        self.busy = true;
        self.supply = supply;
        self.random = random;
        self.error = None;
        Ok(())
    }
    fn reset(&mut self) {
        let mut config = self.config;
        config.level = self.initial_level;
        if let Ok(supply) = Supply::new(config, &mut || self.random.next()) {
            self.config = config;
            self.progression = Progression::new(self.initial_level);
            self.feedback = None;
            self.supply = supply;
            self.board.clear();
            self.offset = 0;
            self.chute = Dispenser::default();
            self.game_over = false;
            self.busy = false;
            self.error = None;
            self.undelivered_rewards.clear();
            info!(
                event = "reset",
                level = self.config.level.get(),
                "New game started"
            );
        } else {
            error!(
                event = "reset_failed",
                "Supply generation failed during reset"
            );
        }
    }
}
// Reproducible render-review setup; opt-in CLI only, not a gameplay level.
fn flight_demo() -> Result<Playfield, GenerationError> {
    let mut field = Playfield::new(
        NonZeroU32::new(5).unwrap_or(NonZeroU32::MIN),
        Random::seeded(),
    )?;
    field.config.bomb_percent = 0;
    field.supply = Supply::new(field.config, &mut || u64::MAX - 1)?;
    for (lane, weights) in [(0, vec![1, 2]), (1, vec![2]), (4, vec![1]), (5, vec![2])] {
        for weight in weights {
            field.board.place(
                lane,
                Ball {
                    kind: BallKind::Normal,
                    weight,
                    colors: BallColors::single(ColorId::RED),
                },
            );
        }
    }
    field.chute.shift(1);
    Ok(field)
}
// Explicit review fixture: one drop compresses five yellow balls and clears a joker bridge.
fn puzzle_demo() -> Result<Playfield, GenerationError> {
    let mut field = Playfield::new(
        NonZeroU32::new(3).unwrap_or(NonZeroU32::MIN),
        Random::seeded(),
    )?;
    field.config.bomb_percent = 0;
    field.supply = Supply::new(field.config, &mut || u64::MAX - 1)?;
    fixture_held(
        &mut field,
        Ball::normal(2, BallColors::single(ColorId::YELLOW)),
    );
    for (lane, ids) in [vec![0], vec![0], vec![0, 1], vec![1], vec![1, 2], vec![2]]
        .into_iter()
        .enumerate()
    {
        let colors = BallColors::new(&ids.into_iter().map(ColorId::new).collect::<Vec<_>>())
            .unwrap_or_else(|| panic!("fixture colors"));
        field.board.place(
            lane,
            Ball {
                kind: BallKind::Normal,
                weight: 1,
                colors,
            },
        );
    }
    for _ in 0..4 {
        field.board.place(
            6,
            Ball {
                kind: BallKind::Normal,
                weight: 1,
                colors: BallColors::single(ColorId::YELLOW),
            },
        );
    }
    for _ in 0..6 {
        field.chute.shift(1);
    }
    Ok(field)
}
// Deterministic edge-crossing scenes for renderer/input review; no automatic throw.
fn wrap_demo(rightward: bool) -> Result<Playfield, GenerationError> {
    let mut field = Playfield::new(
        NonZeroU32::new(3).unwrap_or(NonZeroU32::MIN),
        Random::seeded(),
    )?;
    field.config.bomb_percent = 0;
    field.supply = Supply::new(field.config, &mut || u64::MAX - 1)?;
    fixture_held(
        &mut field,
        Ball::normal(3, BallColors::single(ColorId::BLUE)),
    );
    let (origin, arrival) = if rightward { (6, 7) } else { (1, 0) };
    field.board.place(
        origin,
        Ball {
            kind: BallKind::Normal,
            weight: 1,
            colors: BallColors::single(ColorId::RED),
        },
    );
    for _ in 0..arrival {
        field.chute.shift(1);
    }
    Ok(field)
}
// Opt-in visual review: all twelve finite cue combinations and multi-color examples.
fn palette_demo() -> Result<Playfield, GenerationError> {
    let mut field = Playfield::new(
        NonZeroU32::new(12).unwrap_or(NonZeroU32::MIN),
        Random::seeded(),
    )?;
    let mut samples = Vec::new();
    for index in 0..17u64 {
        let weight = if index == 0 { 6 } else { index % 7 };
        samples.extend([99, weight + 7 * 100, 12 * 100 + index % 12]);
    }
    let mut samples = samples.into_iter();
    field.config.bomb_percent = 0;
    field.supply = Supply::new(field.config, &mut || samples.next().unwrap_or(u64::MAX))?;
    for lane in 0..LANE_COUNT {
        let ids = match lane {
            0 => vec![0],
            1 => vec![1],
            2 => vec![2, 8],
            3 => vec![3, 9],
            4 => vec![4, 10, 11],
            5 => vec![5, 6, 7],
            6 => vec![6],
            _ => vec![11],
        };
        let colors = BallColors::new(&ids.into_iter().map(ColorId::new).collect::<Vec<_>>())
            .unwrap_or_else(|| panic!("fixture colors"));
        field.board.place(
            lane,
            Ball {
                kind: BallKind::Normal,
                weight: if lane == 6 { 125 } else { 12 },
                colors,
            },
        );
    }
    Ok(field)
}
// Development scenes use real core resolution and wait for an actual throw.
fn fixture_held(field: &mut Playfield, ball: Ball) {
    field
        .supply
        .insert_reward_front(0, ball)
        .unwrap_or_else(|error| panic!("invalid fixture reward {error:?}"));
    let _ = field
        .supply
        .take_and_refill(0, Ball::normal(1, BallColors::single(ColorId::RED)))
        .unwrap_or_else(|error| panic!("invalid fixture supply {error:?}"));
}
fn special_demo(mode: &str) -> Result<Playfield, GenerationError> {
    let mut field = Playfield::new(NonZeroU32::MIN, Random::seeded())?;
    let ordinary = |weight| Ball::normal(weight, BallColors::single(ColorId::RED));
    match mode {
        "row" | "column" | "feather" | "inert" => {
            let utility = match mode {
                "row" => Ball::row_laser(),
                "column" => Ball::column_laser(),
                _ => Ball::feather(),
            };
            if mode != "inert" {
                field.board.place(0, Ball::stone());
                field
                    .board
                    .place(0, Ball::normal(2, BallColors::single(ColorId::BLUE)));
                field.board.place(1, ordinary(7));
                field.board.place(2, ordinary(1));
                field.board.place(2, ordinary(1));
                field.board.place(2, Ball::stone());
                field.board.place(3, ordinary(7));
            }
            fixture_held(&mut field, utility);
        }
        "ultimate" => {
            field.board.place(1, ordinary(1));
            // Isolated universal token makes the opt-in visual fixture readable
            // before the real launched clear earns another reward.
            field.board.place(2, Ball::ultimate_joker());
            for lane in 4..7 {
                field.board.place(lane, ordinary(1));
            }
            fixture_held(
                &mut field,
                Ball::normal(3, BallColors::single(ColorId::BLUE)),
            );
        }
        "order" => {
            field = Playfield::new(
                NonZeroU32::new(3).unwrap_or(NonZeroU32::MIN),
                Random::seeded(),
            )?;
            field.config.bomb_percent = 0;
            field.config.joker_percent = 0;
            let mut samples = [99, 0, 3, 99, 1, 1, 99, 0, 2].into_iter();
            field.supply = Supply::new(field.config, &mut || samples.next().unwrap_or(u64::MAX))?;
            field
                .supply
                .insert_reward_after_preview_count(0, Ball::silver_star(), 1)
                .unwrap_or_else(|e| panic!("order fixture {e:?}"));
            field
                .supply
                .insert_reward_after_preview_count(0, Ball::ultimate_joker(), 1)
                .unwrap_or_else(|e| panic!("order fixture {e:?}"));
        }
        "column-reward" => {
            for lane in 0..4 {
                for _ in 0..4 {
                    field.board.place(lane, ordinary(1));
                }
            }
            fixture_held(&mut field, ordinary(1));
            for _ in 0..3 {
                field.chute.shift(1);
            }
        }
        "reward" => {
            for lane in 0..5 {
                field.board.place(lane, ordinary(1));
            }
            fixture_held(&mut field, ordinary(1));
            for _ in 0..5 {
                field.chute.shift(1);
            }
        }
        "bomb" => {
            field.board.place(0, ordinary(1));
            field.board.place(1, Ball::bomb());
            field.board.place(2, Ball::stone());
            field.board.place(3, ordinary(5));
            fixture_held(&mut field, Ball::bomb());
        }
        "silver" => {
            field.board.place(0, Ball::silver_star());
            field.board.place(1, Ball::silver_star());
            field.board.place(6, ordinary(5));
            field.board.place(7, ordinary(5));
            fixture_held(&mut field, Ball::silver_star());
            for _ in 0..2 {
                field.chute.shift(1);
            }
        }
        "gold" | "bonus" => {
            for _ in 0..if mode == "bonus" { 3 } else { 2 } {
                for lane in 0..4 {
                    field.board.place(lane, Ball::gold_star());
                }
                resolution::land(
                    &mut field.board,
                    &mut field.progression,
                    4,
                    Ball::gold_star(),
                )
                .unwrap_or_else(|error| panic!("invalid expansion fixture {error:?}"));
            }
            field.board.place(7, Ball::stone());
            if mode == "gold" {
                for lane in 0..4 {
                    field.board.place(lane, Ball::gold_star());
                }
            }
            fixture_held(&mut field, Ball::gold_star());
            for _ in 0..4 {
                field.chute.shift(1);
            }
        }
        "edge" => {
            field.board.place(1, ordinary(1));
            field.board.place(1, Ball::silver_star());
            fixture_held(
                &mut field,
                Ball::normal(3, BallColors::single(ColorId::BLUE)),
            );
        }
        _ => {
            field.board.place(0, Ball::silver_star());
            field.board.place(1, Ball::gold_star());
            field.board.place(2, Ball::bomb());
            field.board.place(3, Ball::stone());
            fixture_held(&mut field, Ball::silver_star());
        }
    }
    Ok(field)
}
impl Default for Playfield {
    fn default() -> Self {
        match Self::new(NonZeroU32::MIN, Random::seeded()) {
            Ok(field) => field,
            Err(error) => panic!("default level configuration failed: {error:?}"),
        }
    }
}
fn launch_level(args: &[String]) -> Result<NonZeroU32, &'static str> {
    let Some(index) = args.iter().position(|arg| arg == "--level") else {
        return Ok(NonZeroU32::MIN);
    };
    args.get(index + 1)
        .and_then(|value| value.parse::<u32>().ok())
        .and_then(NonZeroU32::new)
        .ok_or("--level requires a positive integer up to 4294967295")
}
#[derive(Component)]
struct Scene;
#[derive(Component)]
struct StackTotal(usize);
#[derive(Resource, Default)]
struct ReversalWarning(Option<usize>);
#[derive(Component)]
struct SceneCamera;
#[derive(Resource, Default)]
struct Smoke {
    enabled: bool,
    frame: u32,
    screenshot: Option<String>,
}
/// Start the shared level-supply view.
pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    let portrait = args.iter().any(|arg| arg == "--portrait");
    let level = match launch_level(&args) {
        Ok(level) => level,
        Err(error) => {
            eprintln!("{error}");
            return;
        }
    };
    let special_mode = [
        ("--reward-order-demo", "order"),
        ("--ultimate-joker-demo", "ultimate"),
        ("--utility-row-demo", "row"),
        ("--utility-column-demo", "column"),
        ("--utility-feather-demo", "feather"),
        ("--utility-inert-demo", "inert"),
        ("--utility-reward-demo", "reward"),
        ("--utility-column-reward-demo", "column-reward"),
        ("--special-bomb-demo", "bomb"),
        ("--special-silver-demo", "silver"),
        ("--special-gold-demo", "gold"),
        ("--special-bonus-demo", "bonus"),
        ("--special-edge-demo", "edge"),
        ("--warning-demo", "edge"),
        ("--special-demo", "merge"),
    ]
    .into_iter()
    .find_map(|(flag, mode)| args.iter().any(|arg| arg == flag).then_some(mode));
    let field = match if let Some(mode) = special_mode {
        special_demo(mode)
    } else if args.iter().any(|arg| arg == "--palette-demo") {
        palette_demo()
    } else if args.iter().any(|arg| arg == "--wrap-left-demo") {
        wrap_demo(false)
    } else if args.iter().any(|arg| arg == "--wrap-right-demo") {
        wrap_demo(true)
    } else if args.iter().any(|arg| arg == "--puzzle-demo") {
        puzzle_demo()
    } else if args.iter().any(|arg| arg == "--flight-demo") {
        flight_demo()
    } else {
        Playfield::new(level, Random::seeded())
    } {
        Ok(field) => field,
        Err(error) => {
            eprintln!("Invalid level configuration: {error:?}");
            return;
        }
    };
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.035, 0.055, 0.09)))
        .insert_resource(Smoke {
            enabled: args.iter().any(|arg| arg == "--smoke-test"),
            frame: 0,
            screenshot: args
                .windows(2)
                .find(|p| p[0] == "--screenshot")
                .map(|p| p[1].clone()),
        })
        .insert_resource(field)
        .init_resource::<Motion>()
        .init_resource::<Clearing>()
        .init_resource::<ReversalWarning>()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Overbalancing - level supply".into(),
                resolution: if portrait {
                    (540, 960).into()
                } else {
                    (1100, 1000).into()
                },
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, |mut commands: Commands| {
            commands.spawn((Camera2d, SceneCamera));
        })
        .add_systems(
            Update,
            (
                pan,
                input,
                animate,
                fit_camera,
                redraw,
                predict_reversal,
                pulse_reversal,
                pulse_clear,
                draw_motion,
                draw_effects,
                smoke_test,
            )
                .chain(),
        )
        .run();
}
fn board_lift(board: &Board) -> f32 {
    board.bonus_rows() as f32 * ROW_HEIGHT
}
fn view_scale_for(width: f32, height: f32, bonus: usize) -> Option<f32> {
    (width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0)
        .then(|| (WIDTH / width).max((HEIGHT + bonus as f32 * ROW_HEIGHT) / height))
}
#[cfg(test)]
fn view_scale(width: f32, height: f32) -> Option<f32> {
    view_scale_for(width, height, 0)
}
fn fit_camera(
    field: Res<Playfield>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut camera: Single<(&mut Projection, &mut Transform), With<SceneCamera>>,
) {
    let (projection, transform) = &mut *camera;
    if let (Projection::Orthographic(projection), Some(scale)) = (
        &mut **projection,
        view_scale_for(window.width(), window.height(), field.board.bonus_rows()),
    ) {
        projection.scale = scale;
        transform.translation.y = board_lift(&field.board) / 2.0;
    }
}
fn label(
    commands: &mut Commands,
    text: impl Into<String>,
    x: f32,
    y: f32,
    size: f32,
    scale: f32,
    color: Color,
) -> Entity {
    commands
        .spawn((
            Scene,
            Text2d::new(text),
            TextFont {
                font_size: bevy::text::FontSize::Px(size.max(14.0 * scale)),
                ..default()
            },
            TextLayout::justify(Justify::Center),
            TextColor(color),
            Transform::from_xyz(x, y, 10.0),
        ))
        .id()
}
fn rect(commands: &mut Commands, x: f32, y: f32, w: f32, h: f32, color: Color, angle: f32) {
    commands.spawn((
        Scene,
        Sprite::from_color(color, Vec2::new(w, h)),
        Transform::from_xyz(x, y, 1.0).with_rotation(Quat::from_rotation_z(angle)),
    ));
}
fn color(id: ColorId) -> Color {
    match id.index() {
        0 => Color::srgb(0.95, 0.34, 0.38),
        1 => Color::srgb(0.22, 0.73, 0.95),
        2 => Color::srgb(0.97, 0.77, 0.3),
        3 => Color::srgb(0.28, 0.85, 0.46),
        4 => Color::srgb(0.78, 0.47, 0.95),
        5 => Color::srgb(0.98, 0.53, 0.22),
        index => Color::hsl((index as f32 * 137.50776) % 360.0, 0.65, 0.65),
    }
}
#[derive(Component)]
struct StackBall(Cell);
#[derive(Component)]
struct GlowCell(Cell);
#[derive(Resource, Default)]
struct Clearing {
    pending: resolution::PendingClear,
    deadline: Option<f64>,
}
fn draw_ball(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    ball: &Ball,
    position: Vec2,
    scale: f32,
    cell: Option<Cell>,
) {
    let visual = ball_visual::build(ball, meshes, materials, scale);
    let radius = ball_visual::radius(scale);
    let glow = cell.map(|_| {
        (
            meshes.add(Annulus::new(radius + 3.0, radius + 8.0)),
            materials.add(Color::srgb(1.0, 0.92, 0.5)),
        )
    });
    let mut group = commands.spawn((
        Scene,
        Transform::from_xyz(position.x, position.y, 2.0),
        Visibility::default(),
    ));
    if let Some(cell) = cell {
        group.insert(StackBall(cell));
    }
    group.with_children(|children| {
        ball_visual::spawn(children, visual);
        if let (Some(cell), Some((mesh, material))) = (cell, glow) {
            children.spawn((
                GlowCell(cell),
                Mesh2d(mesh),
                MeshMaterial2d(material),
                Visibility::Hidden,
                Transform::from_xyz(0.0, 0.0, 7.0),
            ));
        }
    });
}
fn pulse_clear(
    time: Res<Time>,
    clearing: Res<Clearing>,
    mut balls: Query<(&StackBall, &mut Transform)>,
    mut glows: Query<(&GlowCell, &mut Visibility)>,
) {
    let pulse = 0.5 + 0.5 * (time.elapsed_secs() * 16.0).sin();
    for (ball, mut transform) in &mut balls {
        transform.scale = Vec3::splat(if clearing.pending.cells().contains(&ball.0) {
            1.0 - 0.07 * pulse
        } else {
            1.0
        });
    }
    for (glow, mut visibility) in &mut glows {
        *visibility = if clearing.pending.cells().contains(&glow.0) {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}
fn platform_y(board: &Board, lane: usize) -> f32 {
    320.0
        - (board.capacity(lane).unwrap_or(8 + board.bonus_rows()) - board.bonus_rows()) as f32
            * ROW_HEIGHT
}
fn redraw(
    mut commands: Commands,
    field: Res<Playfield>,
    window: Single<&Window, With<PrimaryWindow>>,
    scene: Query<Entity, With<Scene>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut last_size: Local<Vec2>,
) {
    let size = Vec2::new(window.width(), window.height());
    if !field.is_changed() && *last_size == size {
        return;
    }
    *last_size = size;
    let lift = board_lift(&field.board);
    let Some(scale) = view_scale_for(size.x, size.y, field.board.bonus_rows()) else {
        return;
    };
    for entity in &scene {
        commands.entity(entity).despawn();
    }
    label(
        &mut commands,
        format!(
            "Level {} | Score {} | Rows +{}",
            field.progression.level(),
            field.progression.score(),
            field.board.bonus_rows()
        ),
        0.0,
        1000.0 + lift,
        30.0,
        scale,
        INK,
    );
    label(
        &mut commands,
        format!(
            "Next: {} points{}",
            field.progression.remaining(),
            if field.board.expansion_achievement() {
                " | 3 rows earned"
            } else {
                ""
            }
        ),
        0.0,
        940.0 + lift,
        20.0,
        scale,
        INK,
    );
    for lane in 0..LANE_COUNT {
        let x = LEFT + (lane as f32 + 0.5) * LANE;
        rect(
            &mut commands,
            x,
            805.0 + lift,
            88.0,
            200.0,
            Color::srgb(0.08, 0.14, 0.2),
            0.0,
        );
        if let Some(previews) = field.supply.previews(lane) {
            for (index, ball) in previews.iter().enumerate() {
                draw_ball(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    ball,
                    Vec2::new(x, 760.0 + lift + index as f32 * ROW_HEIGHT),
                    scale,
                    None,
                );
            }
        }
        label(
            &mut commands,
            format!("{}", lane + 1),
            x,
            680.0 + lift,
            20.0,
            scale,
            INK,
        );
        rect(
            &mut commands,
            x,
            -105.0 + lift / 2.0,
            2.0,
            890.0 + lift,
            Color::srgb(0.1, 0.18, 0.25),
            0.0,
        );
        let total = label(
            &mut commands,
            field.board.total(lane).to_string(),
            x,
            -610.0,
            20.0,
            scale,
            INK,
        );
        commands.entity(total).insert(StackTotal(lane));
        let base = platform_y(&field.board, lane);
        rect(&mut commands, x, base, 80.0, 12.0, INK, 0.0);
        for (index, ball) in field
            .board
            .stack(lane)
            .unwrap_or(&[])
            .iter()
            .skip(field.offset)
            .take(9)
            .enumerate()
        {
            draw_ball(
                &mut commands,
                &mut meshes,
                &mut materials,
                ball,
                Vec2::new(x, base + ROW_HEIGHT / 2.0 + index as f32 * ROW_HEIGHT),
                scale,
                Some(Cell {
                    lane,
                    index: index + field.offset,
                }),
            );
        }
    }
    let x = LEFT + (field.chute.lane() as f32 + 0.5) * LANE;
    rect(
        &mut commands,
        x,
        570.0 + lift,
        92.0,
        100.0,
        Color::srgb(0.4, 0.3, 0.12),
        0.0,
    );
    draw_ball(
        &mut commands,
        &mut meshes,
        &mut materials,
        field.supply.held(),
        Vec2::new(x, 570.0 + lift),
        scale,
        None,
    );
    rect(
        &mut commands,
        x,
        510.0 + lift,
        92.0,
        18.0,
        Color::srgb(0.95, 0.75, 0.3),
        0.0,
    );
    rect(
        &mut commands,
        x,
        480.0 + lift,
        24.0,
        40.0,
        Color::srgb(0.95, 0.75, 0.3),
        0.0,
    );
    for seesaw in 0..SEESAW_COUNT {
        let left_x = LEFT + (seesaw * 2) as f32 * LANE + LANE / 2.0;
        let a = Vec2::new(left_x, platform_y(&field.board, seesaw * 2));
        let b = Vec2::new(left_x + LANE, platform_y(&field.board, seesaw * 2 + 1));
        let middle = (a + b) / 2.0;
        rect(
            &mut commands,
            middle.x,
            middle.y,
            (b - a).length(),
            9.0,
            INK,
            (b.y - a.y).atan2(b.x - a.x),
        );
        commands.spawn((
            Scene,
            Mesh2d(meshes.add(Triangle2d::new(
                Vec2::new(0.0, 20.0),
                Vec2::new(-20.0, -20.0),
                Vec2::new(20.0, -20.0),
            ))),
            MeshMaterial2d(materials.add(Color::srgb(0.3, 0.45, 0.6))),
            Transform::from_xyz(middle.x, middle.y - 25.0, 0.0),
        ));
    }
    label(
        &mut commands,
        "Left/Right or tap: chute | Down: drop\nWheel / drag: inspect | Backspace: reset",
        0.0,
        -960.0,
        18.0,
        scale,
        INK,
    );
    if field.game_over {
        commands.spawn((
            Scene,
            Sprite::from_color(Color::srgb(0.6, 0.05, 0.08), Vec2::new(780.0, 250.0)),
            Transform::from_xyz(0.0, -30.0, 90.0),
        ));
        commands.spawn((
            Scene,
            Text2d::new("GAME OVER\nStack capacity exceeded\nBackspace to play again"),
            TextFont {
                font_size: bevy::text::FontSize::Px((18.0 * scale).max(28.0)),
                ..default()
            },
            TextLayout::justify(Justify::Center),
            TextColor(Color::WHITE),
            Transform::from_xyz(0.0, -30.0, 100.0),
        ));
    } else if let Some(error) = field.error {
        label(
            &mut commands,
            error,
            0.0,
            0.0,
            24.0,
            scale,
            Color::srgb(1.0, 0.4, 0.4),
        );
    }
}
// Prediction runs only when board, chute or held supply changes. Animation only
// touches these eight text colors; it never changes totals or the field resource.
fn predict_reversal(
    field: Res<Playfield>,
    clearing: Res<Clearing>,
    mut warning: ResMut<ReversalWarning>,
) {
    if !field.is_changed() && !clearing.is_changed() {
        return;
    }
    warning.0 = (!field.game_over
        && field.error.is_none()
        && resolution::would_reverse(
            &field.board,
            &clearing.pending,
            field.chute.lane(),
            field.supply.held(),
        ))
    .then_some(field.chute.lane());
}
fn pulse_reversal(
    time: Res<Time>,
    warning: Res<ReversalWarning>,
    mut totals: Query<(&StackTotal, &mut TextColor)>,
) {
    let blend = (1.0 - (time.elapsed_secs() * std::f32::consts::TAU * 2.0).cos()) * 0.5;
    let amber = Color::srgb(1.0, 0.53, 0.19);
    for (total, mut color) in &mut totals {
        color.0 = if warning.0 == Some(total.0) {
            INK.mix(&amber, blend)
        } else {
            INK
        };
    }
}
fn pan(
    mut wheels: MessageReader<bevy::input::mouse::MouseWheel>,
    touches: Res<Touches>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut field: ResMut<Playfield>,
    mut carry: Local<f32>,
) {
    let Some(scale) = view_scale_for(window.width(), window.height(), field.board.bonus_rows())
    else {
        return;
    };
    let mut rows = 0i32;
    for wheel in wheels.read() {
        if wheel.y != 0.0 {
            rows += if wheel.y > 0.0 { 1 } else { -1 };
        }
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        rows += 1;
    }

    if let Some(touch) = touches
        .iter()
        .find(|touch| touch.distance().length() > 10.0)
    {
        *carry += touch.delta().y * scale;
        while carry.abs() >= ROW_HEIGHT {
            let direction = if *carry > 0.0 { 1 } else { -1 };
            rows += direction;
            *carry -= direction as f32 * ROW_HEIGHT;
        }
    } else {
        *carry = 0.0;
    }
    let max = (0..LANE_COUNT)
        .filter_map(|lane| field.board.stack(lane).map(<[Ball]>::len))
        .max()
        .unwrap_or(0)
        .saturating_sub(9);
    if rows != 0 {
        field.offset = field.offset.saturating_add_signed(rows as isize).min(max);
    }
}
fn input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut field: ResMut<Playfield>,
    mut ignore_mouse: Local<u8>,
    phases: (ResMut<Motion>, ResMut<Clearing>),
) {
    let (mut motion, mut clearing) = phases;
    let touch_activity =
        touches.iter().next().is_some() || touches.iter_just_released().next().is_some();
    *ignore_mouse = if touch_activity {
        2
    } else {
        ignore_mouse.saturating_sub(1)
    };
    let drop_requested = keys.just_pressed(KeyCode::ArrowDown)
        || keys.just_pressed(KeyCode::Space)
        || keys.just_pressed(KeyCode::Enter);
    let reset_requested = keys.just_pressed(KeyCode::Backspace);
    if keys.just_pressed(KeyCode::ArrowLeft) || keys.just_pressed(KeyCode::KeyA) {
        field.chute.shift(-1);
    }
    if keys.just_pressed(KeyCode::ArrowRight) || keys.just_pressed(KeyCode::KeyD) {
        field.chute.shift(1);
    }

    let pointer = touches
        .iter_just_released()
        .find(|touch| touch.distance().length() <= 10.0)
        .map(|t| t.position())
        .or_else(|| {
            (*ignore_mouse == 0 && mouse.just_pressed(MouseButton::Left))
                .then(|| window.cursor_position())
                .flatten()
        });
    if let (Some(position), Some(scale)) = (
        pointer,
        view_scale_for(window.width(), window.height(), field.board.bonus_rows()),
    ) {
        let point = Vec2::new(
            (position.x - window.width() / 2.0) * scale,
            (window.height() / 2.0 - position.y) * scale + board_lift(&field.board) / 2.0,
        );
        if (-400.0..=400.0).contains(&point.x)
            && (-640.0..=640.0 + board_lift(&field.board)).contains(&point.y)
        {
            field
                .chute
                .select_fraction((point.x - LEFT) / (LANE * LANE_COUNT as f32));
        }
    }
    if reset_requested {
        field.reset();
        motion.clear();
        *clearing = Clearing::default();
    } else if drop_requested && let Err(error) = field.drop_ball(&mut motion) {
        error!(event = "drop_failed", error, "Drop rejected");
        field.error = Some(error);
    }
}
#[derive(Clone)]
enum Path {
    Drop {
        start_y: f32,
    },
    Launch {
        origin: usize,
        distance: u64,
        rightward: bool,
        start_y: f32,
        edge_from: Option<BallKind>,
    },
}
#[derive(Clone)]
struct Transit {
    ball: Ball,
    destination: usize,
    path: Path,
    elapsed: f32,
    duration: f32,
}
#[derive(Resource, Default)]
struct Motion {
    active: Option<Transit>,
    queued: VecDeque<Transit>,
    incoming: Vec<(u64, Transit)>,
    flashes: Vec<Flash>,
    next_id: u64,
    revision: u64,
}
impl Motion {
    fn drop(&mut self, transit: Transit) {
        if self.active.is_none() {
            self.start(transit);
        } else {
            self.next_id = self.next_id.wrapping_add(1).max(1);
            let id = self.next_id;
            self.incoming.push((id, transit));
            self.revision = self.revision.wrapping_add(1);
        }
    }
    fn transits(&self) -> impl Iterator<Item = (u64, &Transit)> {
        self.active
            .iter()
            .map(|t| (0, t))
            .chain(self.incoming.iter().map(|(id, t)| (*id, t)))
    }
    fn start(&mut self, transit: Transit) {
        self.active = Some(transit);
        self.revision = self.revision.wrapping_add(1);
    }
    fn clear(&mut self) {
        self.queued.clear();
        self.incoming.clear();
        self.flashes.clear();
        self.active = None;
        self.revision = self.revision.wrapping_add(1);
    }
}
#[derive(Clone)]
struct Flash {
    id: u64,
    position: Vec2,
    size: Vec2,
    color: Color,
    started: f64,
}
#[derive(Component)]
struct EffectVisual(u64);
fn flash(motion: &mut Motion, position: Vec2, size: Vec2, color: Color, started: f64) {
    motion.next_id = motion.next_id.wrapping_add(1).max(1);
    let id = motion.next_id;
    motion.flashes.push(Flash {
        id,
        position,
        size,
        color,
        started,
    });
}
fn draw_effects(
    mut commands: Commands,
    state: (Res<Playfield>, Res<Motion>, Res<Time>),
    mut visuals: Query<(Entity, &EffectVisual, &mut Sprite, &mut Transform)>,
) {
    let (field, motion, time) = state;
    let mut seen = Vec::new();
    for (entity, visual, mut sprite, mut transform) in &mut visuals {
        if let Some(effect) = motion.flashes.iter().find(|effect| effect.id == visual.0) {
            let t = ((time.elapsed_secs_f64() - effect.started) / 0.4).clamp(0.0, 1.0) as f32;
            let color = effect.color.to_srgba();
            sprite.color =
                Color::srgba(color.red, color.green, color.blue, color.alpha * (1.0 - t));
            transform.scale = Vec3::splat(1.0 + 0.12 * t);
            transform.translation.y = effect.position.y - field.offset as f32 * ROW_HEIGHT;
            seen.push(effect.id);
        } else {
            commands.entity(entity).despawn();
        }
    }
    for effect in &motion.flashes {
        if !seen.contains(&effect.id) {
            let color = effect.color.to_srgba();
            let t = ((time.elapsed_secs_f64() - effect.started) / 0.4).clamp(0.0, 1.0) as f32;
            commands.spawn((
                EffectVisual(effect.id),
                Sprite::from_color(
                    Color::srgba(color.red, color.green, color.blue, color.alpha * (1.0 - t)),
                    effect.size,
                ),
                Transform::from_xyz(
                    effect.position.x,
                    effect.position.y - field.offset as f32 * ROW_HEIGHT,
                    50.0,
                ),
            ));
        }
    }
}
#[derive(Component)]
struct TransitVisual {
    target: bool,
    id: u64,
}
fn lane_x(lane: usize) -> f32 {
    LEFT + (lane as f32 + 0.5) * LANE
}
fn landing_y(board: &Board, lane: usize) -> f32 {
    platform_y(board, lane)
        + ROW_HEIGHT / 2.0
        + board.stack(lane).map_or(0, <[Ball]>::len) as f32 * ROW_HEIGHT
}
fn visible_steps(distance: u64) -> f32 {
    let remainder = (distance % LANE_COUNT as u64) as u32;
    if distance >= LANE_COUNT as u64 {
        LANE_COUNT as f32 + remainder as f32
    } else {
        remainder as f32
    }
}
fn transit_position(transit: &Transit, board: &Board, offset: usize) -> Vec2 {
    let t = (transit.elapsed / transit.duration).clamp(0.0, 1.0);
    let target = landing_y(board, transit.destination) - offset as f32 * ROW_HEIGHT;
    match transit.path {
        Path::Drop { start_y } => Vec2::new(
            lane_x(transit.destination),
            start_y + (target - start_y) * t * t,
        ),
        Path::Launch {
            origin,
            distance,
            rightward,
            start_y,
            ..
        } => {
            let steps = visible_steps(distance);
            let direction = if rightward { 1.0 } else { -1.0 };
            let x = LEFT
                + (lane_x(origin) - LEFT + direction * steps * LANE * t)
                    .rem_euclid(LANE * LANE_COUNT as f32);
            let from = start_y - offset as f32 * ROW_HEIGHT;
            Vec2::new(
                x,
                from + (target - from) * t + 4.0 * (180.0 + steps * 6.0) * t * (1.0 - t),
            )
        }
    }
}
fn apply_outcome(
    field: &mut Playfield,
    motion: &mut Motion,
    clearing: &mut Clearing,
    outcome: resolution::Outcome,
    at: f64,
) {
    field.game_over = outcome.game_over;
    for effect in &outcome.utility_effects {
        let y =
            320.0 - 9.0 * ROW_HEIGHT + ROW_HEIGHT / 2.0 + effect.position.row as f32 * ROW_HEIGHT;
        let (position, size, color) = match effect.kind {
            BallKind::RowLaser => (
                Vec2::new(0.0, y),
                Vec2::new(800.0, ROW_HEIGHT * 0.65),
                Color::srgba(1.0, 1.0, 1.0, 0.65),
            ),
            BallKind::ColumnLaser => (
                Vec2::new(
                    lane_x(effect.position.lane),
                    -115.0 + board_lift(&field.board) / 2.0,
                ),
                Vec2::new(ROW_HEIGHT * 0.65, 890.0 + board_lift(&field.board)),
                Color::srgba(1.0, 1.0, 1.0, 0.65),
            ),
            BallKind::Feather => (
                Vec2::new(
                    lane_x(effect.position.lane),
                    -115.0 + board_lift(&field.board) / 2.0,
                ),
                Vec2::new(ROW_HEIGHT * 0.85, 890.0 + board_lift(&field.board)),
                Color::srgba(0.6, 1.0, 0.85, 0.30),
            ),
            _ => continue,
        };
        info!(event="utility_activated",kind=?effect.kind,lane=effect.position.lane,row=effect.position.row,"Utility effect resolved");
        flash(motion, position, size, color, at);
    }
    for center in &outcome.blast_centers {
        let y = 320.0 - 9.0 * ROW_HEIGHT + ROW_HEIGHT / 2.0 + center.row as f32 * ROW_HEIGHT;
        flash(
            motion,
            Vec2::new(lane_x(center.lane), y),
            Vec2::splat(ROW_HEIGHT * 3.0),
            Color::srgba(1.0, 0.26, 0.08, 0.24),
            at,
        );
    }
    if outcome.silver_clears > 0 {
        flash(
            motion,
            Vec2::new(0.0, -115.0 + board_lift(&field.board) / 2.0),
            Vec2::new(800.0, 890.0 + board_lift(&field.board)),
            Color::srgba(0.8, 0.88, 1.0, 0.16),
            at,
        );
    }
    if outcome.expansions > 0 {
        flash(
            motion,
            Vec2::new(0.0, 320.0 + board_lift(&field.board) - ROW_HEIGHT / 2.0),
            Vec2::new(800.0, ROW_HEIGHT),
            Color::srgba(1.0, 0.75, 0.15, 0.24),
            at,
        );
    }

    for reward in &outcome.rewards {
        if let Err(reason) = field.supply.insert_reward_after_preview_count(
            reward.lane,
            reward.ball.clone(),
            reward.count,
        ) {
            error!(event="reward_delivery_failed",error=?reason,"Reward delivery failed");
            field.undelivered_rewards.push(reward.clone());
            field.error = Some("Reward delivery paused - Backspace to reset");
        } else {
            info!(event="reward_queued",lane=reward.lane,count=reward.count,kind=?reward.ball.kind,"Earned reward queued after reserved next preview");
        }
    }
    info!(
        event = "special_effects",
        blasted = outcome.blasted_count,
        silver_clears = outcome.silver_clears,
        expansions = outcome.expansions,
        stars_earned = outcome.stars_earned,
        "Special effects resolved"
    );
    let previous = field.config.level;
    field.config.level = field.progression.level();
    info!(
        event = "resolution",
        points = outcome.points,
        cleared = outcome.cleared_count,
        compressed = outcome.compressed_count,
        score = field.progression.score(),
        level = field.config.level.get(),
        remaining = field.progression.remaining(),
        flights = outcome.flights.len(),
        "Landing phase resolved"
    );
    for seesaw in 0..SEESAW_COUNT {
        info!(event="beam_state",seesaw,left=field.board.total(seesaw*2),right=field.board.total(seesaw*2+1),balance=?field.board.balance(seesaw),"Seesaw evaluated");
    }
    if outcome.points > 0 || outcome.compressed_count > 0 {
        field.feedback = Some(format!(
            "5->1 x{} | Cleared {} | +{}{}",
            outcome.compressed_count,
            outcome.cleared_count,
            outcome.points,
            if previous != field.config.level {
                format!(" | Level {}", field.config.level)
            } else {
                String::new()
            }
        ));
        field.feedback_until = at + 3.0;
    }
    for flight in outcome.flights {
        info!(
            event = "flight_queued",
            origin = flight.origin,
            destination = flight.destination,
            distance = flight.distance,
            weight = flight.ball.weight,
            "Ball launched"
        );
        let start_y = 320.0
            - (flight.origin_capacity - flight.origin_bonus_rows) as f32 * ROW_HEIGHT
            + ROW_HEIGHT / 2.0
            + flight.origin_index as f32 * ROW_HEIGHT;
        let duration = 0.9 + visible_steps(flight.distance) * 0.04;
        motion.queued.push_back(Transit {
            ball: flight.ball,
            destination: flight.destination,
            path: Path::Launch {
                origin: flight.origin,
                distance: flight.distance,
                rightward: flight.origin.is_multiple_of(2),
                start_y,
                edge_from: flight.edge_from,
            },
            elapsed: 0.0,
            duration,
        });
    }
    if clearing.pending.is_empty() {
        clearing.deadline = None;
    } else if clearing.deadline.is_none() {
        let deadline = at + CLEAR_WINDOW;
        clearing.deadline = Some(deadline);
        info!(
            event = "clear_started",
            pending = clearing.pending.cells().len(),
            deadline,
            "Clear window opened"
        );
    }
    if field.game_over {
        warn!(
            event = "game_over",
            score = field.progression.score(),
            "Stack capacity exceeded"
        );
    }
}
fn commit_pending(field: &mut Playfield, motion: &mut Motion, clearing: &mut Clearing, at: f64) {
    let outcome = resolution::commit_clear(
        &mut field.board,
        &mut field.progression,
        &mut clearing.pending,
    );
    match outcome {
        Ok(outcome) => {
            info!(
                event = "clear_committed",
                points = outcome.points,
                cleared = outcome.cleared_count,
                score = field.progression.score(),
                level = field.progression.level().get(),
                "Clear committed"
            );
            clearing.deadline = None;
            apply_outcome(field, motion, clearing, outcome, at);
        }
        Err(reason) => {
            error!(event="clear_failed",error=?reason,"Clear paused without mutation");
            field.error = Some("Clear paused - Backspace to reset");
        }
    }
}
fn animate(
    time: Res<Time>,
    mut field: ResMut<Playfield>,
    mut motion: ResMut<Motion>,
    mut clearing: ResMut<Clearing>,
) {
    let now = time.elapsed_secs_f64();
    motion.flashes.retain(|effect| now - effect.started < 0.4);
    if field.feedback.is_some() && now >= field.feedback_until {
        field.feedback = None;
    }
    if field.game_over || field.error.is_some() {
        return;
    }
    let mut completed = Vec::new();
    if let Some(active) = motion.active.as_mut() {
        active.elapsed += time.delta_secs();
        if active.elapsed >= active.duration {
            let at = now - f64::from(active.elapsed - active.duration);
            if let Some(active) = motion.active.take() {
                completed.push((at, active));
            }
        }
    }
    for (_, incoming) in &mut motion.incoming {
        incoming.elapsed += time.delta_secs();
    }
    let mut index = 0;
    while index < motion.incoming.len() {
        if motion.incoming[index].1.elapsed >= motion.incoming[index].1.duration {
            let (_, incoming) = motion.incoming.remove(index);
            completed.push((
                now - f64::from(incoming.elapsed - incoming.duration),
                incoming,
            ));
        } else {
            index += 1;
        }
    }
    completed.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut events = VecDeque::from(completed);
    while let Some((at, landed)) = events.pop_front() {
        // Arrivals exactly at the deadline still join the pending group.
        while field.error.is_none()
            && !field.game_over
            && let Some(deadline) = clearing.deadline
            && deadline < at
        {
            commit_pending(&mut field, &mut motion, &mut clearing, deadline);
        }
        if field.error.is_some() || field.game_over {
            motion.drop(landed);
            for (_, remaining) in events {
                motion.drop(remaining);
            }
            break;
        }
        info!(
            event = "landing",
            lane = landed.destination,
            weight = landed.ball.weight,
            "Ball landed"
        );
        let prepare = if matches!(landed.path, Path::Launch { .. }) {
            resolution::prepare_flight_landing
        } else {
            resolution::prepare_landing
        };
        match prepare(
            &mut field.board,
            &mut clearing.pending,
            landed.destination,
            landed.ball.clone(),
        ) {
            Ok(outcome) => {
                apply_outcome(&mut field, &mut motion, &mut clearing, outcome, at);
                motion.revision = motion.revision.wrapping_add(1);
            }
            Err(reason) => {
                error!(event="resolution_failed",error=?reason,"In-transit ball preserved");
                field.error = Some("Resolution paused - Backspace to reset");
                motion.drop(landed);
                for (_, remaining) in events {
                    motion.drop(remaining);
                }
                break;
            }
        }
    }
    while field.error.is_none()
        && !field.game_over
        && let Some(deadline) = clearing.deadline
        && deadline <= now
    {
        commit_pending(&mut field, &mut motion, &mut clearing, deadline);
    }
    if motion.active.is_none()
        && let Some(next) = motion.queued.pop_front()
    {
        info!(
            event = "queue_advanced",
            remaining = motion.queued.len(),
            destination = next.destination,
            "Next flight started"
        );
        motion.start(next);
    }
    let busy = motion.active.is_some()
        || !motion.incoming.is_empty()
        || !motion.queued.is_empty()
        || !clearing.pending.is_empty();
    if field.busy != busy {
        field.busy = busy;
    }
}
fn visual_ball(transit: &Transit) -> Ball {
    if let Path::Launch {
        origin,
        distance,
        rightward,
        edge_from: Some(kind),
        ..
    } = transit.path
    {
        let to_edge = if rightward {
            (LANE_COUNT - origin) as f32 - 0.5
        } else {
            origin as f32 + 0.5
        };
        let edge_at = to_edge / visible_steps(distance).max(1.0);
        if transit.elapsed / transit.duration < edge_at {
            return match kind {
                BallKind::SilverStar => Ball::silver_star(),
                BallKind::GoldStar => Ball::gold_star(),
                _ => transit.ball.clone(),
            };
        }
    }
    transit.ball.clone()
}
#[derive(Default)]
struct MotionCache {
    revision: Option<u64>,
    scale: f32,
    kinds: Vec<(u64, BallKind)>,
}
fn draw_motion(
    mut commands: Commands,
    state: (Res<Playfield>, Res<Motion>),
    window: Single<&Window, With<PrimaryWindow>>,
    assets: (ResMut<Assets<Mesh>>, ResMut<Assets<ColorMaterial>>),
    mut visuals: Query<(Entity, &mut Transform, &TransitVisual)>,
    mut cache: Local<MotionCache>,
) {
    let (field, motion) = state;
    let (mut meshes, mut materials) = assets;
    let Some(scale) = view_scale_for(window.width(), window.height(), field.board.bonus_rows())
    else {
        return;
    };
    let kinds = motion
        .transits()
        .map(|(id, transit)| (id, visual_ball(transit).kind))
        .collect::<Vec<_>>();
    let rebuild =
        cache.revision != Some(motion.revision) || cache.scale != scale || cache.kinds != kinds;
    if rebuild {
        for (entity, _, _) in &mut visuals {
            commands.entity(entity).despawn();
        }
        for (id, kind) in &kinds {
            if *kind == BallKind::Bomb
                && cache.kinds.iter().any(|(previous_id, previous)| {
                    previous_id == id
                        && matches!(previous, BallKind::SilverStar | BallKind::GoldStar)
                })
            {
                info!(
                    event = "edge_conversion",
                    transit = id,
                    "Star visibly crossed the edge and became bomb10"
                );
            }
        }
        cache.revision = Some(motion.revision);
        cache.scale = scale;
        cache.kinds = kinds;
        for (id, active) in motion.transits() {
            let position = transit_position(active, &field.board, field.offset);
            let radius = ball_visual::radius(scale);
            let visual =
                ball_visual::build(&visual_ball(active), &mut meshes, &mut materials, scale);
            commands
                .spawn((
                    TransitVisual { target: false, id },
                    Transform::from_xyz(position.x, position.y, 40.0),
                    Visibility::default(),
                ))
                .with_children(|children| ball_visual::spawn(children, visual));
            commands.spawn((
                TransitVisual { target: true, id },
                Mesh2d(meshes.add(Annulus::new(radius + 4.0, radius + 8.0))),
                MeshMaterial2d(materials.add(Color::srgb(0.95, 0.75, 0.3))),
                Transform::from_xyz(
                    lane_x(active.destination),
                    landing_y(&field.board, active.destination) - field.offset as f32 * ROW_HEIGHT,
                    35.0,
                ),
            ));
        }
    } else {
        for (_, mut transform, visual) in &mut visuals {
            if let Some((_, active)) = motion.transits().find(|(id, _)| *id == visual.id) {
                let position = transit_position(active, &field.board, field.offset);
                let at = if visual.target {
                    Vec2::new(
                        lane_x(active.destination),
                        landing_y(&field.board, active.destination)
                            - field.offset as f32 * ROW_HEIGHT,
                    )
                } else {
                    position
                };
                transform.translation.x = at.x;
                transform.translation.y = at.y;
            }
        }
    }
}
fn smoke_test(mut commands: Commands, mut smoke: ResMut<Smoke>, mut exit: MessageWriter<AppExit>) {
    if !smoke.enabled {
        return;
    }
    smoke.frame += 1;
    if smoke.frame == 60
        && let Some(path) = smoke.screenshot.clone()
    {
        commands
            .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
            .observe(bevy::render::view::screenshot::save_to_disk(path));
    }
    if smoke.frame >= 120 {
        exit.write(AppExit::Success);
    }
}
/// Android native-activity entry point.
#[cfg(any(target_os = "android", target_os = "ios"))]
#[bevy::prelude::bevy_main]
fn main() {
    run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::touch::{TouchInput, TouchPhase};
    fn test_field(level: NonZeroU32, seed: u64) -> Playfield {
        let mut field =
            Playfield::new(level, Random(seed)).unwrap_or_else(|error| panic!("fixture {error:?}"));
        field.config.bomb_percent = 0;
        field.random = Random(seed);
        field.supply = Supply::new(field.config, &mut || field.random.next())
            .unwrap_or_else(|error| panic!("fixture {error:?}"));
        field
    }
    fn input_app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin))
            .insert_resource(test_field(NonZeroU32::new(4).unwrap_or(NonZeroU32::MIN), 7))
            .init_resource::<Motion>()
            .init_resource::<Clearing>()
            .init_resource::<ReversalWarning>()
            .add_systems(
                Update,
                (pan, input, animate, predict_reversal, pulse_reversal).chain(),
            );
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution: (390, 844).into(),
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        (app, window)
    }
    fn advance(app: &mut App) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(2));
        app.world_mut().run_schedule(Update);
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::ZERO);
    }
    fn key(app: &mut App, key: KeyCode) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(key);
        app.world_mut().run_schedule(Update);
    }
    fn touch(app: &mut App, window: Entity, phase: TouchPhase, x: f32, y: f32, id: u64) {
        let scale = view_scale(390.0, 844.0).unwrap_or(1.0);
        app.world_mut().write_message(TouchInput {
            phase,
            position: Vec2::new(195.0 + x / scale, 422.0 - y / scale),
            window,
            force: None,
            id,
        });
        app.update();
    }
    #[test]
    fn hovered_reversal_warns_without_changing_totals_or_consuming_held() {
        let (mut app, _) = input_app();
        {
            let mut field = app.world_mut().resource_mut::<Playfield>();
            field.board.clear();
            assert!(
                field
                    .board
                    .place(1, Ball::normal(1, BallColors::single(ColorId::RED)))
            );
            fixture_held(
                &mut field,
                Ball::normal(3, BallColors::single(ColorId::BLUE)),
            );
        }
        let total = app
            .world_mut()
            .spawn((StackTotal(0), Text2d::new("0"), TextColor(INK)))
            .id();
        let other = app.world_mut().spawn((StackTotal(1), TextColor(INK))).id();
        key(&mut app, KeyCode::ArrowLeft);
        assert_eq!(app.world().resource::<ReversalWarning>().0, Some(0));
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(125));
        app.world_mut().run_schedule(Update);
        assert_ne!(
            app.world()
                .get::<TextColor>(total)
                .unwrap_or_else(|| panic!("total text"))
                .0,
            INK
        );
        assert_eq!(
            app.world()
                .get::<TextColor>(other)
                .unwrap_or_else(|| panic!("other total text"))
                .0,
            INK
        );
        assert_eq!(
            app.world()
                .get::<Text2d>(total)
                .unwrap_or_else(|| panic!("numeric total"))
                .0,
            "0"
        );
        key(&mut app, KeyCode::ArrowRight);
        assert_eq!(app.world().resource::<ReversalWarning>().0, None);
        assert_eq!(
            app.world()
                .get::<TextColor>(total)
                .unwrap_or_else(|| panic!("total text"))
                .0,
            INK
        );
        let field = app.world().resource::<Playfield>();
        assert_eq!(field.board.total(0), 0);
        assert_eq!(field.board.total(1), 1);
        assert_eq!(field.supply.held().weight, 3);
        assert!(app.world().resource::<Motion>().active.is_none());
        key(&mut app, KeyCode::ArrowLeft);
        app.world_mut().resource_mut::<Playfield>().game_over = true;
        app.world_mut().run_schedule(Update);
        assert_eq!(app.world().resource::<ReversalWarning>().0, None);
        key(&mut app, KeyCode::Backspace);
        assert_eq!(app.world().resource::<ReversalWarning>().0, None);
    }
    #[test]
    fn reversal_cache_uses_live_marks_and_refreshes_when_only_pending_changes() {
        let mut field = test_field(NonZeroU32::MIN, 7);
        let normal = |weight, id| Ball::normal(weight, BallColors::single(ColorId::new(id)));
        for _ in 0..4 {
            assert!(field.board.place(0, normal(u32::MAX, 0)));
        }
        assert!(field.board.place(1, normal(u32::MAX, 0)));
        for id in [1, 2, 0] {
            assert!(field.board.place(2, normal(1, id)));
        }
        let mut pending = resolution::PendingClear::default();
        resolution::prepare_landing(&mut field.board, &mut pending, 0, normal(u32::MAX, 0))
            .unwrap_or_else(|e| panic!("pending fixture {e:?}"));
        for (index, weight) in [u32::MAX, u32::MAX, u32::MAX, u32::MAX - 1]
            .into_iter()
            .enumerate()
        {
            resolution::prepare_landing(
                &mut field.board,
                &mut pending,
                1,
                normal(weight, 10 + index as u32),
            )
            .unwrap_or_else(|e| panic!("pending fixture {e:?}"));
        }
        fixture_held(&mut field, normal(2, 20));
        field.chute.shift(1);
        let mut app = App::new();
        app.insert_resource(field)
            .insert_resource(Clearing {
                pending,
                deadline: None,
            })
            .init_resource::<ReversalWarning>()
            .add_systems(Update, predict_reversal);
        app.update();
        assert_eq!(app.world().resource::<ReversalWarning>().0, Some(1));
        app.world_mut().resource_mut::<Clearing>().pending = resolution::PendingClear::default();
        app.update();
        assert_eq!(app.world().resource::<ReversalWarning>().0, None);
    }
    #[test]
    fn real_utility_drops_animate_core_effects_and_keep_laser_points_out_of_levels() {
        for (mode, points) in [("row", 500), ("column", 700), ("feather", 0)] {
            let (mut app, _) = input_app();
            *app.world_mut().resource_mut::<Playfield>() =
                special_demo(mode).unwrap_or_else(|e| panic!("utility fixture {e:?}"));
            key(&mut app, KeyCode::ArrowDown);
            assert_eq!(
                app.world()
                    .resource::<Motion>()
                    .active
                    .as_ref()
                    .map(|t| t.ball.weight),
                Some(0)
            );
            step(&mut app, 0.17);
            assert!(!app.world().resource::<Motion>().flashes.is_empty());
            assert_eq!(app.world().resource::<Playfield>().progression.score(), 0);
            if mode == "feather" {
                let field = app.world().resource::<Playfield>();
                let stack = field.board.stack(0).unwrap_or(&[]);
                assert_eq!(stack.len(), 2);
                assert_eq!(stack[0].kind, BallKind::Stone);
                assert_eq!(stack[1].kind, BallKind::Normal);
                assert!(stack.iter().all(|ball| ball.weight == 0));
            }
            step(&mut app, 0.46);
            let field = app.world().resource::<Playfield>();
            assert_eq!(field.progression.score(), points);
            assert_eq!(field.progression.remaining(), 1000);
            key(&mut app, KeyCode::Backspace);
            assert!(app.world().resource::<Motion>().flashes.is_empty());
            assert!(app.world().resource::<Clearing>().pending.is_empty());
        }
    }
    #[test]
    fn utility_empty_landing_is_inert_and_rewards_preserve_reserved_preview() {
        let (mut app, _) = input_app();
        *app.world_mut().resource_mut::<Playfield>() =
            special_demo("inert").unwrap_or_else(|e| panic!("fixture {e:?}"));
        key(&mut app, KeyCode::ArrowDown);
        step(&mut app, 0.17);
        assert_eq!(
            app.world()
                .resource::<Playfield>()
                .board
                .stack(0)
                .unwrap_or(&[])
                .first()
                .map(|ball| ball.kind),
            Some(BallKind::Feather)
        );
        assert!(app.world().resource::<Motion>().flashes.is_empty());
        *app.world_mut().resource_mut::<Playfield>() =
            special_demo("reward").unwrap_or_else(|e| panic!("fixture {e:?}"));
        let old_held = app.world().resource::<Playfield>().supply.held().clone();
        key(&mut app, KeyCode::ArrowDown);
        step(&mut app, 0.17);
        let refilled = app.world().resource::<Playfield>().supply.held().clone();
        assert_ne!(old_held.kind, BallKind::RowLaser);
        step(&mut app, 0.46);
        let field = app.world().resource::<Playfield>();
        assert_eq!(field.progression.score(), 600);
        assert_eq!(field.supply.held(), &refilled);
        let front = field
            .supply
            .previews(5)
            .unwrap_or_else(|| panic!("reward lane"));
        assert_eq!(front[0].kind, BallKind::Normal);
        let kinds = queued_kinds(&field.supply, 5, 4);
        assert!(kinds.contains(&BallKind::RowLaser));
        assert!(kinds.contains(&BallKind::Feather));
    }
    #[test]
    fn real_column_reward_keeps_held_and_all_old_supply_entries() {
        let (mut app, _) = input_app();
        *app.world_mut().resource_mut::<Playfield>() =
            special_demo("column-reward").unwrap_or_else(|e| panic!("fixture {e:?}"));
        key(&mut app, KeyCode::ArrowDown);
        let held = app.world().resource::<Playfield>().supply.held().clone();
        step(&mut app, 0.17);
        step(&mut app, 0.46);
        let field = app.world().resource::<Playfield>();
        assert_eq!(field.progression.score(), 1700);
        assert_eq!(field.supply.held(), &held);
        assert!(queued_kinds(&field.supply, 3, 4).contains(&BallKind::ColumnLaser));
    }
    fn queued_kinds(supply: &Supply, lane: usize, count: usize) -> Vec<BallKind> {
        let mut copy = supply.clone();
        (0..count)
            .map(|_| {
                copy.take_and_refill(lane, ball(1))
                    .unwrap_or_else(|e| panic!("queue fixture {e:?}"));
                copy.held().kind
            })
            .collect()
    }
    #[test]
    fn real_immediate_drops_preserve_reserved_next_then_fifo_rewards_then_old_second() {
        let (mut app, _) = input_app();
        *app.world_mut().resource_mut::<Playfield>() =
            special_demo("order").unwrap_or_else(|e| panic!("fixture {e:?}"));
        let original = app.world().resource::<Playfield>().supply.clone();
        let b = original.previews(0).unwrap_or_else(|| panic!("preview"))[0].clone();
        let mut thrown = Vec::new();
        for _ in 0..5 {
            thrown.push(app.world().resource::<Playfield>().supply.held().clone());
            key(&mut app, KeyCode::ArrowDown);
        }
        assert_eq!(thrown[1], b);
        assert_eq!(
            thrown.iter().map(|ball| ball.kind).collect::<Vec<_>>(),
            [
                BallKind::Normal,
                BallKind::Normal,
                BallKind::SilverStar,
                BallKind::UltimateJoker,
                BallKind::Normal
            ]
        );
        assert_eq!(thrown[4].colors, BallColors::single(ColorId::YELLOW));
        assert_eq!(app.world().resource::<Motion>().incoming.len(), 4);
        key(&mut app, KeyCode::Backspace);
        assert!(app.world().resource::<Motion>().active.is_none());
        assert!(app.world().resource::<Motion>().incoming.is_empty());
        assert!(
            !queued_kinds(&app.world().resource::<Playfield>().supply, 0, 3)
                .contains(&BallKind::UltimateJoker)
        );
    }
    #[test]
    fn real_launch_clear_earns_one_ultimate_while_direct_clear_does_not() {
        let (mut app, _) = input_app();
        *app.world_mut().resource_mut::<Playfield>() =
            special_demo("ultimate").unwrap_or_else(|e| panic!("fixture {e:?}"));
        key(&mut app, KeyCode::ArrowDown);
        step(&mut app, 0.17);
        assert!(matches!(
            app.world()
                .resource::<Motion>()
                .active
                .as_ref()
                .map(|t| &t.path),
            Some(Path::Launch { .. })
        ));
        step(&mut app, 1.0);
        assert_eq!(app.world().resource::<Playfield>().progression.score(), 0);
        step(&mut app, 0.46);
        let field = app.world().resource::<Playfield>();
        assert_eq!(field.progression.score(), 400);
        assert_eq!(
            queued_kinds(&field.supply, 7, 5)
                .iter()
                .filter(|kind| **kind == BallKind::UltimateJoker)
                .count(),
            1
        );
        key(&mut app, KeyCode::Backspace);
        assert!(app.world().resource::<Clearing>().pending.is_empty());
        assert!(
            !queued_kinds(&app.world().resource::<Playfield>().supply, 7, 5)
                .contains(&BallKind::UltimateJoker)
        );
        *app.world_mut().resource_mut::<Playfield>() =
            special_demo("reward").unwrap_or_else(|e| panic!("fixture {e:?}"));
        key(&mut app, KeyCode::ArrowDown);
        step(&mut app, 0.17);
        step(&mut app, 0.46);
        assert!(
            !queued_kinds(&app.world().resource::<Playfield>().supply, 5, 5)
                .contains(&BallKind::UltimateJoker)
        );
    }
    #[test]
    fn splitmix_sequence_is_repeatable_and_distinct() {
        let mut random = Random(0);
        assert_eq!(random.next(), 0xe220a8397b1dcdaf);
        assert_eq!(random.next(), 0x6e789e6aa1b965f4);
    }
    #[test]
    fn launch_level_accepts_full_positive_u32_range() {
        for level in ["1", "4", "4294967295"] {
            assert!(launch_level(&["game".into(), "--level".into(), level.into()]).is_ok());
        }
        for args in [
            vec!["--level".into()],
            vec!["--level".into(), "0".into()],
            vec!["--level".into(), "4294967296".into()],
        ] {
            assert!(launch_level(&args).is_err());
        }
    }
    #[test]
    fn platform_endpoints_change_one_row_and_share_one_ceiling() {
        let mut board = Board::default();
        assert_eq!(platform_y(&board, 0), 320.0 - 8.0 * ROW_HEIGHT);
        assert_eq!(platform_y(&board, 1), 320.0 - 8.0 * ROW_HEIGHT);
        board.place(
            0,
            Ball {
                kind: BallKind::Normal,
                weight: 2,
                colors: BallColors::single(ColorId::RED),
            },
        );
        assert_eq!(platform_y(&board, 0), 320.0 - 9.0 * ROW_HEIGHT);
        assert_eq!(platform_y(&board, 1), 320.0 - 7.0 * ROW_HEIGHT);
        for lane in 0..LANE_COUNT {
            assert_eq!(
                platform_y(&board, lane) + board.capacity(lane).unwrap_or(0) as f32 * ROW_HEIGHT,
                320.0
            );
        }
    }
    #[test]
    fn actual_keyboard_moves_held_ball_without_changing_supply() {
        let (mut app, _) = input_app();
        let held = app.world().resource::<Playfield>().supply.held().clone();
        let previews = app
            .world()
            .resource::<Playfield>()
            .supply
            .previews(0)
            .cloned();
        for _ in 0..12 {
            key(&mut app, KeyCode::ArrowRight);
        }
        let field = app.world().resource::<Playfield>();
        assert_eq!(field.chute.lane(), 7);
        assert_eq!(field.supply.held(), &held);
        assert_eq!(field.supply.previews(0).cloned(), previews);
    }
    #[test]
    fn actual_touch_tap_moves_chute_but_drag_only_inspects() {
        let (mut app, window) = input_app();
        touch(&mut app, window, TouchPhase::Started, 350.0, 0.0, 1);
        touch(&mut app, window, TouchPhase::Ended, 350.0, 0.0, 1);
        assert_eq!(app.world().resource::<Playfield>().chute.lane(), 7);
        {
            let mut field = app.world_mut().resource_mut::<Playfield>();
            for _ in 0..14 {
                field.board.place(
                    0,
                    Ball {
                        kind: BallKind::Normal,
                        weight: 1,
                        colors: BallColors::single(ColorId::RED),
                    },
                );
            }
        }
        touch(&mut app, window, TouchPhase::Started, -350.0, 0.0, 2);
        touch(&mut app, window, TouchPhase::Moved, -350.0, -200.0, 2);
        touch(&mut app, window, TouchPhase::Ended, -350.0, -200.0, 2);
        let field = app.world().resource::<Playfield>();
        assert_eq!(field.chute.lane(), 7);
        assert!(field.offset > 0);
        assert_eq!(field.board.stack(0).map(<[Ball]>::len), Some(14));
    }
    #[test]
    fn real_keyboard_drop_preserves_held_and_refills_only_destination() {
        let (mut app, _) = input_app();
        let held = app.world().resource::<Playfield>().supply.held().clone();
        let source = app
            .world()
            .resource::<Playfield>()
            .supply
            .previews(1)
            .cloned()
            .unwrap_or_else(|| panic!("lane exists"));
        let untouched = app
            .world()
            .resource::<Playfield>()
            .supply
            .previews(0)
            .cloned();
        key(&mut app, KeyCode::ArrowRight);
        key(&mut app, KeyCode::ArrowDown);
        assert_eq!(app.world().resource::<Playfield>().offset, 0);
        assert!(app.world().resource::<Playfield>().busy);
        assert_eq!(app.world().resource::<Playfield>().board.total(1), 0);
        advance(&mut app);
        let field = app.world().resource::<Playfield>();
        assert_eq!(field.board.stack(1).and_then(|s| s.first()), Some(&held));
        assert_eq!(field.supply.held(), &source[0]);
        assert_eq!(field.supply.previews(1).map(|p| &p[0]), Some(&source[1]));
        assert_eq!(field.supply.previews(0).cloned(), untouched);
        key(&mut app, KeyCode::Enter);
        advance(&mut app);
        let field = app.world().resource::<Playfield>();
        assert_eq!(
            field.board.stack(1).and_then(|s| s.get(1)),
            Some(&source[0])
        );
        assert_eq!(field.supply.held(), &source[1]);
    }
    #[test]
    fn real_touch_selection_ignores_same_and_next_frame_synthesized_mouse() {
        use bevy::input::{ButtonState, mouse::MouseButtonInput};
        let (mut app, window) = input_app();
        let supply = app.world().resource::<Playfield>().supply.clone();
        let scale = view_scale(390.0, 844.0).unwrap_or(1.0);
        app.world_mut()
            .entity_mut(window)
            .get_mut::<Window>()
            .unwrap_or_else(|| panic!("window"))
            .set_cursor_position(Some(Vec2::new(195.0 - 350.0 / scale, 422.0)));
        touch(&mut app, window, TouchPhase::Started, 350.0, 0.0, 22);
        app.world_mut().write_message(MouseButtonInput {
            button: MouseButton::Left,
            state: ButtonState::Pressed,
            window,
        });
        touch(&mut app, window, TouchPhase::Ended, 350.0, 0.0, 22);
        assert_eq!(app.world().resource::<Playfield>().chute.lane(), 7);
        app.world_mut().write_message(MouseButtonInput {
            button: MouseButton::Left,
            state: ButtonState::Released,
            window,
        });
        app.world_mut().write_message(MouseButtonInput {
            button: MouseButton::Left,
            state: ButtonState::Pressed,
            window,
        });
        app.update();
        let field = app.world().resource::<Playfield>();
        assert_eq!(field.chute.lane(), 7);
        assert_eq!(field.supply, supply);
        assert_eq!(field.board, Board::default());
        assert!(app.world().resource::<Motion>().active.is_none());
    }
    #[test]
    fn untouched_raised_stack_ends_game_blocks_supply_and_reset_restores() {
        let (mut app, _) = input_app();
        {
            let mut field = app.world_mut().resource_mut::<Playfield>();
            for lane in 0..2 {
                for row in 0..8 {
                    field.board.place(
                        lane,
                        Ball {
                            kind: BallKind::Normal,
                            weight: 1,
                            colors: BallColors::single(ColorId::new((row % 2) as u32)),
                        },
                    );
                }
            }
            assert!(!field.board.is_game_over());
        }
        key(&mut app, KeyCode::Space);
        advance(&mut app);
        let field = app.world().resource::<Playfield>();
        assert!(field.game_over);
        assert_eq!(field.board.capacity(0), Some(9));
        assert_eq!(field.board.capacity(1), Some(7));
        assert_eq!(field.board.overflow(1), Some(1));
        let stopped = field.supply.clone();
        let state = field.random.0;
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::ArrowRight);
        key(&mut app, KeyCode::Space);
        let field = app.world().resource::<Playfield>();
        assert_eq!(field.supply, stopped);
        assert_eq!(field.random.0, state);
        assert_eq!(field.chute.lane(), 1);
        key(&mut app, KeyCode::Backspace);
        let field = app.world().resource::<Playfield>();
        assert!(!field.game_over);
        assert_eq!(field.board, Board::default());
        assert_eq!(field.chute.lane(), 0);
        assert_ne!(field.random.0, state);
    }
    fn ball(weight: u32) -> Ball {
        Ball {
            kind: BallKind::Normal,
            weight,
            colors: BallColors::single(ColorId::RED),
        }
    }
    fn mass(field: &Playfield, motion: &Motion) -> u64 {
        (0..LANE_COUNT)
            .map(|lane| field.board.total(lane))
            .sum::<u64>()
            + motion
                .transits()
                .map(|(_, t)| u64::from(t.ball.weight))
                .sum::<u64>()
            + motion
                .queued
                .iter()
                .map(|t| u64::from(t.ball.weight))
                .sum::<u64>()
    }
    #[test]
    fn two_launch_chain_is_serial_preserves_balls_and_refills_only_once() {
        let (mut app, _) = input_app();
        {
            let mut field = app.world_mut().resource_mut::<Playfield>();
            field.config = GeneratorConfig::new(NonZeroU32::new(5).unwrap_or(NonZeroU32::MIN));
            field.config.joker_percent = 0;
            field.config.bomb_percent = 0;
            field.supply =
                Supply::new(field.config, &mut || 101).unwrap_or_else(|e| panic!("fixture {e:?}"));
            field.board.place(0, ball(3));
            field.board.place(3, ball(1));
            field.chute.shift(1);
        }
        key(&mut app, KeyCode::Space);
        let supply = app.world().resource::<Playfield>().supply.clone();
        let random = app.world().resource::<Playfield>().random.0;
        // No further user throw: the reaction chain itself must not refill.
        assert_eq!(app.world().resource::<Playfield>().supply, supply);
        advance(&mut app);
        let first = app
            .world()
            .resource::<Motion>()
            .active
            .as_ref()
            .unwrap_or_else(|| panic!("first launch"));
        assert_eq!(first.ball.weight, 3);
        assert_eq!(first.destination, 2);
        assert!(matches!(first.path, Path::Launch { distance: 2, .. }));
        assert_eq!(
            mass(
                app.world().resource::<Playfield>(),
                app.world().resource::<Motion>()
            ),
            9
        );
        advance(&mut app);
        let second = app
            .world()
            .resource::<Motion>()
            .active
            .as_ref()
            .unwrap_or_else(|| panic!("second launch"));
        assert_eq!(second.ball.weight, 1);
        assert_eq!(second.destination, 1);
        advance(&mut app);
        assert!(app.world().resource::<Motion>().active.is_none());
        let field = app.world().resource::<Playfield>();
        assert!(!field.busy);
        assert_eq!(field.supply, supply);
        assert_eq!(field.random.0, random);
        assert_eq!(mass(field, app.world().resource::<Motion>()), 9);
        assert_eq!(field.board.stack(1).map(<[Ball]>::len), Some(2));
        assert_eq!(field.board.stack(2).map(<[Ball]>::len), Some(1));
    }
    #[test]
    fn huge_wraps_are_bounded_and_end_at_the_core_destination() {
        for (origin, destination, rightward) in [(6, 0, true), (1, 7, false)] {
            let distance = u64::MAX - 5; // remainder2, preserved without narrowing.
            let transit = Transit {
                ball: ball(1),
                destination,
                path: Path::Launch {
                    origin,
                    distance,
                    rightward,
                    start_y: 0.0,
                    edge_from: None,
                },
                elapsed: 1.0,
                duration: 1.0,
            };
            assert_eq!(visible_steps(distance), 10.0);
            let position = transit_position(&transit, &Board::default(), 0);
            assert!((position.x - lane_x(destination)).abs() < 0.01);
            assert!(matches!(transit.path,Path::Launch{distance:exact,..} if exact==distance));
        }
    }
    #[test]
    fn reset_cancels_transit_and_overflow_freezes_emitted_ball() {
        let (mut app, _) = input_app();
        key(&mut app, KeyCode::Space);
        assert!(app.world().resource::<Motion>().active.is_some());
        key(&mut app, KeyCode::Backspace);
        assert!(app.world().resource::<Motion>().active.is_none());
        advance(&mut app);
        assert_eq!(app.world().resource::<Playfield>().board, Board::default());
        {
            let mut field = app.world_mut().resource_mut::<Playfield>();
            for row in 0..9 {
                field.board.place(
                    0,
                    Ball {
                        kind: BallKind::Normal,
                        weight: 1,
                        colors: BallColors::single(ColorId::new((row % 2) as u32)),
                    },
                );
            }
            field.board.place(1, ball(8));
            field.busy = true;
        }
        app.world_mut().resource_mut::<Motion>().start(Transit {
            ball: ball(2),
            destination: 1,
            path: Path::Drop { start_y: 570.0 },
            elapsed: 0.0,
            duration: 0.5,
        });
        advance(&mut app);
        assert!(app.world().resource::<Playfield>().game_over);
        let frozen = app
            .world()
            .resource::<Motion>()
            .active
            .clone()
            .unwrap_or_else(|| panic!("emitted ball preserved"));
        assert_eq!(frozen.ball.weight, 1);
        advance(&mut app);
        assert_eq!(
            app.world()
                .resource::<Motion>()
                .active
                .as_ref()
                .map(|t| t.elapsed),
            Some(0.0)
        );
        key(&mut app, KeyCode::Backspace);
        assert!(app.world().resource::<Motion>().active.is_none());
        assert!(!app.world().resource::<Playfield>().game_over);
    }
    #[test]
    fn explicit_demo_starts_only_on_input_and_launches_four_then_one() {
        let (mut app, _) = input_app();
        *app.world_mut().resource_mut::<Playfield>() =
            flight_demo().unwrap_or_else(|e| panic!("fixture {e:?}"));
        assert!(app.world().resource::<Motion>().active.is_none());
        assert_eq!(app.world().resource::<Playfield>().supply.held().weight, 5);
        key(&mut app, KeyCode::Space);
        let supply = app.world().resource::<Playfield>().supply.clone();
        advance(&mut app);
        assert!(matches!(
            app.world()
                .resource::<Motion>()
                .active
                .as_ref()
                .map(|t| &t.path),
            Some(Path::Launch { distance: 4, .. })
        ));
        advance(&mut app);
        assert!(matches!(
            app.world()
                .resource::<Motion>()
                .active
                .as_ref()
                .map(|t| &t.path),
            Some(Path::Launch { distance: 1, .. })
        ));
        advance(&mut app);
        assert!(app.world().resource::<Motion>().active.is_none());
        assert_eq!(app.world().resource::<Playfield>().supply, supply);
        key(&mut app, KeyCode::Backspace);
        assert_eq!(app.world().resource::<Playfield>().board, Board::default());
    }
    fn level_one_app() -> App {
        let (mut app, _) = input_app();
        *app.world_mut().resource_mut::<Playfield>() = test_field(NonZeroU32::MIN, 31);
        app
    }
    #[test]
    fn actual_keypresses_clear_four_and_continue_into_level_two() {
        let mut app = level_one_app();
        for group in 0..3 {
            // Keep this progression scenario on ordinary balls; earned utilities
            // have their own real-input/front-queue tests and stay queued.
            fixture_held(&mut app.world_mut().resource_mut::<Playfield>(), ball(1));
            for _ in 0..3 {
                key(&mut app, KeyCode::ArrowLeft);
            }
            for lane in 0..4 {
                if lane > 0 {
                    key(&mut app, KeyCode::ArrowRight);
                }
                key(&mut app, KeyCode::Space);
                advance(&mut app);
            }
            let field = app.world().resource::<Playfield>();
            assert_eq!(field.progression.score(), (group + 1) * 400);
            assert_eq!(field.board, Board::default());
        }
        let field = app.world().resource::<Playfield>();
        assert_eq!(field.progression.level().get(), 2);
        assert_eq!(field.progression.remaining(), 1800);
        assert_eq!(field.config.level.get(), 2);
        let mut random = field.random;
        let expected = field
            .config
            .generate(&mut || random.next())
            .unwrap_or_else(|e| panic!("generation {e:?}"));
        for _ in 0..3 {
            key(&mut app, KeyCode::ArrowLeft);
        }
        key(&mut app, KeyCode::Space);
        let field = app.world().resource::<Playfield>();
        // The new sample is appended behind losslessly retained rewards and
        // older previews, so it need not be among the two visible fronts yet.
        assert_eq!(field.random.0, random.0);
        assert!((1..=2).contains(&expected.weight));
        assert!(expected.colors.iter().all(|color| color.index() < 2));
        assert_eq!(field.progression.score(), 1200);
    }
    #[test]
    fn five_real_drops_compress_to_five_without_scoring() {
        let mut app = level_one_app();
        for _ in 0..5 {
            key(&mut app, KeyCode::Space);
            advance(&mut app);
        }
        let field = app.world().resource::<Playfield>();
        assert_eq!(field.board.stack(0), Some([ball(5)].as_slice()));
        assert_eq!(field.progression.score(), 0);
        assert!(
            field
                .feedback
                .as_ref()
                .is_some_and(|text| text.contains("5->1 x1"))
        );
    }
    #[test]
    fn queued_flights_allow_user_drops_and_reset_discards_every_owned_transit() {
        let (mut app, _) = input_app();
        let supply = app.world().resource::<Playfield>().supply.clone();
        app.world_mut()
            .resource_mut::<Motion>()
            .queued
            .push_back(Transit {
                ball: ball(2),
                destination: 0,
                path: Path::Drop { start_y: 570.0 },
                elapsed: 0.0,
                duration: 0.5,
            });
        let previous_random = app.world().resource::<Playfield>().random.0;
        key(&mut app, KeyCode::ArrowDown);
        assert!(app.world().resource::<Motion>().active.is_some());
        assert_eq!(app.world().resource::<Motion>().queued.len(), 1);
        assert_ne!(
            app.world().resource::<Playfield>().random.0,
            previous_random
        );
        let _ = supply;
        key(&mut app, KeyCode::Backspace);
        assert!(app.world().resource::<Motion>().queued.is_empty());
        assert!(app.world().resource::<Motion>().active.is_none());
    }
    #[test]
    fn puzzle_demo_clears_all_eleven_before_compression_for_twelve_hundred() {
        let (mut app, _) = input_app();
        *app.world_mut().resource_mut::<Playfield>() =
            puzzle_demo().unwrap_or_else(|e| panic!("fixture {e:?}"));
        assert_eq!(app.world().resource::<Playfield>().supply.held().weight, 2);
        key(&mut app, KeyCode::Space);
        step(&mut app, 0.17);
        assert_eq!(app.world().resource::<Playfield>().progression.score(), 0);
        // Matching wins priority: all five uncompressed yellows remain eligible
        // to join the joker bridge, preserving their full score.
        assert_eq!(
            app.world()
                .resource::<Playfield>()
                .board
                .stack(6)
                .map(<[Ball]>::len),
            Some(5)
        );
        step(&mut app, 0.46);
        let field = app.world().resource::<Playfield>();
        assert_eq!(field.progression.score(), 1200);
        assert!((0..LANE_COUNT).all(|lane| field.board.stack(lane) == Some([].as_slice())));
        assert!(
            field
                .feedback
                .as_ref()
                .is_some_and(|text| text.contains("Cleared 11 | +1200"))
        );
        assert!(!field.game_over);
    }
    #[test]
    fn resolution_errors_preserve_board_score_and_every_in_transit_ball() {
        let (mut app, _) = input_app();
        {
            let mut field = app.world_mut().resource_mut::<Playfield>();
            for _ in 0..4 {
                field.board.place(0, ball(u32::MAX));
            }
            field.busy = true;
        }
        let before = app.world().resource::<Playfield>().board.clone();
        let progression = app.world().resource::<Playfield>().progression.clone();
        app.world_mut().resource_mut::<Motion>().start(Transit {
            ball: ball(u32::MAX),
            destination: 0,
            path: Path::Drop { start_y: 570.0 },
            elapsed: 0.0,
            duration: 0.5,
        });
        advance(&mut app);
        let field = app.world().resource::<Playfield>();
        assert_eq!(field.board, before);
        assert_eq!(field.progression, progression);
        assert!(field.error.is_some());
        assert!(!field.game_over);
        assert_eq!(
            app.world()
                .resource::<Motion>()
                .active
                .as_ref()
                .map(|t| t.ball.weight),
            Some(u32::MAX)
        );
        let supply = field.supply.clone();
        key(&mut app, KeyCode::Space);
        assert_eq!(app.world().resource::<Playfield>().supply, supply);
        key(&mut app, KeyCode::Backspace);
        assert!(app.world().resource::<Playfield>().error.is_none());
        assert!(app.world().resource::<Motion>().active.is_none());
    }
    #[test]
    fn newly_emitted_flights_join_after_existing_owned_queue_in_core_order() {
        let (mut app, _) = input_app();
        {
            let mut field = app.world_mut().resource_mut::<Playfield>();
            for (left, id) in [(0, 10), (2, 20)] {
                for (weight, color) in [(1, id), (1, id), (5, 0)] {
                    field.board.place(
                        left,
                        Ball {
                            kind: BallKind::Normal,
                            weight,
                            colors: BallColors::single(ColorId::new(color)),
                        },
                    );
                }
                for (weight, color) in [(1, 0), (5, id + 1)] {
                    field.board.place(
                        left + 1,
                        Ball {
                            kind: BallKind::Normal,
                            weight,
                            colors: BallColors::single(ColorId::new(color)),
                        },
                    );
                }
            }
            field.busy = true;
        }
        {
            let mut motion = app.world_mut().resource_mut::<Motion>();
            motion.start(Transit {
                ball: Ball {
                    kind: BallKind::Normal,
                    weight: 1,
                    colors: BallColors::single(ColorId::new(99)),
                },
                destination: 7,
                path: Path::Drop { start_y: 570.0 },
                elapsed: 0.0,
                duration: 0.5,
            });
            motion.queued.push_back(Transit {
                ball: ball(42),
                destination: 6,
                path: Path::Drop { start_y: 570.0 },
                elapsed: 0.0,
                duration: 0.5,
            });
        }
        advance(&mut app);
        let motion = app.world().resource::<Motion>();
        assert_eq!(motion.active.as_ref().map(|t| t.ball.weight), Some(42));
        assert_eq!(motion.queued.len(), 2);
        assert!(matches!(
            motion.queued[0].path,
            Path::Launch { origin: 0, .. }
        ));
        assert!(matches!(
            motion.queued[1].path,
            Path::Launch { origin: 2, .. }
        ));
        assert_eq!(
            app.world().resource::<Playfield>().progression.score(),
            1200
        );
    }
    fn step(app: &mut App, seconds: f64) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f64(seconds));
        app.world_mut().run_schedule(Update);
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::ZERO);
    }
    fn begin_four_clear() -> App {
        let mut app = level_one_app();
        for lane in 0..3 {
            if lane > 0 {
                key(&mut app, KeyCode::ArrowRight);
            }
            key(&mut app, KeyCode::ArrowDown);
            advance(&mut app);
        }
        key(&mut app, KeyCode::ArrowRight);
        key(&mut app, KeyCode::ArrowDown);
        let transit = app
            .world()
            .resource::<Motion>()
            .active
            .as_ref()
            .unwrap_or_else(|| panic!("incoming"));
        assert!((transit.duration - 1.0 / 6.0).abs() < 0.00001);
        assert_eq!(app.world().resource::<Playfield>().offset, 0);
        step(&mut app, 0.17);
        assert_eq!(app.world().resource::<Clearing>().pending.cells().len(), 4);
        assert_eq!(app.world().resource::<Playfield>().progression.score(), 0);
        app
    }
    #[test]
    fn fast_followup_joins_fixed_clear_window_and_awards_five_hundred_once() {
        let mut app = begin_four_clear();
        let deadline = app.world().resource::<Clearing>().deadline;
        for _ in 0..3 {
            key(&mut app, KeyCode::ArrowLeft);
        }
        key(&mut app, KeyCode::ArrowDown);
        step(&mut app, 0.17);
        assert_eq!(app.world().resource::<Clearing>().deadline, deadline);
        assert_eq!(app.world().resource::<Clearing>().pending.cells().len(), 5);
        assert_eq!(app.world().resource::<Playfield>().progression.score(), 0);
        step(&mut app, 0.28);
        let field = app.world().resource::<Playfield>();
        assert_eq!(field.progression.score(), 500);
        assert_eq!(field.board, Board::default());
        assert!(app.world().resource::<Clearing>().pending.is_empty());
        advance(&mut app);
        assert_eq!(app.world().resource::<Playfield>().progression.score(), 500);
    }
    #[test]
    fn arrival_after_clear_deadline_stays_a_normal_new_ball() {
        let mut app = begin_four_clear();
        step(&mut app, 0.40);
        for _ in 0..3 {
            key(&mut app, KeyCode::ArrowLeft);
        }
        key(&mut app, KeyCode::ArrowDown);
        step(&mut app, 0.17);
        let field = app.world().resource::<Playfield>();
        assert_eq!(field.progression.score(), 400);
        assert_eq!(field.board.stack(0), Some([ball(1)].as_slice()));
        assert!(app.world().resource::<Clearing>().pending.is_empty());
    }
    #[test]
    fn multiple_incoming_streams_are_allowed_during_glow_and_reset_cancels_all() {
        let mut app = begin_four_clear();
        key(&mut app, KeyCode::ArrowDown);
        key(&mut app, KeyCode::ArrowDown);
        assert_eq!(app.world().resource::<Motion>().incoming.len(), 1);
        assert!(app.world().resource::<Motion>().active.is_some());
        key(&mut app, KeyCode::Backspace);
        assert!(app.world().resource::<Clearing>().pending.is_empty());
        assert!(app.world().resource::<Clearing>().deadline.is_none());
        let motion = app.world().resource::<Motion>();
        assert!(motion.active.is_none());
        assert!(motion.incoming.is_empty());
        assert!(motion.queued.is_empty());
        advance(&mut app);
        assert_eq!(app.world().resource::<Playfield>().board, Board::default());
        assert_eq!(app.world().resource::<Playfield>().progression.score(), 0);
    }
    #[test]
    fn long_frame_commits_two_expired_clear_phases_before_a_late_arrival() {
        let (mut app, _) = input_app();
        {
            let mut field = app.world_mut().resource_mut::<Playfield>();
            field.progression = Progression::new(NonZeroU32::MIN);
            field.config.level = NonZeroU32::MIN;
            for (lane, stack) in [
                (0, vec![(1, 0), (1, 0), (1, 1)]),
                (1, vec![(2, 0), (1, 1)]),
                (2, vec![(2, 0), (1, 1)]),
                (3, vec![(2, 0), (1, 20)]),
            ] {
                for (weight, id) in stack {
                    field.board.place(
                        lane,
                        Ball {
                            kind: BallKind::Normal,
                            weight,
                            colors: BallColors::single(ColorId::new(id)),
                        },
                    );
                }
            }
            field.busy = true;
        }
        {
            let mut motion = app.world_mut().resource_mut::<Motion>();
            motion.start(Transit {
                ball: Ball {
                    kind: BallKind::Normal,
                    weight: 1,
                    colors: BallColors::single(ColorId::new(99)),
                },
                destination: 7,
                path: Path::Drop { start_y: 570.0 },
                elapsed: 0.0,
                duration: 1.0 / 6.0,
            });
            motion.drop(Transit {
                ball: Ball {
                    kind: BallKind::Normal,
                    weight: 1,
                    colors: BallColors::single(ColorId::BLUE),
                },
                destination: 0,
                path: Path::Drop { start_y: 570.0 },
                elapsed: 0.0,
                duration: 1.5,
            });
        }
        advance(&mut app);
        let field = app.world().resource::<Playfield>();
        assert_eq!(field.progression.score(), 1100);
        assert_eq!(field.progression.level().get(), 2);
        assert_eq!(field.progression.remaining(), 1900);
        assert_eq!(
            field.board.stack(0),
            Some(
                [Ball {
                    kind: BallKind::Normal,
                    weight: 1,
                    colors: BallColors::single(ColorId::BLUE)
                }]
                .as_slice()
            )
        );
        assert_eq!(field.board.total(3), 1);
        assert_eq!(field.board.total(7), 1);
        assert!(app.world().resource::<Clearing>().pending.is_empty());
        assert!(app.world().resource::<Clearing>().deadline.is_none());
    }
    #[test]
    fn four_immediate_down_presses_own_four_balls_before_any_landing() {
        let (mut app, _) = input_app();
        let held = app.world().resource::<Playfield>().supply.held().clone();
        let previews = app
            .world()
            .resource::<Playfield>()
            .supply
            .previews(0)
            .cloned()
            .unwrap_or_else(|| panic!("lane"));
        for _ in 0..4 {
            key(&mut app, KeyCode::ArrowDown);
        }
        let motion = app.world().resource::<Motion>();
        let balls = motion
            .transits()
            .map(|(_, transit)| transit.ball.clone())
            .collect::<Vec<_>>();
        assert_eq!(balls.len(), 4);
        assert_eq!(motion.incoming.len(), 3);
        assert_eq!(balls[0], held);
        assert_eq!(balls[1], previews[0]);
        assert_eq!(balls[2], previews[1]);
        assert_eq!(app.world().resource::<Playfield>().board, Board::default());
        let total = mass(app.world().resource::<Playfield>(), motion);
        step(&mut app, 0.17);
        let field = app.world().resource::<Playfield>();
        assert_eq!(field.board.stack(0), Some(balls.as_slice()));
        assert_eq!(mass(field, app.world().resource::<Motion>()), total);
        assert!(app.world().resource::<Motion>().incoming.is_empty());
        assert!(app.world().resource::<Motion>().active.is_none());
        key(&mut app, KeyCode::Backspace);
        for _ in 0..4 {
            key(&mut app, KeyCode::ArrowDown);
        }
        key(&mut app, KeyCode::Backspace);
        advance(&mut app);
        assert_eq!(app.world().resource::<Playfield>().board, Board::default());
        assert!(app.world().resource::<Motion>().incoming.is_empty());
    }
    #[test]
    fn real_drop_animates_crosses_and_lands_both_board_edges() {
        for rightward in [false, true] {
            let (mut app, _) = input_app();
            *app.world_mut().resource_mut::<Playfield>() =
                wrap_demo(rightward).unwrap_or_else(|e| panic!("fixture {e:?}"));
            let supply_before = app.world().resource::<Playfield>().supply.clone();
            key(&mut app, KeyCode::ArrowDown);
            assert_ne!(app.world().resource::<Playfield>().supply, supply_before);
            step(&mut app, 0.17);
            let transit = app
                .world()
                .resource::<Motion>()
                .active
                .clone()
                .unwrap_or_else(|| panic!("emitted ball"));
            let (origin, destination) = if rightward { (6, 0) } else { (1, 7) };
            assert_eq!(transit.destination, destination);
            assert!(
                matches!(transit.path,Path::Launch{origin:actual,distance:2,rightward:direction,..} if actual==origin&&direction==rightward)
            );
            let supply = app.world().resource::<Playfield>().supply.clone();
            let mut early = transit.clone();
            early.elapsed = early.duration * 0.7;
            let mut late = transit.clone();
            late.elapsed = late.duration * 0.9;
            let a = transit_position(&early, &app.world().resource::<Playfield>().board, 0).x;
            let b = transit_position(&late, &app.world().resource::<Playfield>().board, 0).x;
            if rightward {
                assert!(a > 350.0 && b < -350.0);
            } else {
                assert!(a < -350.0 && b > 350.0);
            }
            step(&mut app, 0.8);
            assert!(app.world().resource::<Motion>().active.is_some());
            step(&mut app, 0.2);
            let field = app.world().resource::<Playfield>();
            assert_eq!(field.board.stack(destination), Some([ball(1)].as_slice()));
            assert_eq!(field.board.total(if rightward { 7 } else { 0 }), 3);
            assert_eq!(mass(field, app.world().resource::<Motion>()), 4);
            assert_eq!(field.supply, supply);
            assert!(app.world().resource::<Motion>().active.is_none());
            assert!(app.world().resource::<Motion>().incoming.is_empty());
        }
    }
    #[test]
    fn burst_throws_during_an_edge_flight_preserve_every_ball_and_refill_order() {
        for rightward in [false, true] {
            let (mut app, _) = input_app();
            *app.world_mut().resource_mut::<Playfield>() =
                wrap_demo(rightward).unwrap_or_else(|e| panic!("fixture {e:?}"));
            key(&mut app, KeyCode::ArrowDown);
            step(&mut app, 0.17);
            assert!(matches!(
                app.world()
                    .resource::<Motion>()
                    .active
                    .as_ref()
                    .map(|t| &t.path),
                Some(Path::Launch { distance: 2, .. })
            ));
            for _ in 0..4 {
                key(&mut app, KeyCode::ArrowDown);
            }
            assert_eq!(app.world().resource::<Motion>().incoming.len(), 4);
            let total = mass(
                app.world().resource::<Playfield>(),
                app.world().resource::<Motion>(),
            );
            let supply = app.world().resource::<Playfield>().supply.clone();
            step(&mut app, 0.17);
            assert_eq!(
                mass(
                    app.world().resource::<Playfield>(),
                    app.world().resource::<Motion>()
                ),
                total
            );
            assert!(app.world().resource::<Motion>().incoming.is_empty());
            assert!(app.world().resource::<Motion>().active.is_some());
            step(&mut app, 1.0);
            let field = app.world().resource::<Playfield>();
            assert_eq!(mass(field, app.world().resource::<Motion>()), total);
            assert_eq!(field.supply, supply);
            assert_eq!(
                field.board.stack(if rightward { 0 } else { 7 }),
                Some([ball(1)].as_slice())
            );
            assert!(app.world().resource::<Motion>().active.is_none());
            assert!(app.world().resource::<Motion>().queued.is_empty());
        }
    }
    #[test]
    fn static_and_moving_joker_paths_spawn_identical_sector_cues_and_weight_only() {
        use bevy::ecs::system::RunSystemOnce;
        let (mut app, _) = input_app();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<ColorMaterial>>();
        let colors = BallColors::new(&[ColorId::new(0), ColorId::new(6), ColorId::new(11)])
            .unwrap_or_else(|| panic!("fixture colors"));
        let ball = Ball {
            kind: BallKind::Normal,
            weight: 12,
            colors,
        };
        let stationary = ball.clone();
        app.world_mut()
            .run_system_once(
                move |mut commands: Commands,
                      mut meshes: ResMut<Assets<Mesh>>,
                      mut materials: ResMut<Assets<ColorMaterial>>| {
                    draw_ball(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        &stationary,
                        Vec2::ZERO,
                        2.4,
                        Some(Cell { lane: 0, index: 0 }),
                    );
                },
            )
            .unwrap_or_else(|error| panic!("static drawing {error:?}"));
        app.world_mut().resource_mut::<Motion>().start(Transit {
            ball,
            destination: 0,
            path: Path::Drop { start_y: 570.0 },
            elapsed: 0.0,
            duration: 1.0 / 6.0,
        });
        app.world_mut()
            .run_system_once(draw_motion)
            .unwrap_or_else(|error| panic!("moving drawing {error:?}"));
        let world = app.world_mut();
        let mut query = world.query::<(&ball_visual::Marking, &ChildOf)>();
        let rows = query
            .iter(world)
            .map(|(mark, parent)| (mark.clone(), parent.parent()))
            .collect::<Vec<_>>();
        let mut static_marks = Vec::new();
        let mut moving_marks = Vec::new();
        for (mark, parent) in rows {
            if world.entity(parent).contains::<Scene>() {
                static_marks.push(mark);
            } else if world.entity(parent).contains::<TransitVisual>() {
                moving_marks.push(mark);
            }
        }
        static_marks.sort_by_key(|mark| mark.sector);
        moving_marks.sort_by_key(|mark| mark.sector);
        assert_eq!(static_marks.len(), 2);
        assert_eq!(static_marks, moving_marks);
        assert_eq!(
            static_marks
                .iter()
                .map(|mark| mark.id.index())
                .collect::<Vec<_>>(),
            vec![6, 11]
        );
        let mut text = world.query::<&Text2d>();
        assert!(text.iter(world).all(|text| text.0 == "12"));
    }
    #[test]
    fn palette_demo_covers_twelve_ids_two_digit_weights_and_owned_jokers() {
        let field = palette_demo().unwrap_or_else(|error| panic!("fixture {error:?}"));
        assert_eq!(field.config.level.get(), 12);
        assert_eq!(field.supply.held().weight, 12);
        let mut ids = std::collections::BTreeSet::new();
        ids.extend(field.supply.held().colors.iter().map(ColorId::index));
        for lane in 0..LANE_COUNT {
            for ball in field
                .supply
                .previews(lane)
                .unwrap_or_else(|| panic!("lane"))
            {
                ids.extend(ball.colors.iter().map(ColorId::index));
            }
        }
        assert_eq!(ids, (0..12).collect());
        assert_eq!(
            field
                .board
                .stack(6)
                .and_then(|stack| stack.first())
                .map(|ball| ball.weight),
            Some(125)
        );
        assert_eq!(
            field
                .board
                .stack(2)
                .and_then(|stack| stack.first())
                .map(|ball| ball.colors.count()),
            Some(2)
        );
        assert_eq!(
            field
                .board
                .stack(4)
                .and_then(|stack| stack.first())
                .map(|ball| ball.colors.count()),
            Some(3)
        );
    }
    #[test]
    fn real_zero_weight_star_drop_merges_to_gold_without_score() {
        let (mut app, _) = input_app();
        *app.world_mut().resource_mut::<Playfield>() =
            special_demo("merge").unwrap_or_else(|error| panic!("fixture {error:?}"));
        key(&mut app, KeyCode::ArrowDown);
        let incoming = app
            .world()
            .resource::<Motion>()
            .active
            .as_ref()
            .unwrap_or_else(|| panic!("star incoming"));
        assert_eq!(incoming.ball, Ball::silver_star());
        step(&mut app, 0.17);
        let field = app.world().resource::<Playfield>();
        assert_eq!(field.board.stack(0), Some([Ball::gold_star()].as_slice()));
        assert_eq!(field.progression.score(), 0);
        assert_eq!(field.progression.level().get(), 1);
        assert!(field.error.is_none());
    }
    #[test]
    fn real_bomb_throw_chains_and_scores_only_after_feedback_window() {
        let (mut app, _) = input_app();
        *app.world_mut().resource_mut::<Playfield>() =
            special_demo("bomb").unwrap_or_else(|error| panic!("fixture {error:?}"));
        key(&mut app, KeyCode::ArrowDown);
        step(&mut app, 0.17);
        let field = app.world().resource::<Playfield>();
        assert!((0..3).all(|lane| {
            field
                .board
                .stack(lane)
                .is_some_and(|stack| stack.is_empty())
        }));
        assert_eq!(field.board.total(3), 5);
        assert_eq!(field.progression.score(), 0);
        assert_eq!(app.world().resource::<Motion>().flashes.len(), 2);
        step(&mut app, 0.46);
        let field = app.world().resource::<Playfield>();
        assert_eq!(field.progression.score(), 2600);
        assert_eq!(field.progression.level().get(), 2);
        assert_eq!(
            field.supply.previews(0).map(|queue| queue[1].kind),
            Some(BallKind::SilverStar)
        );
    }
    #[test]
    fn silver_whole_board_clear_scores_without_advancing_level() {
        let (mut app, _) = input_app();
        *app.world_mut().resource_mut::<Playfield>() =
            special_demo("silver").unwrap_or_else(|error| panic!("fixture {error:?}"));
        key(&mut app, KeyCode::ArrowDown);
        step(&mut app, 0.17);
        assert!(
            (0..LANE_COUNT).all(|lane| app.world().resource::<Playfield>().board.total(lane) == 0)
        );
        assert_eq!(app.world().resource::<Playfield>().progression.score(), 0);
        step(&mut app, 0.46);
        let field = app.world().resource::<Playfield>();
        assert_eq!(field.progression.score(), 1000);
        assert_eq!(field.progression.level().get(), 1);
        assert_eq!(field.progression.remaining(), 1000);
    }
    #[test]
    fn third_gold_award_expands_ceiling_but_keeps_existing_stack_positions() {
        let (mut app, _) = input_app();
        *app.world_mut().resource_mut::<Playfield>() =
            special_demo("gold").unwrap_or_else(|error| panic!("fixture {error:?}"));
        let before = platform_y(&app.world().resource::<Playfield>().board, 7);
        assert_eq!(app.world().resource::<Playfield>().board.bonus_rows(), 2);
        key(&mut app, KeyCode::ArrowDown);
        step(&mut app, 0.17);
        let field = app.world().resource::<Playfield>();
        assert_eq!(field.board.bonus_rows(), 3);
        assert!(field.board.expansion_achievement());
        assert_eq!(platform_y(&field.board, 7), before);
        assert_eq!(field.board.stack(7), Some([Ball::stone()].as_slice()));
        assert_eq!(field.board.capacity(7), Some(12));
        assert!((0..5).all(|lane| {
            field
                .board
                .stack(lane)
                .is_some_and(|stack| stack.is_empty())
        }));
        let scale = view_scale_for(390.0, 844.0, 3).unwrap_or(1.0);
        assert!(844.0 * scale >= HEIGHT + 3.0 * ROW_HEIGHT);
        assert!(92.0 / scale >= 32.0);
        key(&mut app, KeyCode::Backspace);
        assert_eq!(app.world().resource::<Playfield>().board.bonus_rows(), 0);
        assert!(app.world().resource::<Motion>().flashes.is_empty());
    }
    #[test]
    fn silver_edge_visual_changes_once_but_owned_ball_is_already_bomb_ten() {
        let (mut app, _) = input_app();
        *app.world_mut().resource_mut::<Playfield>() =
            special_demo("edge").unwrap_or_else(|error| panic!("fixture {error:?}"));
        key(&mut app, KeyCode::ArrowDown);
        step(&mut app, 0.17);
        let active = app
            .world()
            .resource::<Motion>()
            .active
            .clone()
            .unwrap_or_else(|| panic!("star flight"));
        assert_eq!(active.ball, Ball::bomb());
        assert!(matches!(
            active.path,
            Path::Launch {
                origin: 1,
                distance: 2,
                edge_from: Some(BallKind::SilverStar),
                ..
            }
        ));
        assert_eq!(visual_ball(&active), Ball::silver_star());
        let mut before = active.clone();
        before.elapsed = before.duration * 0.74;
        let mut after = active.clone();
        after.elapsed = after.duration * 0.76;
        assert_eq!(visual_ball(&before), Ball::silver_star());
        assert_eq!(visual_ball(&after), Ball::bomb());
        step(&mut app, 1.0);
        assert_eq!(
            app.world().resource::<Playfield>().board.stack(7),
            Some([Ball::bomb()].as_slice())
        );
        assert!(app.world().resource::<Motion>().active.is_none());
        key(&mut app, KeyCode::Backspace);
        assert_eq!(app.world().resource::<Playfield>().board, Board::default());
    }
    #[test]
    fn normal_session_has_no_initial_star_reward_and_reward_can_be_thrown_weightless() {
        let field = Playfield::new(NonZeroU32::new(3).unwrap_or(NonZeroU32::MIN), Random(9))
            .unwrap_or_else(|error| panic!("fixture {error:?}"));
        assert!(matches!(
            field.supply.held().kind,
            BallKind::Normal | BallKind::Bomb
        ));
        for lane in 0..LANE_COUNT {
            assert!(field.supply.previews(lane).is_some_and(|queue| {
                queue
                    .iter()
                    .all(|ball| matches!(ball.kind, BallKind::Normal | BallKind::Bomb))
            }));
        }
        let (mut app, _) = input_app();
        let old = app.world().resource::<Playfield>().supply.held().clone();
        app.world_mut()
            .resource_mut::<Playfield>()
            .supply
            .insert_reward_front(0, Ball::silver_star())
            .unwrap_or_else(|error| panic!("reward {error:?}"));
        key(&mut app, KeyCode::ArrowDown);
        assert_eq!(
            app.world().resource::<Playfield>().supply.held(),
            &Ball::silver_star()
        );
        key(&mut app, KeyCode::ArrowRight);
        key(&mut app, KeyCode::ArrowDown);
        assert_eq!(
            app.world()
                .resource::<Motion>()
                .incoming
                .last()
                .map(|(_, transit)| &transit.ball),
            Some(&Ball::silver_star())
        );
        step(&mut app, 0.17);
        assert_eq!(
            app.world().resource::<Playfield>().board.stack(0),
            Some([old].as_slice())
        );
        assert_eq!(
            app.world().resource::<Playfield>().board.stack(1),
            Some([Ball::silver_star()].as_slice())
        );
    }
}
