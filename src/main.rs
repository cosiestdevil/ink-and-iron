#![allow(clippy::too_many_arguments)]
use std::collections::HashMap;

use crate::{
    generate::{CellId, WorldMap},
    pathfinding::ToVec2,
};
use bevy::prelude::*;
use bevy_pancam::{PanCam, PanCamPlugin};
use clap::Parser;
use colorgrad::Gradient;
use glam::Vec2;
use num::Num;
use rand::{Rng, SeedableRng, distr::Uniform};
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
        .add_message::<TurnStart>()
        .insert_resource(GameState::new(2))
        .insert_resource(Random(rng))
        .insert_resource(Selection::None)
        .insert_resource(a)
        .add_systems(Startup, startup)
        .add_systems(
            Update,
            (
                move_unit,
                turn_start,
                temp,
                construct_unit,
                reset_turn_ready_to_end,
            ),
        )
        .add_systems(FixedUpdate, set_unit_next_cell)
        .add_systems(FixedPostUpdate, check_if_turn_ready_to_end)
        .run();
    Ok(())
}
fn temp(
    mut turn_start: MessageWriter<TurnStart>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut game_state: ResMut<GameState>,
) {
    if keyboard.just_pressed(KeyCode::Space) && game_state.turn_ready_to_end {
        let current_player = game_state.active_player;
        let current_player = game_state.players.get(&current_player);
        if let Some(current_player) = current_player {
            let next_player = game_state
                .players
                .values()
                .find(|p| p.order == (current_player.order + 1) % game_state.players.len());
            if let Some(next_player) = next_player {
                turn_start.write(TurnStart {
                    player: next_player.id,
                });
                game_state.active_player = next_player.id;
            }
        }
    }
}

fn construct_unit(
    mut settlements: Query<&mut SettlementCenter>,
    keyboard: Res<ButtonInput<KeyCode>>,
    game_state: Res<GameState>,
    selected: Res<Selection>,
) {
    if let Selection::Settlement(entity) = *selected
        && keyboard.just_pressed(KeyCode::Digit1)
    {
        let mut settlement = settlements.get_mut(entity).unwrap();
        if settlement.controller == game_state.active_player {
            settlement.construction = Some(settlement.available_constructions[0].clone());
        }
    }
}
#[derive(Resource)]
struct Random<R: Rng>(R);
fn startup(
    mut commands: Commands,
    world_map: Res<WorldMap>,
    mut game_state: ResMut<GameState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut random: ResMut<Random<ChaCha20Rng>>,
) {
    let scale = world_map.scale;

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
            vertices.push(*vertices.first().unwrap());
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
            cell.observe(click_cell).observe(over_cell);

            //let outline = commands.spawn().id();
            //cell.add_child(outline);
        }
    }
    for player in game_state.players.values_mut() {
        let mut pos = random.0.sample(
            Uniform::<Vec2>::new(Vec2::ZERO, Vec2::new(16.0 * scale, 9.0 * scale)).unwrap(),
        );
        let cell_id = world_map.get_cell_for_position(pos);
        if let Some(cell_id) = cell_id {
            pos = world_map.voronoi.cell(cell_id.0).site_position().to_vec2() * scale;
            let settlement_mesh = meshes.add(Rectangle::new(10.0, 10.0));
            let unit_mesh = meshes.add(Circle::new(5.0));
            let mut settlment = SettlementCenter {
                controller: player.id,
                production: 1.0,
                construction: None,
                available_constructions: vec![ConstructionJob::Unit(UnitConstuction {
                    cost: 2.0,
                    progress: 0.0,
                    template: UnitTemplate {
                        mesh: Mesh2d(unit_mesh),
                        unit: Unit {
                            speed: 150.0,
                            used_speed: 0.0,
                            current_cell: cell_id,
                            next_cell: None,
                            goal: None,
                            move_timer: None,
                            controller: PlayerId(0),
                        },
                    },
                })],
            };
            player.camera_pos = Some(pos.extend(0.0));
            let mut settlement = commands.spawn((
                Mesh2d(settlement_mesh),
                MeshMaterial2d(materials.add(player.color)),
                Transform::from_translation(pos.extend(3.0)),
                settlment,
            ));
            settlement.observe(click_settlement);
        } else {
            info!("Couldn't find cell at {}", pos);
        }
    }
    let camera_pos = game_state
        .players
        .iter()
        .find(|p| p.1.order == 0)
        .unwrap()
        .1
        .camera_pos
        .unwrap();
    commands.spawn((
        Camera2d,
        Transform::from_translation(camera_pos),
        //Transform::from_xyz(8.0 * scale, 4.5 * scale, 0.0),
        PanCam {
            //max_scale: 1.0,
            ..default()
        },
    ));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlayerId(usize);
#[derive(Message)]
struct TurnStart {
    player: PlayerId,
}
fn check_if_turn_ready_to_end(
    turn_actions: Query<&TurnAction>,
    units: Query<&Unit>,
    mut game_state: ResMut<GameState>,
) {
    if units
        .iter()
        .filter(|u| u.controller == game_state.active_player)
        .all(|u| u.goal.is_some() && u.next_cell.is_none())
        && !game_state.turn_ready_to_end
    {
        game_state.turn_ready_to_end = true;
        info!(
            "Turn ready to end for player: {:?}",
            game_state.active_player
        )
    }
}
fn reset_turn_ready_to_end(
    mut turn_start: MessageReader<TurnStart>,
    mut game_state: ResMut<GameState>,
) {
    for turn in turn_start.read() {
        game_state.turn_ready_to_end = false;
    }
}
fn turn_start(
    mut commands: Commands,
    mut turn_start: MessageReader<TurnStart>,
    mut units: Query<&mut Unit>,
    mut settlements: Query<(&mut SettlementCenter, &Transform)>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    game_state: Res<GameState>,
) {
    for turn in turn_start.read() {
        info!("Turn started for player: {:?}", turn.player);
        let player = game_state.players.get(&turn.player).unwrap();
        for mut unit in units.iter_mut().filter(|u| u.controller == turn.player) {
            unit.used_speed = 0.0;
        }
        for (mut settlement, transform) in settlements
            .iter_mut()
            .filter(|s| s.0.controller == turn.player)
        {
            let production = settlement.production;
            if let Some(ref mut construction) = settlement.construction {
                match construction {
                    ConstructionJob::Unit(unit_constuction) => {
                        if unit_constuction.add_progress(production) {
                            let mut unit = commands.spawn((
                                unit_constuction.template.clone(),
                                MeshMaterial2d(materials.add(player.color)),
                                Transform::from_xyz(
                                    transform.translation.x,
                                    transform.translation.y,
                                    4.0,
                                ),
                            ));
                            unit.observe(click_unit);
                            settlement.construction = None;
                        }
                    }
                }
            }
        }
    }
}
struct Player {
    id: PlayerId,
    order: usize,
    color: Color,
    local: bool,
    camera_pos: Option<Vec3>,
}
#[derive(Resource)]
struct GameState {
    players: HashMap<PlayerId, Player>,
    active_player: PlayerId,
    turn_ready_to_end: bool,
}
impl GameState {
    fn new(player_count: usize) -> Self {
        let mut players = HashMap::with_capacity(player_count);
        for i in 0..player_count {
            let color = Color::hsl(360.0 * (i / (player_count + 1)) as f32, 0.95, 0.7);
            let player = Player {
                order: i,
                id: PlayerId(i),
                local: true,
                camera_pos: None,
                color,
            };
            players.insert(player.id, player);
        }
        let active_player = players.values().find(|p| p.order == 0).unwrap().id;
        Self {
            players,
            active_player,
            turn_ready_to_end: false,
        }
    }
}
#[derive(Component)]
struct TurnAction {
    player: PlayerId,
}
fn set_unit_next_cell(
    mut commands: Commands,
    mut units: Query<(&mut Unit, Entity)>,
    world_map: Res<WorldMap>,
) {
    for (mut unit, entity) in units.iter_mut() {
        let mut a = commands.entity(entity);
        a.remove::<TurnAction>();
        if let Some(goal) = unit.goal {
            if unit.current_cell == goal {
                unit.goal = None;
                info!("Unit reached goal");

                continue;
            }
            if unit.next_cell.is_none() {
                let (graph, nodes) = pathfinding::get_graph(
                    world_map.voronoi.clone(),
                    world_map.cell_height.clone(),
                );
                let result = pathfinding::a_star(
                    unit.current_cell,
                    goal,
                    graph,
                    nodes,
                    world_map.voronoi.clone(),
                );
                match result {
                    Some(mut result) => {
                        let _ = result.pop();
                        let nex_cell = result.pop();
                        if let Some(nex_cell) = nex_cell {
                            let current_cell_pos =
                                world_map.get_position_for_cell(unit.current_cell);
                            let next_cell_pos = world_map.get_position_for_cell(nex_cell);
                            let distance = current_cell_pos.distance(next_cell_pos);
                            if unit.used_speed + distance > unit.speed {
                                unit.next_cell = None;
                                unit.move_timer = None;
                                continue;
                            }
                            a.insert(TurnAction {
                                player: unit.controller,
                            });
                            unit.used_speed += distance;
                            unit.next_cell = Some(nex_cell);
                            unit.move_timer = Some(Timer::from_seconds(5.0, TimerMode::Once));
                            info!("Set unit next_cell");
                        }
                    }
                    None => unit.goal = None,
                }
            }
        }
    }
}
fn move_unit(
    mut units: Query<(&mut Unit, &mut Transform)>,
    world_map: Res<WorldMap>,
    time: Res<Time>,
) {
    for (mut unit, mut transform) in units.iter_mut() {
        if let Some(next_cell) = unit.next_cell {
            if unit.current_cell == next_cell {
                unit.next_cell = None;
                continue;
            }
            let current_cell = unit.current_cell.0;
            let move_timer = unit.move_timer.as_mut().unwrap();
            move_timer.tick(time.delta());

            let next_cell_pos = world_map
                .voronoi
                .cell(next_cell.0)
                .site_position()
                .to_vec2()
                * world_map.scale;
            if move_timer.is_finished() {
                *transform = Transform::from_translation(next_cell_pos.extend(3.0));
                unit.current_cell = next_cell;
                unit.next_cell = None;
            } else {
                let current_cell_pos = world_map
                    .voronoi
                    .cell(current_cell)
                    .site_position()
                    .to_vec2()
                    * world_map.scale;
                let new_pos = current_cell_pos.lerp(next_cell_pos, move_timer.fraction());
                *transform = Transform::from_translation(new_pos.extend(3.0));
            }
        }
    }
}
fn click_unit(mut event: On<Pointer<Click>>, mut selected_unit: ResMut<Selection>) {
    if event.button == PointerButton::Primary {
        *selected_unit = Selection::Unit(event.entity);
        event.propagate(false);
    }
}
fn click_settlement(mut event: On<Pointer<Click>>, mut selected_unit: ResMut<Selection>) {
    if event.button == PointerButton::Primary {
        *selected_unit = Selection::Settlement(event.entity);
        event.propagate(false);
    }
}
fn click_cell(
    mut event: On<Pointer<Click>>,
    cells: Query<&Cell>,
    mut units: Query<&mut Unit>,
    mut selected_unit: ResMut<Selection>,
) {
    if event.button == PointerButton::Primary {
        match *selected_unit {
            Selection::None => {}
            Selection::Unit(unit) => {
                if let Ok(cell) = cells.get(event.entity) {
                    let mut unit = units.get_mut(unit).unwrap();
                    unit.goal = Some(cell.cell_id);
                    info!("Set unit's goal");
                }
            }
            Selection::Settlement(entity) => {
                *selected_unit = Selection::None;
            }
        }
        event.propagate(false);
    }
}
fn over_cell(
    mut event: On<Pointer<Over>>,
    cells: Query<(&Cell, Entity)>,
    units: Query<&Unit>,
    highlights: Query<(Entity, &CellHighlight)>,
    selected: Res<Selection>,
    world_map: Res<WorldMap>,
    mut commands: Commands,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let (graph, nodes) =
        pathfinding::get_graph(world_map.voronoi.clone(), world_map.cell_height.clone());
    if let Selection::Unit(unit_entity) = *selected {
        for (e, highlight) in highlights.iter() {
            let mut e = commands.entity(e);
            e.despawn();
        }
        let unit = units.get(unit_entity).unwrap();
        let start = unit.current_cell;
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
                        CellHighlight {
                            unit: Some(unit_entity),
                        },
                    ));
                }
            }
        }
        event.propagate(false);
    }
}

#[derive(Component)]
struct Cell {
    pub cell_id: CellId,
    outline: Handle<Mesh>,
}
#[derive(Component)]
struct CellHighlight {
    unit: Option<Entity>,
}

#[derive(Resource)]
enum Selection {
    None,
    Unit(Entity),
    Settlement(Entity),
}

#[derive(Component, Clone)]
struct Unit {
    controller: PlayerId,
    speed: f32,
    used_speed: f32,
    current_cell: CellId,
    next_cell: Option<CellId>,
    goal: Option<CellId>,
    move_timer: Option<Timer>,
}

#[derive(Component)]
struct SettlementCenter {
    controller: PlayerId,
    construction: Option<ConstructionJob>,
    production: f32,
    available_constructions: Vec<ConstructionJob>,
}
trait Construction {
    fn add_progress(&mut self, progress: f32) -> bool;
    fn cost(&self) -> f32;
    fn progress(&self) -> f32;
}

#[derive(Clone)]
struct UnitConstuction {
    cost: f32,
    progress: f32,
    template: UnitTemplate,
}

impl Construction for UnitConstuction {
    fn add_progress(&mut self, progress: f32) -> bool {
        self.progress += progress;
        self.progress >= self.cost
    }

    fn cost(&self) -> f32 {
        self.cost
    }

    fn progress(&self) -> f32 {
        self.progress
    }
}
#[derive(Bundle, Clone)]
struct UnitTemplate {
    unit: Unit,
    mesh: Mesh2d,
}
#[derive(Clone)]
enum ConstructionJob {
    Unit(UnitConstuction),
}
