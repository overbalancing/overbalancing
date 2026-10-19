//! Interactive fixed-stack placement sandbox shared by desktop and mobile.
use bevy::{prelude::*, window::PrimaryWindow};
use overpalancing_core::{
    Balance, Ball, BallColor, BallColors, Board, Dispenser, LANE_COUNT, SEESAW_COUNT,
};

const WIDTH: f32 = 920.0;
const HEIGHT: f32 = 1740.0;
const LEFT: f32 = -400.0;
const LANE: f32 = 100.0;
const INK: Color = Color::srgb(0.85, 0.91, 0.98);
#[derive(Resource)]
struct Sandbox {
    board: Board,
    dispenser: Dispenser,
    ball: Ball,
    offset: usize,
}
impl Default for Sandbox {
    fn default() -> Self {
        Self {
            board: Board::default(),
            dispenser: Dispenser::default(),
            ball: Ball {
                weight: 1,
                colors: BallColors::single(BallColor::Red),
            },
            offset: 0,
        }
    }
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

/// Start the application with shared keyboard, pointer and touch controls.
pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    let portrait = args.iter().any(|arg| arg == "--portrait");
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
        .init_resource::<Sandbox>()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Overpalancing - placement sandbox".into(),
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
        .add_systems(Update, (input, fit_camera, redraw, smoke_test).chain())
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
fn color(ball: BallColor) -> Color {
    match ball {
        BallColor::Red => Color::srgb(0.95, 0.34, 0.38),
        BallColor::Blue => Color::srgb(0.22, 0.73, 0.95),
        BallColor::Yellow => Color::srgb(0.97, 0.77, 0.3),
    }
}
fn name(ball: BallColor) -> &'static str {
    match ball {
        BallColor::Red => "R",
        BallColor::Blue => "B",
        BallColor::Yellow => "Y",
    }
}
fn names(colors: BallColors) -> String {
    colors.iter().map(name).collect::<String>()
}
fn redraw(
    mut commands: Commands,
    sandbox: Res<Sandbox>,
    window: Single<&Window, With<PrimaryWindow>>,
    scene: Query<Entity, With<Scene>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut last_size: Local<Vec2>,
) {
    let size = Vec2::new(window.width(), window.height());
    if !sandbox.is_changed() && *last_size == size {
        return;
    }
    *last_size = size;
    let Some(scale) = view_scale(size.x, size.y) else {
        return;
    };
    for entity in &scene {
        commands.entity(entity).despawn();
    }
    label(&mut commands, "OVERPALANCING", 0.0, 600.0, 34.0, scale, INK);
    label(
        &mut commands,
        "PLACEMENT SANDBOX",
        0.0,
        530.0,
        22.0,
        scale,
        INK,
    );
    label(
        &mut commands,
        "Flights / matching pending",
        0.0,
        460.0,
        20.0,
        scale,
        INK,
    );
    let selected = sandbox.dispenser.lane();
    label(
        &mut commands,
        format!(
            "Seesaw {} | {}\nColors {} | Weight {}",
            selected / 2 + 1,
            if selected.is_multiple_of(2) {
                "Left"
            } else {
                "Right"
            },
            names(sandbox.ball.colors),
            sandbox.ball.weight
        ),
        0.0,
        390.0,
        20.0,
        scale,
        INK,
    );
    let x = LEFT + (selected as f32 + 0.5) * LANE;
    rect(
        &mut commands,
        x,
        275.0,
        80.0,
        35.0,
        Color::srgb(0.95, 0.75, 0.3),
        0.0,
    );
    rect(
        &mut commands,
        x,
        250.0,
        22.0,
        20.0,
        Color::srgb(0.95, 0.75, 0.3),
        0.0,
    );
    label(
        &mut commands,
        "Select 1-3 colors; last stays on",
        0.0,
        215.0,
        18.0,
        scale,
        INK,
    );
    for seesaw in 0..SEESAW_COUNT {
        let center = -300.0 + seesaw as f32 * 200.0;
        let angle = match sandbox.board.balance(seesaw) {
            Some(Balance::LeftHeavy) => 0.08,
            Some(Balance::RightHeavy) => -0.08,
            _ => 0.0,
        };
        rect(&mut commands, center, -90.0, 180.0, 12.0, INK, angle);
        label(
            &mut commands,
            match sandbox.board.balance(seesaw) {
                Some(Balance::LeftHeavy) => "L down",
                Some(Balance::RightHeavy) => "R down",
                _ => "Equal",
            },
            center,
            -205.0,
            18.0,
            scale,
            INK,
        );
        commands.spawn((
            Scene,
            Mesh2d(meshes.add(Triangle2d::new(
                Vec2::new(0.0, 20.0),
                Vec2::new(-20.0, -20.0),
                Vec2::new(20.0, -20.0),
            ))),
            MeshMaterial2d(materials.add(Color::srgb(0.3, 0.45, 0.6))),
            Transform::from_xyz(center, -115.0, 0.0),
        ));
    }
    let radius = (20.0 * scale).clamp(22.0, 40.0);
    for lane in 0..LANE_COUNT {
        let x = LEFT + (lane as f32 + 0.5) * LANE;
        let stack = sandbox.board.stack(lane).unwrap_or(&[]);
        label(
            &mut commands,
            format!(
                "{} {}",
                if lane.is_multiple_of(2) { "L" } else { "R" },
                sandbox.board.total(lane)
            ),
            x,
            -170.0,
            24.0,
            scale,
            if lane == selected {
                Color::srgb(0.95, 0.75, 0.3)
            } else {
                INK
            },
        );
        for (index, ball) in stack.iter().skip(sandbox.offset).take(3).enumerate() {
            let y = -35.0 + index as f32 * 90.0;
            let count = ball.colors.count();
            for (sector, c) in ball.colors.iter().enumerate() {
                if count == 1 {
                    commands.spawn((
                        Scene,
                        Mesh2d(meshes.add(Circle::new(radius))),
                        MeshMaterial2d(materials.add(color(c))),
                        Transform::from_xyz(x, y, 2.0),
                    ));
                } else {
                    commands.spawn((
                        Scene,
                        Mesh2d(meshes.add(CircularSector::new(
                            radius,
                            std::f32::consts::PI / count as f32,
                        ))),
                        MeshMaterial2d(materials.add(color(c))),
                        Transform::from_xyz(x, y, 2.0).with_rotation(Quat::from_rotation_z(
                            sector as f32 * std::f32::consts::TAU / count as f32,
                        )),
                    ));
                }
            }
            label(
                &mut commands,
                format!("{}\n{}", names(ball.colors), ball.weight),
                x,
                y,
                20.0,
                scale,
                Color::BLACK,
            );
        }
    }
    let max = sandbox.board.stack(selected).map_or(0, <[Ball]>::len);
    button(&mut commands, "Lower", -290.0, -280.0, 230.0, false, scale);
    label(
        &mut commands,
        if sandbox.offset >= max {
            format!("Empty / {max}")
        } else {
            format!(
                "{}-{} / {}",
                sandbox.offset + 1,
                (sandbox.offset + 3).min(max),
                max
            )
        },
        0.0,
        -280.0,
        20.0,
        scale,
        INK,
    );
    button(&mut commands, "Higher", 290.0, -280.0, 230.0, false, scale);
    for index in 0..3 {
        let x = -280.0 + index as f32 * 280.0;
        button(
            &mut commands,
            format!("Weight {}", index + 1),
            x,
            -420.0,
            260.0,
            sandbox.ball.weight == index + 1,
            scale,
        );
        let c = [BallColor::Red, BallColor::Blue, BallColor::Yellow][index as usize];
        button(
            &mut commands,
            ["Red", "Blue", "Yellow"][index as usize],
            x,
            -550.0,
            260.0,
            sandbox.ball.colors.contains(c),
            scale,
        );
    }
    label(
        &mut commands,
        "Left/Right: lane | 1-3: weight\nR B Y: color | Space: place\nBackspace: reset | Up/Down: rows",
        0.0,
        -790.0,
        18.0,
        scale,
        INK,
    );
    button(&mut commands, "PLACE", -210.0, -680.0, 390.0, false, scale);
    button(&mut commands, "RESET", 210.0, -680.0, 390.0, false, scale);
}
fn input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut sandbox: ResMut<Sandbox>,
) {
    if keys.just_pressed(KeyCode::ArrowLeft) || keys.just_pressed(KeyCode::KeyA) {
        sandbox.dispenser.shift(-1);
    }
    if keys.just_pressed(KeyCode::ArrowRight) || keys.just_pressed(KeyCode::KeyD) {
        sandbox.dispenser.shift(1);
    }
    for (key, weight) in [
        (KeyCode::Digit1, 1),
        (KeyCode::Digit2, 2),
        (KeyCode::Digit3, 3),
    ] {
        if keys.just_pressed(key) {
            sandbox.ball.weight = weight;
        }
    }
    for (key, c) in [
        (KeyCode::KeyR, BallColor::Red),
        (KeyCode::KeyB, BallColor::Blue),
        (KeyCode::KeyY, BallColor::Yellow),
    ] {
        if keys.just_pressed(key) {
            sandbox.ball.colors.toggle(c);
        }
    }
    if keys.just_pressed(KeyCode::Space) || keys.just_pressed(KeyCode::Enter) {
        place(&mut sandbox);
    }
    if keys.just_pressed(KeyCode::Backspace) {
        sandbox.board.clear();
        sandbox.offset = 0;
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        sandbox.offset = sandbox.offset.saturating_add(1);
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        sandbox.offset = sandbox.offset.saturating_sub(1);
    }
    // Prefer touch over synthesized mouse events, ensuring exactly one action.
    let pointer = touches
        .iter_just_pressed()
        .next()
        .map(|t| t.position())
        .or_else(|| {
            mouse
                .just_pressed(MouseButton::Left)
                .then(|| window.cursor_position())
                .flatten()
        });
    if let (Some(position), Some(scale)) = (pointer, view_scale(window.width(), window.height())) {
        let point = Vec2::new(
            (position.x - window.width() / 2.0) * scale,
            (window.height() / 2.0 - position.y) * scale,
        );
        if (-400.0..=400.0).contains(&point.x) {
            if (-220.0..=300.0).contains(&point.y) {
                sandbox
                    .dispenser
                    .select_fraction((point.x - LEFT) / (LANE * LANE_COUNT as f32));
            } else if (point.y + 280.0).abs() <= 55.0 {
                if point.x < -170.0 {
                    sandbox.offset = sandbox.offset.saturating_sub(1);
                } else if point.x > 170.0 {
                    sandbox.offset = sandbox.offset.saturating_add(1);
                }
            } else if (point.y + 420.0).abs() <= 55.0 {
                sandbox.ball.weight =
                    ((point.x + 420.0) / 280.0).floor().clamp(0.0, 2.0) as u32 + 1;
            } else if (point.y + 550.0).abs() <= 55.0 {
                sandbox.ball.colors.toggle(
                    [BallColor::Red, BallColor::Blue, BallColor::Yellow]
                        [((point.x + 420.0) / 280.0).floor().clamp(0.0, 2.0) as usize],
                );
            } else if (point.y + 680.0).abs() <= 55.0 {
                if point.x < 0.0 {
                    place(&mut sandbox);
                } else {
                    sandbox.board.clear();
                    sandbox.offset = 0;
                }
            }
        }
    }
    let max = (0..LANE_COUNT)
        .filter_map(|lane| sandbox.board.stack(lane).map(<[Ball]>::len))
        .max()
        .unwrap_or(0)
        .saturating_sub(1);
    if sandbox.offset > max {
        sandbox.offset = max;
    }
}
fn place(sandbox: &mut Sandbox) {
    sandbox.board.place(sandbox.dispenser.lane(), sandbox.ball);
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

    fn app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin))
            .init_resource::<Sandbox>()
            .add_systems(Update, input);
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
    fn tap(app: &mut App, window: Entity, x: f32, y: f32, id: u64) {
        let scale = view_scale(390.0, 844.0).unwrap_or(1.0);
        app.world_mut().write_message(TouchInput {
            phase: TouchPhase::Started,
            position: Vec2::new(195.0 + x / scale, 422.0 - y / scale),
            window,
            force: None,
            id,
        });
        app.update();
    }
    #[test]
    fn keyboard_configures_places_inspects_and_resets() {
        let (mut app, _) = app();
        key(&mut app, KeyCode::ArrowRight);
        key(&mut app, KeyCode::Digit3);
        key(&mut app, KeyCode::KeyB);
        key(&mut app, KeyCode::Space);
        let sandbox = app.world().resource::<Sandbox>();
        assert_eq!(sandbox.board.total(1), 3);
        assert_eq!(sandbox.board.balance(0), Some(Balance::RightHeavy));
        assert_eq!(
            sandbox
                .board
                .stack(1)
                .and_then(|s| s.first())
                .map(|b| names(b.colors)),
            Some("RB".into())
        );
        key(&mut app, KeyCode::KeyY);
        key(&mut app, KeyCode::Digit2);
        key(&mut app, KeyCode::Space);
        let balls = app
            .world()
            .resource::<Sandbox>()
            .board
            .stack(1)
            .unwrap_or(&[]);
        assert_eq!(names(balls[0].colors), "RB");
        assert_eq!(balls[0].weight, 3);
        assert_eq!(names(balls[1].colors), "RBY");
        assert_eq!(balls[1].weight, 2);
        for _ in 0..4 {
            key(&mut app, KeyCode::Enter);
        }
        key(&mut app, KeyCode::ArrowUp);
        assert_eq!(app.world().resource::<Sandbox>().offset, 1);
        assert_eq!(
            app.world()
                .resource::<Sandbox>()
                .board
                .stack(1)
                .map(<[Ball]>::len),
            Some(6)
        );
        key(&mut app, KeyCode::Backspace);
        assert_eq!(app.world().resource::<Sandbox>().board.total(1), 0);
        assert_eq!(app.world().resource::<Sandbox>().ball.weight, 2);
    }
    #[test]
    fn actual_portrait_touch_configures_places_once_and_resets() {
        let (mut app, window) = app();
        tap(&mut app, window, -280.0, -550.0, 10);
        assert_eq!(names(app.world().resource::<Sandbox>().ball.colors), "R");
        tap(&mut app, window, 350.0, 0.0, 1);
        tap(&mut app, window, 0.0, -420.0, 2);
        tap(&mut app, window, 280.0, -550.0, 3);
        // A synthesized mouse press in the same frame must not duplicate touch.
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        tap(&mut app, window, -210.0, -680.0, 4);
        let sandbox = app.world().resource::<Sandbox>();
        assert_eq!(sandbox.dispenser.lane(), 7);
        assert_eq!(sandbox.board.total(7), 2);
        assert_eq!(sandbox.board.stack(7).map(<[Ball]>::len), Some(1));
        assert_eq!(names(sandbox.ball.colors), "RY");
        tap(&mut app, window, 0.0, -550.0, 11);
        tap(&mut app, window, -210.0, -680.0, 12);
        let balls = app
            .world()
            .resource::<Sandbox>()
            .board
            .stack(7)
            .unwrap_or(&[]);
        assert_eq!(names(balls[0].colors), "RY");
        assert_eq!(names(balls[1].colors), "RBY");
        tap(&mut app, window, 210.0, -680.0, 5);
        assert_eq!(app.world().resource::<Sandbox>().board.total(7), 0);
    }
}
