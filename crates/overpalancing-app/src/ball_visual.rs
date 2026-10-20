//! Original, procedural color cues shared by stationary and moving balls.
//! Six symbols and a stripe bit repeat every twelve IDs; this is a finite cue set.
use bevy::{asset::RenderAssetUsages, mesh::PrimitiveTopology, prelude::*};
use overpalancing_core::{Ball, ColorId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Symbol {
    Triangle,
    Square,
    Diamond,
    Plus,
    Cross,
    Star,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Cue {
    pub symbol: Symbol,
    pub striped: bool,
}
pub(super) fn cue(id: ColorId) -> Cue {
    let symbol = [
        Symbol::Triangle,
        Symbol::Square,
        Symbol::Diamond,
        Symbol::Plus,
        Symbol::Cross,
        Symbol::Star,
    ][(id.index() % 6) as usize];
    Cue {
        symbol,
        striped: (id.index() / 6) % 2 == 1,
    }
}
#[derive(Component, Clone, Debug, Eq, PartialEq)]
pub(super) struct Marking {
    pub id: ColorId,
    pub cue: Cue,
    pub sector: usize,
}
struct Part {
    mesh: Handle<Mesh>,
    material: Handle<ColorMaterial>,
    transform: Transform,
    marking: Option<Marking>,
}
pub(super) struct Visual {
    parts: Vec<Part>,
    weight: String,
    font: f32,
}

fn rotate(point: Vec2, angle: f32) -> Vec2 {
    let (sin, cos) = angle.sin_cos();
    Vec2::new(point.x * cos - point.y * sin, point.x * sin + point.y * cos)
}
fn polygon_mesh(points: &[Vec2]) -> Mesh {
    let center = points.iter().copied().sum::<Vec2>() / points.len() as f32;
    let mut positions = Vec::with_capacity(points.len() * 3);
    for index in 0..points.len() {
        for point in [center, points[index], points[(index + 1) % points.len()]] {
            positions.push([point.x, point.y, 0.0]);
        }
    }
    let count = positions.len();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; count])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; count])
}
fn clip_half(points: Vec<Vec2>, normal: Vec2, offset: f32) -> Vec<Vec2> {
    let mut result = Vec::new();
    if points.is_empty() {
        return result;
    }
    let mut previous = points[points.len() - 1];
    let mut before = normal.dot(previous) - offset;
    for current in points {
        let after = normal.dot(current) - offset;
        if (before >= 0.0) != (after >= 0.0) {
            result.push(previous + (current - previous) * (before / (before - after)));
        }
        if after >= 0.0 {
            result.push(current);
        }
        previous = current;
        before = after;
    }
    result
}
fn clip_sector(mut points: Vec<Vec2>, radius: f32, count: usize) -> Vec<Vec2> {
    for index in 0..48 {
        let a = Vec2::from_angle(index as f32 * std::f32::consts::TAU / 48.0) * radius;
        let b = Vec2::from_angle((index + 1) as f32 * std::f32::consts::TAU / 48.0) * radius;
        let edge = b - a;
        let inward = Vec2::new(-edge.y, edge.x);
        points = clip_half(points, inward, inward.dot(a));
    }
    if count > 1 {
        let half = std::f32::consts::PI / count as f32;
        points = clip_half(points, Vec2::new(half.cos(), half.sin()), 0.0);
        points = clip_half(points, Vec2::new(-half.cos(), half.sin()), 0.0);
    }
    points
}
fn symbol_points(symbol: Symbol) -> Vec<Vec2> {
    match symbol {
        Symbol::Triangle => vec![Vec2::new(-1.0, -0.8), Vec2::new(1.0, -0.8), Vec2::Y],
        Symbol::Square => vec![
            Vec2::new(-1.0, -1.0),
            Vec2::new(1.0, -1.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(-1.0, 1.0),
        ],
        Symbol::Diamond => vec![Vec2::Y, Vec2::NEG_X, Vec2::NEG_Y, Vec2::X],
        Symbol::Plus | Symbol::Cross => {
            let points = vec![
                Vec2::new(-0.3, -1.0),
                Vec2::new(0.3, -1.0),
                Vec2::new(0.3, -0.3),
                Vec2::new(1.0, -0.3),
                Vec2::new(1.0, 0.3),
                Vec2::new(0.3, 0.3),
                Vec2::new(0.3, 1.0),
                Vec2::new(-0.3, 1.0),
                Vec2::new(-0.3, 0.3),
                Vec2::new(-1.0, 0.3),
                Vec2::new(-1.0, -0.3),
                Vec2::new(-0.3, -0.3),
            ];
            points
                .into_iter()
                .map(|p| {
                    if symbol == Symbol::Cross {
                        rotate(p, std::f32::consts::FRAC_PI_4)
                    } else {
                        p
                    }
                })
                .collect()
        }
        Symbol::Star => (0..10)
            .map(|index| {
                Vec2::from_angle(
                    std::f32::consts::FRAC_PI_2 + index as f32 * std::f32::consts::PI / 5.0,
                ) * if index % 2 == 0 { 1.0 } else { 0.45 }
            })
            .collect(),
    }
}
fn stripes(radius: f32, count: usize) -> Vec<Vec<Vec2>> {
    let direction = Vec2::new(1.0, 0.5).normalize();
    let normal = Vec2::new(-direction.y, direction.x) * radius * 0.055;
    [0.12, 0.45]
        .into_iter()
        .map(|offset| {
            let center = Vec2::new(0.0, offset * radius);
            let length = direction * radius * 2.0;
            clip_sector(
                vec![
                    center - length - normal,
                    center + length - normal,
                    center + length + normal,
                    center - length + normal,
                ],
                radius * 0.97,
                count,
            )
        })
        .collect()
}
pub(super) fn radius(scale: f32) -> f32 {
    (22.0 * scale).clamp(24.0, 46.0)
}
fn marker_points(symbol: Symbol, radius: f32, count: usize, angle: f32, scale: f32) -> Vec<Vec2> {
    let sector = (angle * count as f32 / std::f32::consts::TAU).round() as usize;
    let cap_angle = if count == 3 {
        match sector {
            1 => 160.0f32.to_radians(),
            2 => 200.0f32.to_radians(),
            _ => angle,
        }
    } else {
        angle
    };
    let center = rotate(Vec2::Y * radius * 0.70, cap_angle);
    let size = 4.0 * scale; // Eight screen pixels, never shrunk to fit.
    clip_sector(
        symbol_points(symbol)
            .into_iter()
            .map(|p| rotate(center + p * size, -angle))
            .collect(),
        radius * 0.97,
        count,
    )
    .into_iter()
    .map(|p| rotate(p, angle))
    .collect()
}
fn outline_mesh(symbol: Symbol, radius: f32, scale: f32) -> Mesh {
    let points = symbol_points(symbol)
        .into_iter()
        .map(|p| p * radius * 0.74)
        .collect::<Vec<_>>();
    let mut triangles = Vec::new();
    for index in 0..points.len() {
        let a = points[index];
        let b = points[(index + 1) % points.len()];
        let direction = (b - a).normalize();
        let normal = Vec2::new(-direction.y, direction.x) * scale * 0.85;
        let polygon = clip_sector(
            vec![a - normal, b - normal, b + normal, a + normal],
            radius * 0.97,
            1,
        );
        if polygon.len() >= 3 {
            let center = polygon.iter().copied().sum::<Vec2>() / polygon.len() as f32;
            for i in 0..polygon.len() {
                for p in [center, polygon[i], polygon[(i + 1) % polygon.len()]] {
                    triangles.push([p.x, p.y, 0.0]);
                }
            }
        }
    }
    let count = triangles.len();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, triangles)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; count])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; count])
}
pub(super) fn build(
    ball: &Ball,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    scale: f32,
) -> Visual {
    let radius = radius(scale);
    let font = (14.0 * scale).max(20.0);
    let weight = ball.weight.to_string();
    let plate = Vec2::new(
        (weight.len() as f32 * font * 0.29 + scale * 2.0).clamp(radius * 0.36, radius * 0.80),
        scale * 6.0,
    );
    let ink = materials.add(Color::srgb(0.025, 0.035, 0.055));
    let stripe = materials.add(Color::srgba(0.98, 0.99, 1.0, 0.72));
    let mut parts = Vec::new();
    let count = ball.colors.count();
    for (sector, id) in ball.colors.iter().enumerate() {
        let angle = sector as f32 * std::f32::consts::TAU / count as f32;
        let mesh = if count == 1 {
            meshes.add(Circle::new(radius))
        } else {
            meshes.add(CircularSector::new(
                radius,
                std::f32::consts::PI / count as f32,
            ))
        };
        parts.push(Part {
            mesh,
            material: materials.add(super::color(id)),
            transform: Transform::from_rotation(Quat::from_rotation_z(angle)),
            marking: None,
        });
        let cue = cue(id);
        if cue.striped {
            for polygon in stripes(radius, count) {
                if polygon.len() >= 3 {
                    let points = polygon
                        .into_iter()
                        .map(|p| rotate(p, angle))
                        .collect::<Vec<_>>();
                    parts.push(Part {
                        mesh: meshes.add(polygon_mesh(&points)),
                        material: stripe.clone(),
                        transform: Transform::from_xyz(0.0, 0.0, 1.0),
                        marking: None,
                    });
                }
            }
        }
        let marker = if count == 1 {
            outline_mesh(cue.symbol, radius, scale)
        } else {
            polygon_mesh(&marker_points(cue.symbol, radius, count, angle, scale))
        };
        parts.push(Part {
            mesh: meshes.add(marker),
            material: ink.clone(),
            transform: Transform::from_xyz(0.0, 0.0, 2.0),
            marking: Some(Marking { id, cue, sector }),
        });
    }
    parts.push(Part {
        mesh: meshes.add(Annulus::new(radius - scale * 0.55, radius)),
        material: ink,
        transform: Transform::from_xyz(0.0, 0.0, 3.0),
        marking: None,
    });
    parts.push(Part {
        mesh: meshes.add(Ellipse::new(plate.x, plate.y)),
        material: materials.add(Color::srgb(0.98, 0.98, 0.96)),
        transform: Transform::from_xyz(0.0, 0.0, 4.0),
        marking: None,
    });
    Visual {
        parts,
        weight,
        font,
    }
}
pub(super) fn spawn(children: &mut ChildSpawnerCommands<'_>, visual: Visual) {
    for part in visual.parts {
        let mut entity = children.spawn((
            Mesh2d(part.mesh),
            MeshMaterial2d(part.material),
            part.transform,
        ));
        if let Some(marking) = part.marking {
            entity.insert(marking);
        }
    }
    children.spawn((
        Text2d::new(visual.weight),
        TextFont {
            font_size: bevy::text::FontSize::Px(visual.font),
            ..default()
        },
        TextLayout::justify(Justify::Center),
        TextColor(Color::BLACK),
        Transform::from_xyz(0.0, 0.0, 8.0),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_twelve_cues_are_distinct_and_large_ids_are_bounded_deterministic() {
        let cues = (0..12).map(|id| cue(ColorId::new(id))).collect::<Vec<_>>();
        for a in 0..12 {
            for b in a + 1..12 {
                assert_ne!(cues[a], cues[b]);
            }
        }
        for id in [0, 11, 12, 31, u32::MAX] {
            assert_eq!(cue(ColorId::new(id)), cue(ColorId::new(id % 12)));
        }
    }
    #[test]
    fn eight_pixel_joker_symbols_and_stripes_stay_in_sector_and_clear_the_weight() {
        for count in 2..=3 {
            for sector in 0..count {
                let angle = sector as f32 * std::f32::consts::TAU / count as f32;
                let plate = Vec2::new(46.0 * 0.80, 2.4 * 6.0);
                for id in 0..12 {
                    let points =
                        marker_points(cue(ColorId::new(id)).symbol, 46.0, count, angle, 2.4);
                    assert!(points.len() >= 3);
                    for p in points {
                        let local = rotate(p, -angle);
                        assert!(p.length() <= 46.0);
                        let half = std::f32::consts::PI / count as f32;
                        assert!(Vec2::new(half.cos(), half.sin()).dot(local) >= -0.001);
                        assert!(Vec2::new(-half.cos(), half.sin()).dot(local) >= -0.001);
                        assert!((p.x / plate.x).powi(2) + (p.y / plate.y).powi(2) > 1.0);
                    }
                }
                for stripe in stripes(46.0, count) {
                    assert!(stripe.len() >= 3);
                    for p in stripe {
                        assert!(p.length() <= 46.0);
                        let half = std::f32::consts::PI / count as f32;
                        assert!(Vec2::new(half.cos(), half.sin()).dot(p) >= -0.001);
                        assert!(Vec2::new(-half.cos(), half.sin()).dot(p) >= -0.001);
                    }
                }
            }
        }
    }
}
