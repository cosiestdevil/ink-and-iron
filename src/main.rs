use crate::generate::{CellId, WorldMap};
use bevy::{ecs::relationship::RelationshipSourceCollection, prelude::*};
use bevy_pancam::{PanCam, PanCamPlugin};
use clap::Parser;
use colorgrad::Gradient;
use glam::Vec2;
use num::Num;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

mod generate;
mod helpers;
mod pathfinding;
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
            MeshPickingPlugin,
        ))
        .insert_resource(SelectedCell(None))
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
    let scale = 500.0;
    commands.spawn((
        Camera2d,
        Transform::from_xyz(8.0 * scale, 4.5 * scale, 0.0),
        PanCam {
            max_scale: 1.0,
            ..default()
        },
    ));

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
            let polyline = bevy::math::primitives::Polyline2d::new(vertices);
            let outline_mesh_id = meshes.add(polyline);
            let mut cell = commands.spawn((
                Mesh2d(mesh_id.clone()),
                MeshMaterial2d(materials.add(color)),
                Transform::from_xyz(
                    // Distribute shapes from -X_EXTENT/2 to +X_EXTENT/2.
                    v_cell.site_position().x as f32 * scale,
                    v_cell.site_position().y as f32 * scale,
                    0.0,
                ),
                Cell {
                    cell_id: CellId(v_cell.site()),
                    outline: outline_mesh_id.clone(),
                },
            ));

            cell.with_child((
                Mesh2d(outline_mesh_id.clone()),
                MeshMaterial2d(materials.add(bevy::color::Color::BLACK)),
                Transform::from_xyz(0.0, 0.0, 1.0),
            ));
            cell.observe(
                |mut event: On<Pointer<Click>>,
                 cells: Query<&Cell>,
                 mut selected: ResMut<SelectedCell>| {
                    if event.button == PointerButton::Primary {
                        selected.0 = Some(cells.get(event.entity).unwrap().cell_id);
                        event.propagate(false);
                    }
                },
            )
            .observe(over_cell);

            //let outline = commands.spawn().id();
            //cell.add_child(outline);
        }
    }
    fn over_cell(
        mut event: On<Pointer<Over>>,
        cells: Query<(&Cell, Entity)>,
        highlights: Query<Entity, With<CellHighlight>>,
        selected: Res<SelectedCell>,
        world_map: Res<WorldMap>,
        mut commands: Commands,
        mut materials: ResMut<Assets<ColorMaterial>>,
    ) {
        let (graph, nodes) = pathfinding::get_graph(world_map.voronoi.clone());
        if let Some(start) = selected.0 {
            for e in highlights.iter() {
                let mut e = commands.entity(e);
                e.despawn();
            }
            let goal = cells.get(event.entity).unwrap().0.cell_id;
            let result = pathfinding::a_star(start, goal, graph, nodes, world_map.voronoi.clone());
            if let Some(result) = result {
                for cell_id in result {
                    let cell = cells.iter().find(|e| e.0.cell_id == cell_id);
                    if let Some((cell, entity)) = cell {
                        let mut e = commands.entity(entity);
                        e.with_child((
                            Mesh2d(cell.outline.clone()),
                            MeshMaterial2d(materials.add(bevy::color::Color::WHITE)),
                            Transform::from_xyz(0.0, 0.0, 2.0),
                            CellHighlight,
                        ));
                    }
                }
            }
            event.propagate(false);
        }
    }
}
#[derive(Component)]
struct Cell {
    pub cell_id: CellId,
    outline: Handle<Mesh>,
}
#[derive(Component)]
struct CellHighlight;
#[derive(Resource)]
struct SelectedCell(Option<CellId>);
