//! Level-driven supply and fixed-stack board shared by desktop and mobile.
use bevy::{prelude::*, window::PrimaryWindow};
use overpalancing_core::supply::{GenerationError, GeneratorConfig, Supply};
use overpalancing_core::{
    Balance, Ball, BallColors, Board, ColorId, Dispenser, LANE_COUNT, SEESAW_COUNT,
};
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
const INK: Color = Color::srgb(0.85, 0.91, 0.98);

#[derive(Resource)]
struct Playfield {
    board: Board,
    chute: Dispenser,
    supply: Supply,
    offset: usize,
    config: GeneratorConfig,
    random: Random,
    game_over: bool,
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
            random,
            game_over: false,
            error: None,
        })
    }
    fn drop_ball(&mut self) -> Result<(), &'static str> {
        if self.game_over {
            return Ok(());
        }
        let source = self.chute.lane();
        let mut random = self.random;
        let next = self
            .config
            .generate(&mut || random.next())
            .map_err(|_| "Unable to generate a ball")?;
        let mut supply = self.supply.clone();
        let held = supply
            .take_and_refill(source, next)
            .map_err(|_| "Unable to refill this lane")?;
        let mut board = self.board.clone();
        if !board.place(source, held) {
            return Err("Unable to place this ball");
        }
        self.game_over = board.is_game_over();
        self.board = board;
        self.supply = supply;
        self.random = random;
        self.error = None;
        Ok(())
    }
    fn reset(&mut self) {
        if let Ok(supply) = Supply::new(self.config, &mut || self.random.next()) {
            self.supply = supply;
            self.board.clear();
            self.offset = 0;
            self.chute = Dispenser::default();
            self.game_over = false;
            self.error = None;
        }
    }
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
    let field = match Playfield::new(level, Random::seeded()) {
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
        .add_systems(Update, (pan, input, fit_camera, redraw, smoke_test).chain())
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
fn button(
    commands: &mut Commands,
    text: impl Into<String>,
    x: f32,
    y: f32,
    width: f32,
    selected: bool,
    scale: f32,
) {
    rect(
        commands,
        x,
        y,
        width,
        110.0,
        if selected {
            Color::srgb(0.25, 0.43, 0.56)
        } else {
            Color::srgb(0.12, 0.2, 0.3)
        },
        0.0,
    );
    label(
        commands,
        if selected {
            format!("[{}]", text.into())
        } else {
            text.into()
        },
        x,
        y,
        24.0,
        scale,
        INK,
    );
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
fn names(colors: &BallColors) -> String {
    colors
        .iter()
        .map(|id| format!("C{}", u64::from(id.index()) + 1))
        .collect::<Vec<_>>()
        .join("+")
}
fn draw_ball(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    ball: &Ball,
    x: f32,
    y: f32,
    scale: f32,
) {
    let radius = (20.0 * scale).clamp(22.0, 40.0);
    let count = ball.colors.count();
    for (sector, id) in ball.colors.iter().enumerate() {
        if count == 1 {
            commands.spawn((
                Scene,
                Mesh2d(meshes.add(Circle::new(radius))),
                MeshMaterial2d(materials.add(color(id))),
                Transform::from_xyz(x, y, 2.0),
            ));
        } else {
            commands.spawn((
                Scene,
                Mesh2d(meshes.add(CircularSector::new(
                    radius,
                    std::f32::consts::PI / count as f32,
                ))),
                MeshMaterial2d(materials.add(color(id))),
                Transform::from_xyz(x, y, 2.0).with_rotation(Quat::from_rotation_z(
                    sector as f32 * std::f32::consts::TAU / count as f32,
                )),
            ));
        }
    }
    let ids = ball
        .colors
        .iter()
        .map(|id| (u64::from(id.index()) + 1).to_string())
        .collect::<Vec<_>>()
        .join("");
    label(
        commands,
        format!("{}\n{}", ids, ball.weight),
        x,
        y,
        20.0,
        scale,
        Color::BLACK,
    );
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
        format!("OVERPALANCING | LEVEL {}", field.config.level),
        0.0,
        1000.0,
        30.0,
        scale,
        INK,
    );
    label(
        &mut commands,
        "Color IDs / weight | 2 previews per lane",
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
                    x,
                    760.0 + index as f32 * 90.0,
                    scale,
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
        label(
            &mut commands,
            format!(
                "{} {}",
                if lane.is_multiple_of(2) { "L" } else { "R" },
                field.board.total(lane)
            ),
            x,
            -570.0,
            20.0,
            scale,
            INK,
        );
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
                x,
                base + 45.0 + index as f32 * 90.0,
                scale,
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
        x,
        570.0,
        scale,
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
    label(
        &mut commands,
        format!(
            "Rows {}-{} | drag to inspect",
            field.offset + 1,
            field.offset + 9
        ),
        0.0,
        410.0,
        18.0,
        scale,
        INK,
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
        label(
            &mut commands,
            match field.board.balance(seesaw) {
                Some(Balance::LeftHeavy) => "L down",
                Some(Balance::RightHeavy) => "R down",
                _ => "Equal",
            },
            middle.x,
            -625.0,
            18.0,
            scale,
            INK,
        );
    }
    let held = field.supply.held();
    label(
        &mut commands,
        format!(
            "Target {}{} | Held {} | Weight {}",
            field.chute.lane() / 2 + 1,
            if field.chute.lane().is_multiple_of(2) {
                "L"
            } else {
                "R"
            },
            names(&held.colors),
            held.weight
        ),
        0.0,
        -700.0,
        20.0,
        scale,
        INK,
    );
    button(
        &mut commands,
        if field.game_over { "STOPPED" } else { "DROP" },
        -210.0,
        -815.0,
        390.0,
        false,
        scale,
    );
    button(&mut commands, "RESET", 210.0, -815.0, 390.0, false, scale);
    label(
        &mut commands,
        "Left/Right or tap: chute | Space: drop\nWheel / drag: inspect | Backspace: reset",
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
            Transform::from_xyz(0.0, -30.0, 20.0),
        ));
        commands.spawn((
            Scene,
            Text2d::new("GAME OVER\nStack capacity exceeded\nReset to play again"),
            TextFont {
                font_size: bevy::text::FontSize::Px((18.0 * scale).max(28.0)),
                ..default()
            },
            TextLayout::justify(Justify::Center),
            TextColor(Color::WHITE),
            Transform::from_xyz(0.0, -30.0, 30.0),
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
    if keys.just_pressed(KeyCode::ArrowDown) {
        rows -= 1;
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
) {
    let touch_activity =
        touches.iter().next().is_some() || touches.iter_just_released().next().is_some();
    *ignore_mouse = if touch_activity {
        2
    } else {
        ignore_mouse.saturating_sub(1)
    };
    let mut drop_requested = keys.just_pressed(KeyCode::Space) || keys.just_pressed(KeyCode::Enter);
    let mut reset_requested = keys.just_pressed(KeyCode::Backspace);
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
        } else if (point.y + 815.0).abs() <= 55.0 && (-400.0..=400.0).contains(&point.x) {
            if point.x < 0.0 {
                drop_requested = true;
            } else {
                reset_requested = true;
            }
        }
    }
    if reset_requested {
        field.reset();
    } else if drop_requested && let Err(error) = field.drop_ball() {
        field.error = Some(error);
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
            .add_systems(Update, (pan, input).chain());
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
        key(&mut app, KeyCode::Space);
        let field = app.world().resource::<Playfield>();
        assert_eq!(field.board.stack(1).and_then(|s| s.first()), Some(&held));
        assert_eq!(field.supply.held(), &source[0]);
        assert_eq!(field.supply.previews(1).map(|p| &p[0]), Some(&source[1]));
        assert_eq!(field.supply.previews(0).cloned(), untouched);
        key(&mut app, KeyCode::Enter);
        let field = app.world().resource::<Playfield>();
        assert_eq!(
            field.board.stack(1).and_then(|s| s.get(1)),
            Some(&source[0])
        );
        assert_eq!(field.supply.held(), &source[1]);
    }
    #[test]
    fn real_touch_drop_ignores_same_and_next_frame_synthesized_mouse() {
        use bevy::input::{ButtonState, mouse::MouseButtonInput};
        let (mut app, window) = input_app();
        let scale = view_scale(390.0, 844.0).unwrap_or(1.0);
        app.world_mut()
            .entity_mut(window)
            .get_mut::<Window>()
            .unwrap_or_else(|| panic!("window"))
            .set_cursor_position(Some(Vec2::new(
                195.0 - 210.0 / scale,
                422.0 + 815.0 / scale,
            )));
        touch(&mut app, window, TouchPhase::Started, -210.0, -815.0, 22);
        app.world_mut().write_message(MouseButtonInput {
            button: MouseButton::Left,
            state: ButtonState::Pressed,
            window,
        });
        touch(&mut app, window, TouchPhase::Ended, -210.0, -815.0, 22);
        assert_eq!(
            app.world()
                .resource::<Playfield>()
                .board
                .stack(0)
                .map(<[Ball]>::len),
            Some(1)
        );
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
        assert_eq!(
            app.world()
                .resource::<Playfield>()
                .board
                .stack(0)
                .map(<[Ball]>::len),
            Some(1)
        );
    }
    #[test]
    fn untouched_raised_stack_ends_game_blocks_supply_and_reset_restores() {
        let (mut app, _) = input_app();
        {
            let mut field = app.world_mut().resource_mut::<Playfield>();
            for lane in 0..2 {
                for _ in 0..8 {
                    field.board.place(
                        lane,
                        Ball {
                            weight: 1,
                            colors: BallColors::single(ColorId::RED),
                        },
                    );
                }
            }
            assert!(!field.board.is_game_over());
        }
        key(&mut app, KeyCode::Space);
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
}
