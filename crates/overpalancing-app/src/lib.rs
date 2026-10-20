//! Level-driven supply and fixed-stack board shared by desktop and mobile.
use bevy::log::{error, info, warn};
use bevy::{prelude::*, window::PrimaryWindow};
use overpalancing_core::supply::{GenerationError, GeneratorConfig, Supply};
use overpalancing_core::{Ball, BallColors, Board, ColorId, Dispenser, LANE_COUNT, SEESAW_COUNT};
use overpalancing_core::{matching::Cell, resolution, scoring::Progression};
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
        })
    }
    fn drop_ball(&mut self, motion: &mut Motion, clearing: &Clearing) -> Result<(), &'static str> {
        if self.game_over
            || self.error.is_some()
            || (clearing.pending.is_empty()
                && (motion.active.is_some()
                    || !motion.queued.is_empty()
                    || !motion.incoming.is_empty()))
        {
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
        info!(event="drop_started",lane=destination,weight=held.weight,colors=?held.colors,queue=motion.queued.len(),"Drop started and supply refilled");
        motion.drop(Transit {
            ball: held,
            destination,
            path: Path::Drop,
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
    field.supply = Supply::new(field.config, &mut || u64::MAX - 1)?;
    for (lane, weights) in [(0, vec![1, 2]), (1, vec![2]), (4, vec![1]), (5, vec![2])] {
        for weight in weights {
            field.board.place(
                lane,
                Ball {
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
    let mut prefix = [0, 2, 99].into_iter();
    field.supply = Supply::new(field.config, &mut || prefix.next().unwrap_or(u64::MAX))?;
    for (lane, ids) in [vec![0], vec![0], vec![0, 1], vec![1], vec![1, 2], vec![2]]
        .into_iter()
        .enumerate()
    {
        let colors = BallColors::new(&ids.into_iter().map(ColorId::new).collect::<Vec<_>>())
            .unwrap_or_else(|| panic!("fixture colors"));
        field.board.place(lane, Ball { weight: 1, colors });
    }
    for _ in 0..4 {
        field.board.place(
            6,
            Ball {
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
    let field = match if args.iter().any(|arg| arg == "--puzzle-demo") {
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
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Overpalancing - level supply".into(),
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
                pulse_clear,
                draw_motion,
                smoke_test,
            )
                .chain(),
        )
        .run();
}
fn view_scale(width: f32, height: f32) -> Option<f32> {
    (width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0)
        .then(|| (WIDTH / width).max(HEIGHT / height))
}
fn fit_camera(
    window: Single<&Window, With<PrimaryWindow>>,
    mut camera: Single<&mut Projection, With<SceneCamera>>,
) {
    if let (Projection::Orthographic(projection), Some(scale)) =
        (&mut **camera, view_scale(window.width(), window.height()))
    {
        projection.scale = scale;
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
) {
    commands.spawn((
        Scene,
        Text2d::new(text),
        TextFont {
            font_size: bevy::text::FontSize::Px(size.max(14.0 * scale)),
            ..default()
        },
        TextLayout::justify(Justify::Center),
        TextColor(color),
        Transform::from_xyz(x, y, 10.0),
    ));
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
    let radius = (20.0 * scale).clamp(22.0, 40.0);
    let count = ball.colors.count();
    let mut parts = Vec::new();
    for (sector, id) in ball.colors.iter().enumerate() {
        let mesh = if count == 1 {
            meshes.add(Circle::new(radius))
        } else {
            meshes.add(CircularSector::new(
                radius,
                std::f32::consts::PI / count as f32,
            ))
        };
        parts.push((
            mesh,
            materials.add(color(id)),
            sector as f32 * std::f32::consts::TAU / count as f32,
        ));
    }
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
        for (mesh, material, angle) in parts {
            children.spawn((
                Mesh2d(mesh),
                MeshMaterial2d(material),
                Transform::from_rotation(Quat::from_rotation_z(angle)),
            ));
        }
        children.spawn((
            Text2d::new(ball.weight.to_string()),
            TextFont {
                font_size: bevy::text::FontSize::Px((14.0 * scale).max(20.0)),
                ..default()
            },
            TextLayout::justify(Justify::Center),
            TextColor(Color::BLACK),
            Transform::from_xyz(0.0, 0.0, 8.0),
        ));
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
    320.0 - board.capacity(lane).unwrap_or(8) as f32 * 90.0
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
    let Some(scale) = view_scale(size.x, size.y) else {
        return;
    };
    for entity in &scene {
        commands.entity(entity).despawn();
    }
    label(
        &mut commands,
        format!(
            "Level {} | Score {}",
            field.progression.level(),
            field.progression.score()
        ),
        0.0,
        1000.0,
        30.0,
        scale,
        INK,
    );
    label(
        &mut commands,
        format!("Next level: {} points", field.progression.remaining()),
        0.0,
        940.0,
        20.0,
        scale,
        INK,
    );
    for lane in 0..LANE_COUNT {
        let x = LEFT + (lane as f32 + 0.5) * LANE;
        rect(
            &mut commands,
            x,
            805.0,
            88.0,
            190.0,
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
                    Vec2::new(x, 760.0 + index as f32 * 90.0),
                    scale,
                    None,
                );
            }
        }
        label(
            &mut commands,
            format!("{}", lane + 1),
            x,
            680.0,
            20.0,
            scale,
            INK,
        );
        rect(
            &mut commands,
            x,
            -40.0,
            2.0,
            760.0,
            Color::srgb(0.1, 0.18, 0.25),
            0.0,
        );
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
                Vec2::new(x, base + 45.0 + index as f32 * 90.0),
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
        570.0,
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
        Vec2::new(x, 570.0),
        scale,
        None,
    );
    rect(
        &mut commands,
        x,
        510.0,
        92.0,
        18.0,
        Color::srgb(0.95, 0.75, 0.3),
        0.0,
    );
    rect(
        &mut commands,
        x,
        480.0,
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
fn pan(
    mut wheels: MessageReader<bevy::input::mouse::MouseWheel>,
    touches: Res<Touches>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut field: ResMut<Playfield>,
    mut carry: Local<f32>,
) {
    let Some(scale) = view_scale(window.width(), window.height()) else {
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
        while carry.abs() >= 90.0 {
            let direction = if *carry > 0.0 { 1 } else { -1 };
            rows += direction;
            *carry -= direction as f32 * 90.0;
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
    if let (Some(position), Some(scale)) = (pointer, view_scale(window.width(), window.height())) {
        let point = Vec2::new(
            (position.x - window.width() / 2.0) * scale,
            (window.height() / 2.0 - position.y) * scale,
        );
        if (-400.0..=400.0).contains(&point.x) && (-640.0..=640.0).contains(&point.y) {
            field
                .chute
                .select_fraction((point.x - LEFT) / (LANE * LANE_COUNT as f32));
        }
    }
    if reset_requested {
        field.reset();
        motion.clear();
        *clearing = Clearing::default();
    } else if drop_requested && let Err(error) = field.drop_ball(&mut motion, &clearing) {
        error!(event = "drop_failed", error, "Drop rejected");
        field.error = Some(error);
    }
}
#[derive(Clone)]
enum Path {
    Drop,
    Launch {
        origin: usize,
        distance: u64,
        rightward: bool,
        start_y: f32,
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
        self.active = None;
        self.revision = self.revision.wrapping_add(1);
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
    platform_y(board, lane) + 45.0 + board.stack(lane).map_or(0, <[Ball]>::len) as f32 * 90.0
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
    let target = landing_y(board, transit.destination) - offset as f32 * 90.0;
    match transit.path {
        Path::Drop => Vec2::new(
            lane_x(transit.destination),
            570.0 + (target - 570.0) * t * t,
        ),
        Path::Launch {
            origin,
            distance,
            rightward,
            start_y,
        } => {
            let steps = visible_steps(distance);
            let direction = if rightward { 1.0 } else { -1.0 };
            let x = LEFT
                + (lane_x(origin) - LEFT + direction * steps * LANE * t)
                    .rem_euclid(LANE * LANE_COUNT as f32);
            let from = start_y - offset as f32 * 90.0;
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
        let start_y =
            320.0 - flight.origin_capacity as f32 * 90.0 + 45.0 + flight.origin_index as f32 * 90.0;
        let duration = 0.9 + visible_steps(flight.distance) * 0.04;
        motion.queued.push_back(Transit {
            ball: flight.ball,
            destination: flight.destination,
            path: Path::Launch {
                origin: flight.origin,
                distance: flight.distance,
                rightward: flight.origin.is_multiple_of(2),
                start_y,
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
        match resolution::prepare_landing(
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
fn draw_motion(
    mut commands: Commands,
    state: (Res<Playfield>, Res<Motion>),
    window: Single<&Window, With<PrimaryWindow>>,
    assets: (ResMut<Assets<Mesh>>, ResMut<Assets<ColorMaterial>>),
    mut visuals: Query<(Entity, &mut Transform, &TransitVisual)>,
    mut cache: Local<(Option<u64>, f32)>,
) {
    let (field, motion) = state;
    let (mut meshes, mut materials) = assets;
    let Some(scale) = view_scale(window.width(), window.height()) else {
        return;
    };
    let rebuild = cache.0 != Some(motion.revision) || cache.1 != scale;
    if rebuild {
        for (entity, _, _) in &mut visuals {
            commands.entity(entity).despawn();
        }
        cache.0 = Some(motion.revision);
        cache.1 = scale;
        for (id, active) in motion.transits() {
            let position = transit_position(active, &field.board, field.offset);
            let radius = (20.0 * scale).clamp(22.0, 40.0);
            let mut parts = Vec::new();
            for (sector, id) in active.ball.colors.iter().enumerate() {
                let count = active.ball.colors.count();
                let mesh = if count == 1 {
                    meshes.add(Circle::new(radius))
                } else {
                    meshes.add(CircularSector::new(
                        radius,
                        std::f32::consts::PI / count as f32,
                    ))
                };
                parts.push((
                    mesh,
                    materials.add(color(id)),
                    sector as f32 * std::f32::consts::TAU / count as f32,
                ));
            }
            commands
                .spawn((
                    TransitVisual { target: false, id },
                    Transform::from_xyz(position.x, position.y, 40.0),
                    Visibility::default(),
                ))
                .with_children(|children| {
                    for (mesh, material, angle) in parts {
                        children.spawn((
                            Mesh2d(mesh),
                            MeshMaterial2d(material),
                            Transform::from_rotation(Quat::from_rotation_z(angle)),
                        ));
                    }
                    children.spawn((
                        Text2d::new(active.ball.weight.to_string()),
                        TextFont {
                            font_size: bevy::text::FontSize::Px((14.0 * scale).max(20.0)),
                            ..default()
                        },
                        TextLayout::justify(Justify::Center),
                        TextColor(Color::BLACK),
                        Transform::from_xyz(0.0, 0.0, 1.0),
                    ));
                });
            commands.spawn((
                TransitVisual { target: true, id },
                Mesh2d(meshes.add(Annulus::new(radius + 4.0, radius + 8.0))),
                MeshMaterial2d(materials.add(Color::srgb(0.95, 0.75, 0.3))),
                Transform::from_xyz(
                    lane_x(active.destination),
                    landing_y(&field.board, active.destination) - field.offset as f32 * 90.0,
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
                        landing_y(&field.board, active.destination) - field.offset as f32 * 90.0,
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
    fn input_app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin))
            .insert_resource(
                Playfield::new(NonZeroU32::new(4).unwrap_or(NonZeroU32::MIN), Random(7))
                    .unwrap_or_else(|e| panic!("fixture: {e:?}")),
            )
            .init_resource::<Motion>()
            .init_resource::<Clearing>()
            .add_systems(Update, (pan, input, animate).chain());
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
        assert_eq!(platform_y(&board, 0), -400.0);
        assert_eq!(platform_y(&board, 1), -400.0);
        board.place(
            0,
            Ball {
                weight: 2,
                colors: BallColors::single(ColorId::RED),
            },
        );
        assert_eq!(platform_y(&board, 0), -490.0);
        assert_eq!(platform_y(&board, 1), -310.0);
        for lane in 0..LANE_COUNT {
            assert_eq!(
                platform_y(&board, lane) + board.capacity(lane).unwrap_or(0) as f32 * 90.0,
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
            weight,
            colors: BallColors::single(ColorId::RED),
        }
    }
    fn mass(field: &Playfield, motion: &Motion) -> u64 {
        (0..LANE_COUNT)
            .map(|lane| field.board.total(lane))
            .sum::<u64>()
            + motion
                .active
                .as_ref()
                .map_or(0, |t| u64::from(t.ball.weight))
    }
    #[test]
    fn two_launch_chain_is_serial_preserves_balls_and_refills_only_once() {
        let (mut app, _) = input_app();
        {
            let mut field = app.world_mut().resource_mut::<Playfield>();
            field.config = GeneratorConfig::new(NonZeroU32::new(5).unwrap_or(NonZeroU32::MIN));
            field.config.joker_percent = 0;
            field.supply =
                Supply::new(field.config, &mut || 101).unwrap_or_else(|e| panic!("fixture {e:?}"));
            field.board.place(0, ball(3));
            field.board.place(3, ball(1));
            field.chute.shift(1);
        }
        key(&mut app, KeyCode::Space);
        let supply = app.world().resource::<Playfield>().supply.clone();
        let random = app.world().resource::<Playfield>().random.0;
        key(&mut app, KeyCode::Enter);
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
            path: Path::Drop,
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
        *app.world_mut().resource_mut::<Playfield>() =
            Playfield::new(NonZeroU32::MIN, Random(31)).unwrap_or_else(|e| panic!("fixture {e:?}"));
        app
    }
    #[test]
    fn actual_keypresses_clear_four_and_continue_into_level_two() {
        let mut app = level_one_app();
        for group in 0..3 {
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
        assert_eq!(field.supply.held(), &ball(1));
        for lane in 0..LANE_COUNT {
            assert_eq!(field.supply.previews(lane), Some(&[ball(1), ball(1)]));
        }
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
        assert_eq!(field.supply.previews(0).map(|p| &p[1]), Some(&expected));
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
    fn queued_flights_block_drops_and_reset_discards_every_owned_transit() {
        let (mut app, _) = input_app();
        let supply = app.world().resource::<Playfield>().supply.clone();
        app.world_mut()
            .resource_mut::<Motion>()
            .queued
            .push_back(Transit {
                ball: ball(2),
                destination: 0,
                path: Path::Drop,
                elapsed: 0.0,
                duration: 0.5,
            });
        key(&mut app, KeyCode::Space);
        assert_eq!(app.world().resource::<Playfield>().supply, supply);
        key(&mut app, KeyCode::Backspace);
        assert!(app.world().resource::<Motion>().queued.is_empty());
        assert!(app.world().resource::<Motion>().active.is_none());
    }
    #[test]
    fn puzzle_demo_compresses_then_clears_joker_bridge_for_six_hundred() {
        let (mut app, _) = input_app();
        *app.world_mut().resource_mut::<Playfield>() =
            puzzle_demo().unwrap_or_else(|e| panic!("fixture {e:?}"));
        assert_eq!(app.world().resource::<Playfield>().supply.held().weight, 2);
        key(&mut app, KeyCode::Space);
        step(&mut app, 0.17);
        assert_eq!(app.world().resource::<Playfield>().progression.score(), 0);
        assert!(
            app.world()
                .resource::<Playfield>()
                .feedback
                .as_ref()
                .is_some_and(|text| text.contains("5->1 x1"))
        );
        step(&mut app, 0.46);
        let field = app.world().resource::<Playfield>();
        assert_eq!(field.progression.score(), 600);
        assert_eq!(
            field.board.stack(6),
            Some(
                [Ball {
                    weight: 6,
                    colors: BallColors::single(ColorId::YELLOW)
                }]
                .as_slice()
            )
        );
        assert!((0..6).all(|lane| field.board.total(lane) == 0));
        assert!(
            field
                .feedback
                .as_ref()
                .is_some_and(|text| text.contains("Cleared 6 | +600"))
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
            path: Path::Drop,
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
                            weight,
                            colors: BallColors::single(ColorId::new(color)),
                        },
                    );
                }
                for (weight, color) in [(1, 0), (5, id + 1)] {
                    field.board.place(
                        left + 1,
                        Ball {
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
                    weight: 1,
                    colors: BallColors::single(ColorId::new(99)),
                },
                destination: 7,
                path: Path::Drop,
                elapsed: 0.0,
                duration: 0.5,
            });
            motion.queued.push_back(Transit {
                ball: ball(42),
                destination: 6,
                path: Path::Drop,
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
                    weight: 1,
                    colors: BallColors::single(ColorId::new(99)),
                },
                destination: 7,
                path: Path::Drop,
                elapsed: 0.0,
                duration: 1.0 / 6.0,
            });
            motion.drop(Transit {
                ball: Ball {
                    weight: 1,
                    colors: BallColors::single(ColorId::BLUE),
                },
                destination: 0,
                path: Path::Drop,
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
}
