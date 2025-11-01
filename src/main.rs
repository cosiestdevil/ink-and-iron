use crate::generate::{CellId, WorldMap};
use bevy::prelude::*;
use bevy_pancam::{PanCam, PanCamPlugin};
use clap::Parser;
use colorgrad::Gradient;
use glam::Vec2;
use num::Num;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

mod generate;
mod helpers;
#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value_t = 16.0)]
    width: f64,
    #[arg(long, default_value_t = 9.0)]
    height: f64,
    #[arg(long, default_value_t = 10)]
    plate_count: usize,
    #[arg(long, default_value_t = 10)]
    plate_size: usize,
    #[arg(long, default_value_t = 55)]
    continent_count: usize,
    #[arg(long, default_value_t = 350)]
    continent_size: usize,
    #[arg(long, default_value_t = 66)]
    ocean_count: usize,
    #[arg(long, default_value_t = 250)]
    ocean_size: usize,
    #[arg(long)]
    seed: Option<String>,
}
fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let mut rng = match args.seed {
        Some(ref s) => {
            let num = num::BigUint::from_str_radix(s, 36)?;
            let seed_bytes = num.to_bytes_le();
            let mut seed_arr = [0u8; 32];
            for (i, b) in seed_bytes.iter().enumerate().take(32) {
                seed_arr[i] = *b;
            }
            ChaCha20Rng::from_seed(seed_arr)
        }
        None => ChaCha20Rng::from_os_rng(),
    };
    let a = generate::generate_world((&args).into(), &mut rng)?;
    let seed = rng.get_seed();
    let num = num::BigUint::from_bytes_le(&seed);
    let seed = num.to_str_radix(36);
    println!("Seed: {}", seed);
    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    present_mode: bevy::window::PresentMode::Mailbox,
                    ..default()
                }),
                ..default()
            }),
            PanCamPlugin,
        ))
        .insert_resource(a)
        .add_systems(Startup, startup)
        .run();
    Ok(())
}
fn startup(
    mut commands: Commands,
    world_map: Res<WorldMap>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.spawn((Camera2d, PanCam::default()));
    let scale = 500.0;
    let g = colorgrad::GradientBuilder::new()
        .css("#001a33 0%, #003a6b 18%, #0f7a8a 32%, #bfe9e9 42%, #f2e6c8 48%, #e8d7a1 52%, #a7c88a 62%, #5b7f3a 72%, #8c8f93 85%, #cdd2d8 93%, #ffffff 100%   ")
        .build::<colorgrad::LinearGradient>().unwrap();
    for v_cell in world_map.voronoi.iter_cells() {
        if v_cell.is_on_hull() {
            continue;
        }
        let mut vertices = v_cell
            .iter_vertices()
            .map(|p| {
                Vec2::new(
                    (p.x - v_cell.site_position().x) as f32 * scale,
                    (p.y - v_cell.site_position().y) as f32 * scale,
                )
            })
            .collect::<Vec<_>>();
        vertices.reverse();
        if let Ok(polygon) = bevy::math::primitives::ConvexPolygon::new(vertices.clone()) {
            let mesh_id = meshes.add(polygon);
            let color = g.at(*world_map.cell_height.get(&CellId(v_cell.site())).unwrap());
            let color = bevy::color::Color::srgb(color.r, color.g, color.b);
            commands.spawn((
                Mesh2d(mesh_id),
                MeshMaterial2d(materials.add(color)),
                Transform::from_xyz(
                    // Distribute shapes from -X_EXTENT/2 to +X_EXTENT/2.
                    v_cell.site_position().x as f32 * scale,
                    v_cell.site_position().y as f32 * scale,
                    0.0,
                ),
            ));
            let polyline = bevy::math::primitives::Polyline2d::new(vertices);
            let mesh_id = meshes.add(polyline);
            commands.spawn((
                Mesh2d(mesh_id),
                MeshMaterial2d(materials.add(bevy::color::Color::BLACK)),
                Transform::from_xyz(
                    // Distribute shapes from -X_EXTENT/2 to +X_EXTENT/2.
                    v_cell.site_position().x as f32 * scale,
                    v_cell.site_position().y as f32 * scale,
                    0.0,
                ),
            ));
        }
    }
}
