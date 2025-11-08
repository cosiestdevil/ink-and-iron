#![allow(clippy::too_many_arguments)]
use std::{collections::HashMap, time::Duration};

use crate::{
    generate::{CellId, WorldMap},
    llm::SettlementNameCtx,
    pathfinding::ToVec2,
};
use bevy::{
    camera::{Viewport, visibility::RenderLayers},
    color::palettes::css::BLACK,
    ecs::system::SystemState,
    prelude::*,
    render::render_resource::BlendState,
    window::PrimaryWindow,
};
use bevy_egui::{
    EguiContext, EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass,
    PrimaryEguiContext,
    egui::{self, Ui},
};
use bevy_kira_audio::prelude::*;
use bevy_pancam::{PanCam, PanCamPlugin};
use bevy_prototype_lyon::{
    entity::Shape,
    plugin::ShapePlugin,
    prelude::{ShapeBuilder, ShapeBuilderBase},
    shapes::{Circle, RegularPolygon},
};
use bevy_tokio_tasks::TokioTasksRuntime;
use clap::Parser;
use colorgrad::Gradient;
use glam::Vec2;
use num::Num;
use rand::{Rng, SeedableRng, distr::Uniform};
use rand_chacha::ChaCha20Rng;
mod generate;
mod helpers;
mod llm;
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
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash, States)]
enum AppState {
    #[default]
    Loading,
    InGame,
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
                    present_mode: bevy::window::PresentMode::AutoNoVsync,
                    ..default()
                }),
                ..default()
            }),
            PanCamPlugin,
            MeshPickingPlugin,
            ShapePlugin,
            AudioPlugin,
        ))
        .add_plugins(bevy_tokio_tasks::TokioTasksPlugin::default())
        .add_plugins(EguiPlugin::default())
        .add_message::<TurnStart>()
        .init_state::<AppState>()
        .insert_resource(GameState::new(2))
        .insert_resource(Random(rng))
        .insert_resource(Selection::None)
        .insert_resource(a)
        .add_systems(
            Startup,
            (
                generate_settlement_name,
                start_background_audio,
                startup_screens,
                setup_ui_camera,
            ),
        )
        .add_systems(OnExit(AppState::Loading), remove_startup_screen)
        .add_systems(OnEnter(AppState::InGame), startup)
        .add_systems(
            Update,
            (
                move_unit,
                turn_start,
                deselect,
                temp,
                construct_unit,
                reset_turn_ready_to_end,
            )
                .run_if(in_state(AppState::InGame)),
        )
        .add_systems(
            FixedUpdate,
            set_unit_next_cell.run_if(in_state(AppState::InGame)),
        )
        .add_systems(
            FixedPostUpdate,
            check_if_turn_ready_to_end.run_if(in_state(AppState::InGame)),
        )
        .add_systems(
            EguiPrimaryContextPass,
            ui_example_system.run_if(in_state(AppState::InGame)),
        )
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
#[derive(Component)]
struct StartupScreen;
fn startup_screens(mut commands: Commands) {
    commands.spawn((
        StartupScreen,
        Node {
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            flex_direction:FlexDirection::Column,
            ..default()
        },
        children![
            (
                Node { ..default() },
                children![(
                    Text::new("Ink & Iron"),
                    TextFont {
                        font_size: 99.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.9, 0.9, 0.9)),
                    TextShadow::default(),
                )],
            ),
            (
                Node { ..default() },
                children![(
                    Text::new("Cosiest Devil"),
                    TextFont {
                        font_size: 33.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.9, 0.9, 0.9)),
                    TextShadow::default(),
                )]
            )
        ],
    ));
}
fn remove_startup_screen(mut commands: Commands, screens: Query<Entity, With<StartupScreen>>) {
    for entity in screens.iter() {
        let mut screen = commands.entity(entity);
        screen.despawn();
    }
}
fn start_background_audio(asset_server: Res<AssetServer>, audio: Res<Audio>) {
    audio
        .play(asset_server.load("sounds/Pixel Kingdom.wav"))
        .fade_in(AudioTween::new(
            Duration::from_secs(2),
            AudioEasing::InPowi(2),
        ))
        .with_volume(-20.)
        .looped();
}
fn deselect(
    mut commands: Commands,
    highlights: Query<Entity, With<CellHighlight>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut selected: ResMut<Selection>,
) {
    if keyboard.just_pressed(KeyCode::Escape) {
        *selected = Selection::None;
        for entity in highlights.iter() {
            let mut highlight = commands.entity(entity);
            highlight.despawn();
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
fn generate_settlement_name(
    mut rng: ResMut<Random<ChaCha20Rng>>,
    runtime: ResMut<TokioTasksRuntime>,
    game_state: Res<GameState>,
) {
    let temp = rng.0.random_range(0.3..0.5);
    for player in game_state.players.values() {
        let context = player.settlement_context.clone();
        let player_id = player.id;
        runtime.spawn_background_task(move |mut ctx| async move {
            if let Ok(names) = llm::settlement_names(context.clone(), temp).await {
                ctx.run_on_main_thread(move |ctx| {
                    let world = ctx.world;
                    let (mut game_state, mut next_state) = {
                        let mut system_state = SystemState::<(
                            ResMut<GameState>,
                            ResMut<NextState<AppState>>,
                        )>::new(world);
                        system_state.get_mut(world)
                    };
                    let player = game_state.players.get_mut(&player_id).unwrap();
                    player.settlement_names = names;
                    if game_state
                        .players
                        .values()
                        .all(|p| !p.settlement_names.is_empty())
                    {
                        next_state.set(AppState::InGame);
                    }
                })
                .await;
            }
        });
    }
}
#[derive(Resource)]
struct Random<R: Rng>(R);

fn setup_ui_camera(mut commands: Commands, mut egui_global_settings: ResMut<EguiGlobalSettings>) {
    egui_global_settings.auto_create_primary_context = false;
    commands.spawn((
        // The `PrimaryEguiContext` component requires everything needed to render a primary context.
        PrimaryEguiContext,
        Camera2d,
        // Setting RenderLayers to none makes sure we won't render anything apart from the UI.
        RenderLayers::none(),
        Camera {
            order: 1,
            output_mode: bevy::camera::CameraOutputMode::Write {
                blend_state: Some(BlendState::ALPHA_BLENDING),
                clear_color: ClearColorConfig::None,
            },
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
    ));
}

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

        let polygon = bevy_prototype_lyon::prelude::shapes::Polygon {
            points: vertices.clone(),
            closed: true,
        };
        let color = g.at(*world_map.cell_height.get(&CellId(v_cell.site())).unwrap());
        let color = bevy::color::Color::srgb(color.r, color.g, color.b);
        let cell_shape = ShapeBuilder::with(&polygon)
            .fill(color)
            .stroke((BLACK, 2.0))
            .build();
        let polyline = bevy::math::primitives::Polyline2d::new(vertices);
        let outline_mesh_id = meshes.add(polyline);
        let mut cell = commands.spawn((
            cell_shape,
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
        cell.observe(click_cell).observe(over_cell);
    }
    for player in game_state.players.values_mut() {
        let mut pos = random.0.sample(
            Uniform::<Vec2>::new(Vec2::ZERO, Vec2::new(16.0 * scale, 9.0 * scale)).unwrap(),
        );
        let cell_id = world_map.get_cell_for_position(pos);
        if let Some(cell_id) = cell_id {
            pos = world_map.voronoi.cell(cell_id.0).site_position().to_vec2() * scale;
            let settlement_mesh = ShapeBuilder::with(&RegularPolygon {
                sides: 4,
                center: Vec2::ZERO,
                feature: bevy_prototype_lyon::shapes::RegularPolygonFeature::SideLength(10.0),
            })
            .fill(player.color)
            .stroke((BLACK, 1.0))
            .build();
            let unit_mesh = ShapeBuilder::with(&Circle {
                radius: 5.0,
                center: Vec2::ZERO,
            })
            .fill(player.color)
            .stroke((BLACK, 1.0))
            .build();
            let name = player
                .settlement_names
                .pop()
                .unwrap_or(format!("Settlement: {}", player.id.0));
            let mut settlment = SettlementCenter {
                name,
                controller: player.id,
                production: 1.0,
                construction: None,
                available_constructions: vec![ConstructionJob::Unit(UnitConstuction {
                    name: "Unit 1".to_string(),
                    cost: 2.0,
                    progress: 0.0,
                    template: UnitTemplate {
                        mesh: unit_mesh,
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

            let camera_entity = commands
                .spawn((
                    Camera2d,
                    Transform::from_translation(pos.extend(0.0)),
                    //Transform::from_xyz(8.0 * scale, 4.5 * scale, 0.0),
                    Camera {
                        is_active: player.order == 0,
                        ..default()
                    },
                    PanCam {
                        enabled: player.order == 0,
                        //max_scale: 1.0,
                        ..default()
                    },
                ))
                .id();
            player.camera_entity = Some(camera_entity);
            let mut settlement = commands.spawn((
                settlement_mesh,
                Transform::from_translation(pos.extend(3.0)),
                settlment,
            ));
            settlement.observe(click_settlement);
        } else {
            info!("Couldn't find cell at {}", pos);
        }
    }

    // Egui camera.
}
// This function runs every frame. Therefore, updating the viewport after drawing the gui.
// With a resource which stores the dimensions of the panels, the update of the Viewport can
// be done in another system.
fn ui_example_system(
    mut contexts: EguiContexts,
    mut camera: Query<&mut Camera, Without<EguiContext>>,
    window: Single<&mut Window, With<PrimaryWindow>>,
    game_state: Res<GameState>,
    selected: Res<Selection>,
    mut settlements: Query<&mut SettlementCenter>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let player = game_state.players.get(&game_state.active_player).unwrap();
    let mut left = egui::SidePanel::left("left_panel")
        .resizable(true)
        .show(ctx, |ui| {
            ui.label("Left resizeable panel");
            ui.allocate_rect(ui.available_rect_before_wrap(), egui::Sense::hover());
        })
        .response
        .rect
        .width(); // height is ignored, as the panel has a hight of 100% of the screen

    let mut right = egui::SidePanel::right("right_panel")
        .resizable(true)
        .show(ctx, |ui| {
            ui.label("Right resizeable panel");
            match *selected {
                Selection::None => {}
                Selection::Unit(entity) => {}
                Selection::Settlement(entity) => {
                    let settlement = settlements.get_mut(entity).unwrap();
                    ui.label(settlement.name.clone());
                    if let Some(ref job) = settlement.construction {
                        job.progress_label(ui);
                    } else {
                        ui.label("No Construction Queued");
                    }
                    for job in settlement.available_constructions.iter() {
                        job.available_label(ui);
                    }
                }
            }

            ui.allocate_rect(ui.available_rect_before_wrap(), egui::Sense::hover());
        })
        .response
        .rect
        .width(); // height is ignored, as the panel has a height of 100% of the screen

    let mut top = egui::TopBottomPanel::top("top_panel")
        .resizable(true)
        .show(ctx, |ui| {
            ui.label("Top resizeable panel");
            ui.allocate_rect(ui.available_rect_before_wrap(), egui::Sense::hover());
        })
        .response
        .rect
        .height(); // width is ignored, as the panel has a width of 100% of the screen
    let mut bottom = egui::TopBottomPanel::bottom("bottom_panel")
        .resizable(true)
        .show(ctx, |ui| {
            ui.label("Bottom resizeable panel");
            ui.allocate_rect(ui.available_rect_before_wrap(), egui::Sense::hover());
        })
        .response
        .rect
        .height(); // width is ignored, as the panel has a width of 100% of the screen

    // Scale from logical units to physical units.
    left *= window.scale_factor();
    right *= window.scale_factor();
    top *= window.scale_factor();
    bottom *= window.scale_factor();

    // -------------------------------------------------
    // |  left   |            top   ^^^^^^   |  right  |
    // |  panel  |           panel  height   |  panel  |
    // |         |                  vvvvvv   |         |
    // |         |---------------------------|         |
    // |         |                           |         |
    // |<-width->|          viewport         |<-width->|
    // |         |                           |         |
    // |         |---------------------------|         |
    // |         |          bottom   ^^^^^^  |         |
    // |         |          panel    height  |         |
    // |         |                   vvvvvv  |         |
    // -------------------------------------------------
    //
    // The upper left point of the viewport is the width of the left panel and the height of the
    // top panel
    //
    // The width of the viewport the width of the top/bottom panel
    // Alternative the width can be calculated as follow:
    // size.x = window width - left panel width - right panel width
    //
    // The height of the viewport is:
    // size.y = window height - top panel height - bottom panel height
    //
    // Therefore we use the alternative for the width, as we can callculate the Viewport as
    // following:

    let pos = UVec2::new(left as u32, top as u32);
    let size = UVec2::new(window.physical_width(), window.physical_height())
        - pos
        - UVec2::new(right as u32, bottom as u32);

    if let Some(camera_entity) = player.camera_entity {
        let mut camera = camera.get_mut(camera_entity).unwrap();
        camera.viewport = Some(Viewport {
            physical_position: pos,
            physical_size: size,
            ..default()
        });
    }

    Ok(())
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlayerId(usize);
#[derive(Message)]
struct TurnStart {
    player: PlayerId,
}
fn check_if_turn_ready_to_end(
    units: Query<&Unit>,
    settlements: Query<&SettlementCenter>,
    mut game_state: ResMut<GameState>,
) {
    let player_units_used = units
        .iter()
        .filter(|u| u.controller == game_state.active_player)
        .all(|u| (u.goal.is_some() && u.next_cell.is_none()) || u.used_speed > 0.0);
    let player_settlements_busy = settlements
        .iter()
        .filter(|u| u.controller == game_state.active_player)
        .all(|s| s.construction.is_some());
    if player_units_used && player_settlements_busy && !game_state.turn_ready_to_end {
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
    mut cameras: Query<(&mut Camera, &mut PanCam, Entity), Without<EguiContext>>,
    mut turn_start: MessageReader<TurnStart>,
    mut units: Query<&mut Unit>,
    mut settlements: Query<(&mut SettlementCenter, &Transform)>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut selected: ResMut<Selection>,
    highlights: Query<Entity, With<CellHighlight>>,
    game_state: Res<GameState>,
) {
    for turn in turn_start.read() {
        let player = game_state.players.get(&turn.player).unwrap();
        for (mut camera, mut pancam, entity) in cameras.iter_mut() {
            if let Some(player_camera_entity) = player.camera_entity
                && player_camera_entity == entity
            {
                camera.is_active = true;
                pancam.enabled = true;
            } else {
                camera.is_active = false;
                pancam.enabled = false;
            }
        }
        info!("Turn started for player: {:?}", turn.player);
        *selected = Selection::None;
        for entity in highlights.iter() {
            let mut highlight = commands.entity(entity);
            highlight.despawn();
        }
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
                                //MeshMaterial2d(materials.add(player.color)),
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
    camera_entity: Option<Entity>,
    settlement_names: Vec<String>,
    settlement_context: SettlementNameCtx,
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
        let civs = ["Luikha Empire", "Ishabia Kingdom"];
        for i in 0..player_count {
            let t = (i as f32 / (player_count + 1) as f32);
            let color = Color::hsl(360.0 * t, 0.95, 0.7);
            let player = Player {
                order: i,
                id: PlayerId(i),
                local: true,
                settlement_names: vec![],
                settlement_context: SettlementNameCtx {
                    civilisation_name: civs[i].to_string(),
                },
                camera_entity: None,
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
    name: String,
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
    name: String,
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
    mesh: Shape,
}
#[derive(Clone)]
enum ConstructionJob {
    Unit(UnitConstuction),
}
impl ConstructionJob {
    pub fn progress_label(&self, mut ui: &mut Ui) {
        match self {
            ConstructionJob::Unit(unit_constuction) => {
                ui.label(format!(
                    "{}: {}/{}",
                    unit_constuction.name, unit_constuction.progress, unit_constuction.cost
                ));
            }
        }
    }
    pub fn available_label(&self, ui: &mut Ui) {
        match self {
            ConstructionJob::Unit(unit_constuction) => {
                ui.label(format!(
                    "{}: {}",
                    unit_constuction.name, unit_constuction.cost
                ));
            }
        }
    }
}
