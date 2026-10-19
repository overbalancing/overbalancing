//! Primitive first-view prototype shared by desktop and mobile.
use bevy::{prelude::*, window::PrimaryWindow};
use overpalancing_core::{Dispenser, LANE_COUNT, SEESAW_COUNT};

const WIDTH: f32 = 920.0;
const HEIGHT: f32 = 760.0;
const BOARD_LEFT: f32 = -400.0;
const LANE_WIDTH: f32 = 100.0;

#[derive(Resource, Default)]
struct Selection(Dispenser);
#[derive(Component)]
struct DispenserVisual;
#[derive(Component)]
struct SceneCamera;
#[derive(Component)]
struct AdaptiveText {
    base: f32,
    minimum: f32,
}
#[derive(Component)]
struct AdaptiveBall {
    stack: usize,
}
#[derive(Resource, Default)]
struct Smoke {
    enabled: bool,
    frame: u32,
    screenshot: Option<String>,
}

/// Start the shared platform application.
pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    let screenshot = args
        .windows(2)
        .find(|pair| pair[0] == "--screenshot")
        .map(|pair| pair[1].clone());
    let portrait = args.iter().any(|arg| arg == "--portrait");
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.035, 0.055, 0.09)))
        .insert_resource(Smoke {
            enabled: args.iter().any(|arg| arg == "--smoke-test"),
            screenshot,
            ..default()
        })
        .init_resource::<Selection>()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Overpalancing — first view".into(),
                resolution: if portrait {
                    (540, 960).into()
                } else {
                    (1100, 800).into()
                },
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                fit_camera,
                fit_balls,
                fit_text,
                input,
                move_dispenser,
                smoke_test,
            )
                .chain(),
        )
        .run();
}

fn label(commands: &mut Commands, text: &str, x: f32, y: f32, size: f32, color: Color) -> Entity {
    commands
        .spawn((
            Text2d::new(text),
            TextFont {
                font_size: bevy::text::FontSize::Px(size),
                ..default()
            },
            TextColor(color),
            Transform::from_xyz(x, y, 10.0),
            AdaptiveText {
                base: size,
                minimum: if size <= 11.0 {
                    12.0
                } else if size >= 30.0 {
                    22.0
                } else {
                    14.0
                },
            },
        ))
        .id()
}
fn rect(
    commands: &mut Commands,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: Color,
    angle: f32,
) {
    commands.spawn((
        Sprite::from_color(color, Vec2::new(width, height)),
        Transform::from_xyz(x, y, 1.0).with_rotation(Quat::from_rotation_z(angle)),
    ));
}
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.spawn((Camera2d, SceneCamera));
    let ink = Color::srgb(0.85, 0.91, 0.98);
    label(&mut commands, "OVERPALANCING", 0.0, 282.0, 38.0, ink);
    label(
        &mut commands,
        "FIRST VIEW  /  PLAYFIELD PROTOTYPE",
        0.0,
        242.0,
        16.0,
        Color::srgb(0.5, 0.65, 0.8),
    );
    rect(
        &mut commands,
        0.0,
        140.0,
        820.0,
        3.0,
        Color::srgb(0.2, 0.31, 0.44),
        0.0,
    );
    commands.spawn((
        Sprite::from_color(Color::srgb(0.94, 0.73, 0.3), Vec2::new(66.0, 35.0)),
        Transform::from_xyz(-350.0, 164.0, 2.0),
        DispenserVisual,
    ));
    commands.spawn((
        Text2d::new("CHUTE"),
        TextFont {
            font_size: bevy::text::FontSize::Px(15.0),
            ..default()
        },
        TextColor(Color::BLACK),
        AdaptiveText {
            base: 15.0,
            minimum: 12.0,
        },
        Transform::from_xyz(-350.0, 164.0, 10.0),
        DispenserVisual,
    ));
    commands.spawn((
        Sprite::from_color(Color::srgb(0.94, 0.73, 0.3), Vec2::new(22.0, 16.0)),
        Transform::from_xyz(-350.0, 143.0, 2.0),
        DispenserVisual,
    ));
    for i in 0..SEESAW_COUNT {
        let x = -300.0 + i as f32 * 200.0;
        let angle = [-0.055, 0.04, -0.025, 0.065][i];
        rect(
            &mut commands,
            x,
            -135.0,
            174.0,
            12.0,
            Color::srgb(0.59, 0.72, 0.86),
            angle,
        );
        commands.spawn((
            Mesh2d(meshes.add(Triangle2d::new(
                Vec2::new(0.0, 21.0),
                Vec2::new(-22.0, -21.0),
                Vec2::new(22.0, -21.0),
            ))),
            MeshMaterial2d(materials.add(Color::srgb(0.28, 0.4, 0.54))),
            Transform::from_xyz(x, -163.0, 0.0),
        ));
        label(&mut commands, &format!("0{}", i + 1), x, -217.0, 20.0, ink);
    }
    let colors = [
        Color::srgb(0.93, 0.29, 0.35),
        Color::srgb(0.2, 0.73, 0.92),
        Color::srgb(0.97, 0.73, 0.25),
    ];
    for lane in 0..LANE_COUNT {
        let x = BOARD_LEFT + (lane as f32 + 0.5) * LANE_WIDTH;
        let count = [2, 1, 3, 2, 1, 2, 2, 3][lane];
        for stack in 0..count {
            let y = -108.0 + stack as f32 * 43.0;
            let color_count = if lane == 5 && stack == 1 {
                3
            } else if lane == 2 && stack == 2 {
                2
            } else {
                1
            };
            commands.spawn((
                Mesh2d(meshes.add(Circle::new(22.0))),
                MeshMaterial2d(materials.add(Color::srgb(0.86, 0.91, 0.97))),
                Transform::from_xyz(x, y, 2.0),
                AdaptiveBall { stack },
            ));
            if color_count == 1 {
                commands.spawn((
                    Mesh2d(meshes.add(Circle::new(20.0))),
                    MeshMaterial2d(materials.add(colors[(lane + stack) % 3])),
                    Transform::from_xyz(x, y, 3.0),
                    AdaptiveBall { stack },
                ));
            } else {
                for (sector, color) in colors.iter().enumerate().take(color_count) {
                    commands.spawn((
                        Mesh2d(meshes.add(CircularSector::new(
                            20.0,
                            std::f32::consts::PI / color_count as f32,
                        ))),
                        MeshMaterial2d(materials.add(*color)),
                        Transform::from_xyz(x, y, 3.0).with_rotation(Quat::from_rotation_z(
                            sector as f32 * std::f32::consts::TAU / color_count as f32,
                        )),
                        AdaptiveBall { stack },
                    ));
                }
            }
            let entity = label(
                &mut commands,
                &format!(
                    "{}\n{}",
                    if color_count == 3 {
                        "RBY"
                    } else if color_count == 2 {
                        "RB"
                    } else {
                        ["R", "B", "Y"][(lane + stack) % 3]
                    },
                    (lane * 2 + stack) % 3 + 1
                ),
                x,
                y,
                11.0,
                Color::srgb(0.03, 0.05, 0.08),
            );
            commands
                .entity(entity)
                .insert((AdaptiveBall { stack }, TextLayout::justify(Justify::Center)));
        }
    }
    label(
        &mut commands,
        "ARROWS / A D   OR   TAP A LANE",
        0.0,
        -272.0,
        19.0,
        ink,
    );
    label(
        &mut commands,
        "R/B/Y = colors | Numbers = weights\nSplit colors = joker",
        0.0,
        -340.0,
        16.0,
        Color::srgb(0.5, 0.65, 0.8),
    );
}
fn fit_camera(
    window: Single<&Window, With<PrimaryWindow>>,
    mut camera: Single<&mut Projection, With<SceneCamera>>,
) {
    if let Projection::Orthographic(projection) = &mut **camera
        && let Some(scale) = view_scale(window.width(), window.height())
    {
        projection.scale = scale;
    }
}
fn input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut selection: ResMut<Selection>,
) {
    if keys.just_pressed(KeyCode::ArrowLeft) || keys.just_pressed(KeyCode::KeyA) {
        selection.0.shift(-1);
    }
    if keys.just_pressed(KeyCode::ArrowRight) || keys.just_pressed(KeyCode::KeyD) {
        selection.0.shift(1);
    }
    let pointer = touches
        .iter_just_pressed()
        .next()
        .map(|touch| touch.position())
        .or_else(|| {
            mouse
                .just_pressed(MouseButton::Left)
                .then(|| window.cursor_position())
                .flatten()
        });
    if let Some(position) = pointer {
        let Some(scale) = view_scale(window.width(), window.height()) else {
            return;
        };
        let world_x = (position.x - window.width() / 2.0) * scale;
        let world_y = (window.height() / 2.0 - position.y) * scale;
        if (BOARD_LEFT..=-BOARD_LEFT).contains(&world_x) && (-235.0..=200.0).contains(&world_y) {
            selection
                .0
                .select_fraction((world_x - BOARD_LEFT) / (LANE_WIDTH * LANE_COUNT as f32));
        }
    }
}
fn move_dispenser(
    selection: Res<Selection>,
    mut transforms: Query<&mut Transform, With<DispenserVisual>>,
) {
    for mut transform in &mut transforms {
        transform.translation.x = BOARD_LEFT + (selection.0.lane() as f32 + 0.5) * LANE_WIDTH;
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

/// Mobile entry point generated by Bevy.
#[cfg(any(target_os = "android", target_os = "ios"))]
#[bevy::prelude::bevy_main]
fn main() {
    run();
}

fn view_scale(width: f32, height: f32) -> Option<f32> {
    (width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0)
        .then(|| (WIDTH / width).max(HEIGHT / height))
}

// Keep balls and their two-line non-color cues legible in narrow portrait views.
fn ball_scale(camera_scale: f32) -> f32 {
    camera_scale.clamp(1.0, 2.0)
}
fn fit_balls(
    window: Single<&Window, With<PrimaryWindow>>,
    mut balls: Query<(&AdaptiveBall, &mut Transform)>,
) {
    let Some(scale) = view_scale(window.width(), window.height()) else {
        return;
    };
    let factor = ball_scale(scale);
    for (ball, mut transform) in &mut balls {
        transform.scale = Vec3::splat(factor);
        transform.translation.y = -130.0 + (22.0 + ball.stack as f32 * 45.0) * factor;
    }
}

fn fit_text(
    window: Single<&Window, With<PrimaryWindow>>,
    mut texts: Query<(&AdaptiveText, &mut Transform)>,
) {
    let Some(scale) = view_scale(window.width(), window.height()) else {
        return;
    };
    for (text, mut transform) in &mut texts {
        transform.scale = Vec3::splat((text.base / scale).max(text.minimum) * scale / text.base);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input_app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin))
            .init_resource::<Selection>()
            .add_systems(Update, input);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution: (540, 960).into(),
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        (app, window)
    }

    #[test]
    fn real_input_system_routes_portrait_touch_and_ignores_outside_board() {
        use bevy::input::touch::{TouchInput, TouchPhase};
        let (mut app, window) = input_app();
        // At 540 px width, the board spans x=35..505. Lane 6 center is x=417.
        app.world_mut().write_message(TouchInput {
            phase: TouchPhase::Started,
            position: Vec2::new(417.0, 480.0),
            window,
            force: None,
            id: 1,
        });
        app.update();
        assert_eq!(app.world().resource::<Selection>().0.lane(), 6);
        // A new touch above the playfield must not alter the selected lane.
        app.world_mut().write_message(TouchInput {
            phase: TouchPhase::Started,
            position: Vec2::new(50.0, 10.0),
            window,
            force: None,
            id: 2,
        });
        app.update();
        assert_eq!(app.world().resource::<Selection>().0.lane(), 6);
    }

    #[test]
    fn real_input_system_keyboard_selection_stops_at_both_edges() {
        let (mut app, _) = input_app();
        // Execute the actual Update system after supplying the keyboard resource;
        // a platform keyboard plugin normally supplies this resource before Update.
        for _ in 0..12 {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::ArrowRight);
            app.world_mut().run_schedule(Update);
        }
        assert_eq!(app.world().resource::<Selection>().0.lane(), 7);
        for _ in 0..12 {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::KeyA);
            app.world_mut().run_schedule(Update);
        }
        assert_eq!(app.world().resource::<Selection>().0.lane(), 0);
    }

    #[test]
    fn resized_board_remains_fully_visible() {
        for (width, height) in [(1100.0, 800.0), (540.0, 960.0), (1600.0, 400.0)] {
            let Some(scale) = view_scale(width, height) else {
                panic!("valid viewport must have a scale");
            };
            assert!(width * scale >= WIDTH - 0.001);
            assert!(height * scale >= HEIGHT - 0.001);
            for lane in 0..LANE_COUNT {
                let world_x = BOARD_LEFT + (lane as f32 + 0.5) * LANE_WIDTH;
                let pointer_x = world_x / scale + width / 2.0;
                let recovered_x = (pointer_x - width / 2.0) * scale;
                let mut dispenser = Dispenser::default();
                dispenser
                    .select_fraction((recovered_x - BOARD_LEFT) / (LANE_WIDTH * LANE_COUNT as f32));
                assert_eq!(dispenser.lane(), lane);
            }
        }
    }
    #[test]
    fn portrait_ball_cues_keep_minimum_size_without_stack_overlap() {
        let Some(scale) = view_scale(540.0, 960.0) else {
            panic!("valid viewport");
        };
        let factor = ball_scale(scale);
        assert!(44.0 * factor / scale >= 40.0);
        assert!(44.0 * factor <= 45.0 * factor);
        assert!(44.0 * factor < LANE_WIDTH);
        let Some(narrow_scale) = view_scale(390.0, 844.0) else {
            panic!("valid viewport");
        };
        assert!(44.0 * ball_scale(narrow_scale) / narrow_scale >= 32.0);
        assert!((11.0 / narrow_scale).max(12.0) >= 12.0);
        assert!(
            -130.0
                + (22.0 + 2.0 * 45.0) * ball_scale(narrow_scale)
                + 22.0 * ball_scale(narrow_scale)
                < 140.0
        );
    }
    #[test]
    fn minimized_or_invalid_window_has_no_coordinate_mapping() {
        for (width, height) in [
            (0.0, 800.0),
            (600.0, 0.0),
            (f32::NAN, 500.0),
            (500.0, f32::INFINITY),
        ] {
            assert!(view_scale(width, height).is_none());
        }
    }
}
