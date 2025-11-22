#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(clippy::too_many_arguments)]
use std::{collections::HashMap, path::Path, time::Duration};

use crate::{
    generate::ToVec2,
    generate::{CellId, WorldMap},
    llm::SettlementNameCtx,
};
use bevy::{
    asset::RenderAssetUsages,
    camera::Exposure,
    ecs::system::SystemState,
    input_focus::InputFocus,
    light::{AtmosphereEnvironmentMapLight, NotShadowCaster, light_consts::lux},
    log::LogPlugin,
    math::bounding::Aabb2d,
    mesh::{Indices, PrimitiveTopology},
    pbr::Atmosphere,
    post_process::bloom::Bloom,
    prelude::*,
};
use bevy_easings::{Ease, EasingsPlugin};
use bevy_egui::{
    EguiContext,
    egui::{self, Response, Ui},
};
use bevy_kira_audio::prelude::*;
use bevy_prototype_lyon::plugin::ShapePlugin;
use bevy_rts_camera::{Ground, RtsCamera, RtsCameraControls, RtsCameraPlugin};
use bevy_tokio_tasks::TokioTasksRuntime;
use clap::Parser;
use colorgrad::Gradient;
use num::Num;
use rand::{Rng, SeedableRng, distr::Uniform};
use rand_chacha::ChaCha20Rng;
mod generate;
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
pub(crate) enum AppState {
    #[default]
    Loading,
    Menu,
    Generating,
    InGame,
}
mod logs;
mod menu;
fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    App::new()
        .add_plugins((
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        present_mode: bevy::window::PresentMode::AutoNoVsync,
                        ..default()
                    }),
                    ..default()
                })
                .set(LogPlugin {
                    custom_layer: logs::custom_layer,
                    ..default()
                }),
            //PanCamPlugin,
            MeshPickingPlugin,
            ShapePlugin,
            AudioPlugin,
            EasingsPlugin::default(),
            RtsCameraPlugin,
        ))
        .add_audio_channel::<Music>()
        .add_plugins(bevy_tokio_tasks::TokioTasksPlugin::default())
        .add_plugins(crate::ui::UIPlugin)
        .add_plugins(crate::generate::WorldPlugin)
        .add_plugins(crate::menu::MenuPlugin)
        .add_message::<TurnStart>()
        .init_state::<AppState>()
        .init_resource::<InputFocus>()
        .insert_resource(AudioSettings::default())
        .insert_resource(Seed(args.seed.clone()))
        .insert_resource(GameState::new(2))
        .insert_resource::<Random<RandomRng>>(Random(None))
        .insert_resource(Selection::None)
        .insert_resource::<generate::WorldGenerationParams>((&args).into())
        .add_systems(
            Startup,
            (
                start_background_audio,
                startup_screens,
                setup_rng,
                archive_old_logs,
            ),
        )
        .add_systems(Update, loaded.run_if(in_state(AppState::Loading)))
        .add_systems(OnExit(AppState::Loading), remove_startup_screen)
        .add_systems(OnEnter(AppState::InGame), startup)
        .add_systems(
            Update,
            (
                move_unit,
                turn_start,
                deselect,
                reset_turn_ready_to_end,
                move_sun,
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
        .run();
    Ok(())
}

fn loaded(
    mut next_state: ResMut<NextState<AppState>>,
    mut timer: Local<Option<Timer>>,
    time: Res<Time>,
) {
    let timer = timer.get_or_insert(Timer::new(Duration::from_secs(5), TimerMode::Once));
    timer.tick(time.delta());
    if timer.is_finished() {
        next_state.set(AppState::Menu);
    }
}
#[derive(Resource)]
struct Music;
#[derive(Component)]
struct StartupScreen;

#[derive(Resource)]
struct Seed(Option<String>);

#[derive(Resource)]
struct AudioSettings {
    music_volume: f32,
}
impl Default for AudioSettings {
    fn default() -> Self {
        AudioSettings { music_volume: 1.0 }
    }
}

fn setup_rng(mut random: ResMut<Random<ChaCha20Rng>>, seed: Res<Seed>) {
    let rng = match seed.0.as_ref() {
        Some(s) => {
            let num = num::BigUint::from_str_radix(s, 36).unwrap();
            let seed_bytes = num.to_bytes_le();
            let mut seed_arr = [0u8; 32];
            for (i, b) in seed_bytes.iter().enumerate().take(32) {
                seed_arr[i] = *b;
            }
            ChaCha20Rng::from_seed(seed_arr)
        }
        None => ChaCha20Rng::from_os_rng(),
    };
    let seed = rng.get_seed();
    let num = num::BigUint::from_bytes_le(&seed);
    let seed = num.to_str_radix(36);
    info!("Seed: {}", seed);
    random.0 = Some(rng);
}
fn startup_screens(mut commands: Commands) {
    commands.spawn((
        StartupScreen,
        Node {
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            flex_direction: FlexDirection::Column,
            position_type: PositionType::Absolute,
            top: Val::Percent(-100.0),
            ..default()
        }
        .ease_to_fn(
            |prev| Node {
                top: Val::Percent(0.0),
                ..prev.clone()
            },
            bevy_easings::EaseFunction::BounceInOut,
            bevy_easings::EasingType::Once {
                duration: std::time::Duration::from_secs(2),
            },
        )
        .with_original_value(),
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
fn start_background_audio(
    asset_server: Res<AssetServer>,
    audio: Res<AudioChannel<Music>>,
    audio_settings: Res<AudioSettings>,
) {
    audio
        .play(asset_server.load("sounds/Pixel Kingdom.wav"))
        .fade_in(AudioTween::new(
            Duration::from_secs(2),
            AudioEasing::InPowi(2),
        ))
        .with_volume(((1.0 - audio_settings.music_volume) * -40.) - 10.)
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
fn archive_old_logs(runtime: ResMut<TokioTasksRuntime>) {
    runtime.spawn_background_task(move |_| async move {
        logs::archive_old_logs(Path::new("logs")).unwrap();
    });
}
pub type RandomRng = ChaCha20Rng;
#[derive(Resource)]
pub struct Random<R: Rng>(Option<R>);
mod ui;
fn smooth01(t: f32) -> f32 {
    // standard smoothstep from 0..1
    t * t * (3.0 - 2.0 * t)
}
fn move_sun(mut light: Query<(&mut Transform, &mut DirectionalLight)>, time: Res<Time>) {
    let noon = vec3(0.0, 200.0, -200.0);
    let day_t = (time.elapsed_secs() % 300.0) / 300.0;
    let sunrise = vec3(-200.0, 0.0, 0.0);
    let sunset = vec3(200.0, 0.0, 0.0);
    let midnight = vec3(0.0, -200.0, 200.0);
    let l0 = 0.165; // midnight -> sunrise
    let l1 = 0.33; // sunrise -> noon
    let l2 = 0.33; // noon -> sunset
    // ensure they add up to 1.0 exactly
    let l3 = 1.0 - (l0 + l1 + l2); // ~0.175, sunset -> midnight

    let t = day_t.fract(); // wrap into 0..1

    let pos = if t < l0 {
        // midnight → sunrise
        let u = t / l0;
        let s = smooth01(u);
        midnight.lerp(sunrise, s)
    } else if t < l0 + l1 {
        // sunrise → noon
        let u = (t - l0) / l1;
        let s = smooth01(u);
        sunrise.lerp(noon, s)
    } else if t < l0 + l1 + l2 {
        // noon → sunset
        let u = (t - (l0 + l1)) / l2;
        let s = smooth01(u);
        noon.lerp(sunset, s)
    } else {
        // sunset → midnight
        let u = (t - (l0 + l1 + l2)) / l3;
        let s = smooth01(u);
        sunset.lerp(midnight, s)
    };
    let (mut transform, mut light) = light.single_mut().unwrap();
    *transform = Transform::from_translation(pos).looking_at(vec3(0.0, 0.0, 0.0), Vec3::Y);
    let sun_dir = pos.normalize();
    let elevation = sun_dir.y;
    let g = colorgrad::GradientBuilder::new()
        .css("#ff9a9e 0%,    #fecf71 25%,    #FAFAD2 60%,    #DBF7FF 100%")
        .build::<colorgrad::LinearGradient>()
        .unwrap();

    let color = if elevation > 0.0 {
        g.at(elevation)
    } else {
        colorgrad::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        }
    };
    light.color = bevy::color::Color::srgba(color.r, color.g, color.b, 1.0);
}
fn startup(
    mut commands: Commands,
    world_map: Res<WorldMap>,
    mut game_state: ResMut<GameState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut random: ResMut<Random<ChaCha20Rng>>,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let scale = world_map.scale;
    let parchment_handle: Handle<Image> = asset_server.load("textures/brown-texture.png");
    let g = colorgrad::GradientBuilder::new()
        .css("#001a33 0%, #003a6b 18%, #0f7a8a 32%, #bfe9e9 42%, #f2e6c8 48%, #e8d7a1 52%, #a7c88a 62%, #5b7f3a 72%, #8c8f93 85%, #cdd2d8 93%, #ffffff 100%   ")
        .build::<colorgrad::LinearGradient>().unwrap();
    let map_box = world_map.bounds();
    let mut height_material_cache = HashMap::<u8, Handle<StandardMaterial>>::new();
    let outline_material = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        unlit: true,

        ..Default::default()
    });
    let ocean_material = materials.add(StandardMaterial {
        base_color_texture: None, // Some(parchment_handle.clone()),
        base_color: bevy::color::palettes::css::DEEP_SKY_BLUE
            .with_alpha(0.3)
            .into(),
        alpha_mode: AlphaMode::AlphaToCoverage,
        perceptual_roughness: 0.2,
        unlit: false,
        ..default()
    });
    for v_cell in world_map.iter_cells() {
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
        vertices.push(vertices[0]);
        // let polygon = bevy_prototype_lyon::prelude::shapes::Polygon {
        //     points: vertices.clone(),
        //     closed: true,
        // };
        // let vertices3: Vec<(Vec3, Vec2)> = vertices
        //     .iter()
        //     .map(|(v, uv)| (v.extend(0.0).xzy(), *uv))
        //     .collect();
        //vertices3.reverse();
        let height = world_map.get_raw_height(&CellId(v_cell.site()));

        let height_key = (height * 100.0).round() as u8;
        assert!((0.0..=1.0).contains(&height));
        let material = if let Some(mat) = height_material_cache.get(&height_key) {
            mat.clone()
        } else {
            let color = g.at(height);
            assert!(color.to_css_hex() != "#000000");
            let color = bevy::color::Color::srgba(color.r, color.g, color.b, 1.0); //.lighter(0.2);
            //let color = Color::linear_rgba(height, height, height, 1.0);
            let mat = materials.add(StandardMaterial {
                base_color_texture: Some(parchment_handle.clone()),
                base_color: color,
                perceptual_roughness: 0.9,
                unlit: false,
                ..default()
            });
            height_material_cache.insert(height_key, mat.clone());
            mat.clone()
        };

        let scaled_height = height * world_map.height_scale;
        let mesh = build_extruded_with_caps(
            &vertices,
            scaled_height,
            v_cell.site_position().to_vec2() * scale,
            map_box,
        );
        //let mesh = Cuboid::new(1.0, scaled_height, 1.0);
        if height < 0.5 {
            let mesh = build_extruded_with_caps(
                &vertices,
                (0.5 - height) * scale * 0.25,
                v_cell.site_position().to_vec2() * scale,
                map_box,
            );

            commands.spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(ocean_material.clone()),
                Transform::from_xyz(
                    // Distribute shapes from -X_EXTENT/2 to +X_EXTENT/2.
                    v_cell.site_position().x as f32 * scale,
                    scaled_height,
                    v_cell.site_position().y as f32 * scale,
                ),
                Ground,
                NotShadowCaster,
            ));
        }
        let line = Polyline3d::new(extrude_polygon_xz_to_polyline_vertices(
            &vertices,
            0.0,
            scaled_height,
        ));
        let outline_mesh = meshes.add(line);

        let mut cell = commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material),
            Transform::from_xyz(
                // Distribute shapes from -X_EXTENT/2 to +X_EXTENT/2.
                v_cell.site_position().x as f32 * scale,
                0.0,
                v_cell.site_position().y as f32 * scale,
            ),
            Cell {
                cell_id: CellId(v_cell.site()),
                outline: outline_mesh.clone(),
            },
            Ground,
            children![(
                Mesh3d(outline_mesh),
                MeshMaterial3d(outline_material.clone()),
                Transform::IDENTITY
            )],
        ));
        cell.observe(click_cell).observe(over_cell);
    }
    // let cascade_shadow_config = CascadeShadowConfigBuilder {
    //         first_cascade_far_bound: 0.3,
    //         maximum_distance: 3.0*scale,
    //         ..default()
    //     }
    //     .build();
    commands.spawn((
        DirectionalLight {
            shadows_enabled: true,
            illuminance: lux::RAW_SUNLIGHT,
            ..default()
        },
        //cascade_shadow_config,
        Transform::from_xyz(0.0, 200.0, -200.0).looking_at(vec3(0.0, 0.0, 0.0), Vec3::Y),
    ));
    for player in game_state.players.values_mut() {
        let valid_settlment_cells = world_map.get_valid_settlement_cells();
        let valid_settlment_cells_i = random
            .0
            .as_mut()
            .unwrap()
            .sample(Uniform::new(0, valid_settlment_cells.len()).unwrap());
        let cell_id = valid_settlment_cells.get(valid_settlment_cells_i).copied();
        // let pos = random
        //     .0
        //     .sample(Uniform::<Vec2>::new(map_box.0, map_box.1).unwrap());
        // let cell_id = world_map.get_cell_for_position(pos);
        if let Some(cell_id) = cell_id {
            let pos = world_map.get_position_for_cell(cell_id);
            let player_mat = materials.add(player.color);
            let settlement_mesh = meshes.add(Cuboid::from_length(world_map.entity_scale));
            let unit_mesh = meshes.add(Cylinder::new(
                world_map.entity_scale,
                world_map.entity_scale,
            ));
            let name = player
                .settlement_names
                .pop()
                .unwrap_or(format!("Settlement: {}", player.id.0));
            let settlment = SettlementCenter {
                name,
                controller: player.id,
                production: 1.0,
                construction: None,
                cell: cell_id,
                available_constructions: vec![ConstructionJob::Unit(UnitConstuction {
                    name: "Unit 1".to_string(),
                    cost: 2.0,
                    progress: 0.0,
                    template: UnitTemplate {
                        mesh: Mesh3d(unit_mesh),
                        material: MeshMaterial3d(player_mat.clone()),
                        unit: Unit {
                            name: "Unit 1".to_string(),
                            speed: 5.0,
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
                    Camera3d { ..default() },
                    // PanOrbitCamera {
                    //     enabled: player.order == 0,
                    //     ..default()
                    // },
                    Atmosphere::EARTH,
                    // AtmosphereSettings {
                    //     aerial_view_lut_max_distance: 3.2e5,
                    //     scene_units_to_m: 1e+4,
                    //     ..Default::default()
                    // },
                    Bloom::NATURAL,
                    Exposure::SUNLIGHT,
                    AtmosphereEnvironmentMapLight::default(),
                    RtsCamera {
                        height_max: scale * 10.0,
                        target_zoom: 0.8,
                        target_focus: Transform::from_translation(pos),
                        bounds: Aabb2d {
                            max: map_box.1,
                            min: map_box.0,
                        },
                        min_angle: 0.0f32.to_radians(),
                        ..default()
                    },
                    RtsCameraControls {
                        key_up: KeyCode::KeyW,
                        key_right: KeyCode::KeyD,
                        key_down: KeyCode::KeyS,
                        key_left: KeyCode::KeyA,
                        zoom_sensitivity: 0.25,
                        edge_pan_restrict_to_viewport: true,
                        enabled: player.order == 0,
                        pan_speed: 30.0,
                        ..default()
                    },
                    Camera {
                        is_active: player.order == 0,

                        ..default()
                    },
                    // PanCam {
                    //     enabled: player.order == 0,
                    //     //max_scale: 1.0,
                    //     ..default()
                    // },
                ))
                .id();
            player.camera_entity = Some(camera_entity);

            let mut settlement = commands.spawn((
                Mesh3d(settlement_mesh),
                MeshMaterial3d(player_mat.clone()),
                Transform::from_translation(pos),
                settlment,
            ));
            settlement.observe(click_settlement);
        }
    }

    // Egui camera.
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
    for _turn in turn_start.read() {
        game_state.turn_ready_to_end = false;
    }
}
fn turn_start(
    mut commands: Commands,
    mut cameras: Query<(&mut Camera, Entity, &mut RtsCameraControls), Without<EguiContext>>,
    mut turn_start: MessageReader<TurnStart>,
    mut units: Query<&mut Unit>,
    mut settlements: Query<&mut SettlementCenter>,
    mut selected: ResMut<Selection>,
    highlights: Query<Entity, With<CellHighlight>>,
    game_state: Res<GameState>,
    world_map: Res<WorldMap>,
) {
    for turn in turn_start.read() {
        let player = game_state.players.get(&turn.player).unwrap();
        for (mut camera, entity, mut controls) in cameras.iter_mut() {
            if let Some(player_camera_entity) = player.camera_entity
                && player_camera_entity == entity
            {
                camera.is_active = true;
                controls.enabled = true;
            } else {
                camera.is_active = false;
                controls.enabled = false;
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
        for mut settlement in settlements
            .iter_mut()
            .filter(|s| s.controller == turn.player)
        {
            let production = settlement.production;
            let cell = settlement.cell;
            if let Some(ref mut construction) = settlement.construction {
                match construction {
                    ConstructionJob::Unit(unit_constuction) => {
                        if unit_constuction.add_progress(production) {
                            let neighbours = world_map.get_neighbours(cell);
                            let neighbour = neighbours
                                .iter()
                                .find(|n| !units.iter().any(|u| u.current_cell == **n));
                            if let Some(cell) = neighbour {
                                let pos = world_map.get_position_for_cell(*cell);
                                let mut unit = commands.spawn((
                                    unit_constuction.get_template(*cell),
                                    //MeshMaterial2d(materials.add(player.color)),
                                    Transform::from_translation(pos),
                                ));
                                unit.observe(click_unit);
                                settlement.construction = None;
                            } else {
                                info!("No space for unit!");
                            }
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
    _local: bool,
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
        for (i, civ) in civs.iter().take(player_count).enumerate() {
            let t = i as f32 / (player_count + 1) as f32;
            let color = Color::hsl(360.0 * t, 0.95, 0.7);
            let player = Player {
                order: i,
                id: PlayerId(i),
                _local: true,
                settlement_names: vec![],
                settlement_context: SettlementNameCtx {
                    civilisation_name: civ.to_string(),
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
fn set_unit_next_cell(mut units: Query<&mut Unit>, world_map: Res<WorldMap>) {
    for mut unit in units.iter_mut() {
        if let Some(goal) = unit.goal {
            if unit.current_cell == goal {
                unit.goal = None;
                info!("Unit reached goal");

                continue;
            }
            if unit.next_cell.is_none() {
                let (graph, nodes) = pathfinding::get_graph(&world_map);
                let result = pathfinding::a_star(unit.current_cell, goal, graph, nodes, &world_map);
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
                            unit.used_speed += distance;
                            unit.next_cell = Some(nex_cell);
                            unit.move_timer = Some(Timer::from_seconds(5.0, TimerMode::Once));
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

            let next_cell_pos = world_map.get_position_for_cell(next_cell);
            if move_timer.is_finished() {
                *transform = Transform::from_translation(next_cell_pos);
                unit.current_cell = next_cell;
                unit.next_cell = None;
            } else {
                let current_cell_pos = world_map.get_position_for_cell(CellId(current_cell));
                let new_pos = current_cell_pos.lerp(next_cell_pos, move_timer.fraction());

                *transform = Transform::from_translation(new_pos);
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
            Selection::Settlement(_entity) => {
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
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (graph, nodes) = pathfinding::get_graph(&world_map);
    if let Selection::Unit(unit_entity) = *selected {
        for (e, _highlight) in highlights.iter() {
            let mut e = commands.entity(e);
            e.despawn();
        }
        let unit = units.get(unit_entity).unwrap();
        let start = unit.current_cell;
        let goal = cells.get(event.entity).unwrap().0.cell_id;
        let result = pathfinding::a_star(start, goal, graph, nodes, &world_map);
        if let Some(result) = result {
            for cell_id in result {
                let cell = cells.iter().find(|e| e.0.cell_id == cell_id);
                if let Some((cell, entity)) = cell {
                    let mut e = commands.entity(entity);
                    e.with_child((
                        Mesh3d(cell.outline.clone()),
                        MeshMaterial3d(materials.add(Color::WHITE)),
                        Transform::from_xyz(0.0, 2.0, 0.0),
                        CellHighlight {
                            _unit: Some(unit_entity),
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
    _unit: Option<Entity>,
}

#[derive(Resource)]
enum Selection {
    None,
    Unit(Entity),
    Settlement(Entity),
}

#[derive(Component, Clone)]
struct Unit {
    name: String,
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
    cell: CellId,
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
impl UnitConstuction {
    fn get_template(&self, cell: CellId) -> UnitTemplate {
        UnitTemplate {
            unit: Unit {
                current_cell: cell,
                ..self.template.unit.clone()
            },
            ..self.template.clone()
        }
    }
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
    mesh: Mesh3d,
    material: MeshMaterial3d<StandardMaterial>,
}
#[derive(Clone)]
enum ConstructionJob {
    Unit(UnitConstuction),
}
impl ConstructionJob {
    pub fn progress_label(&self, ui: &mut Ui) {
        match self {
            ConstructionJob::Unit(unit_constuction) => {
                ui.label(format!(
                    "{}: {}/{}",
                    unit_constuction.name,
                    unit_constuction.progress(),
                    unit_constuction.cost()
                ));
            }
        }
    }
    pub fn available_button(&self, ui: &mut Ui, enabled: bool) -> Response {
        match self {
            ConstructionJob::Unit(unit_constuction) => ui.add_enabled(
                enabled,
                egui::widgets::Button::new(format!(
                    "{}: {}",
                    unit_constuction.name, unit_constuction.cost
                )),
            ),
        }
    }
}

/// Build a single mesh: extruded strip + top & bottom caps.
/// `path` is in XZ (Vec2(x, z)), extrusion along +Y by `height`.
pub fn build_extruded_with_caps(
    path: &[Vec2],
    height: f32,
    _world_center: Vec2,
    _map_box: (Vec2, Vec2),
) -> Mesh {
    assert!(path.len() >= 3, "Need at least 3 points for caps");
    //let (min, max) = map_box;
    //let size = max - min;
    let n = path.len();

    // vertex layout:
    // 0 .. 2*n-1      : side vertices (bottom/top columns)
    // 2*n .. 3*n-1    : top cap vertices
    // 3*n .. 4*n-1    : bottom cap vertices
    let side_vert_count = 2 * n;
    let top_cap_vert_base = side_vert_count;
    let bot_cap_vert_base = side_vert_count + n;

    let total_verts = 4 * n;

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(total_verts);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(total_verts);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(total_verts);

    // -----------------
    // 1) SIDE VERTICES
    // -----------------
    for i in 0..n {
        let p = path[i];

        // tangent along path (XZ)
        let dir: Vec2 = if i == 0 {
            (path[1] - path[0]).normalize()
        } else if i == n - 1 {
            (path[i] - path[i - 1]).normalize()
        } else {
            let d1 = (path[i] - path[i - 1]).normalize();
            let d2 = (path[i + 1] - path[i]).normalize();
            (d1 + d2).normalize()
        };

        // outward normal in XZ (left of the path):
        // n2 = (-dz, dx)
        let n2 = Vec2::new(-dir.y, dir.x).normalize();
        let normal3 = [n2.x, 0.0, n2.y];

        // bottom vertex
        positions.push([p.x, 0.0, p.y]);
        normals.push(normal3);
        //let u = i as f32 / (n as f32 - 1.0);
        uvs.push([0.0, 0.0]);

        // top vertex
        positions.push([p.x, height, p.y]);
        normals.push(normal3);
        uvs.push([0.0, 0.0]);
    }

    // -----------------
    // 2) TOP CAP VERTICES (y = height, normal +Y)
    // -----------------
    for p in path.iter().take(n) {
        positions.push([p.x, height, p.y]);
        normals.push([0.0, 1.0, 0.0]);
        // simple planar UV (you can rescale/center as needed)
        // let world_pos = p + world_center;
        // let mut uv = ((world_pos - min) / size) * 4.0;
        // uv = uv.fract_gl();
        uvs.push(p.fract_gl().to_array());
    }

    // -----------------
    // 3) BOTTOM CAP VERTICES (y = 0, normal -Y)
    // -----------------
    for p in path.iter().take(n) {
        positions.push([p.x, 0.0, p.y]);
        normals.push([0.0, -1.0, 0.0]);
        uvs.push([0.0, 0.0]);
    }

    // -----------------
    // INDICES (TRIANGLE LIST)
    // -----------------
    let mut indices: Vec<u32> = Vec::new();

    // 3.1) Side quads -> 2 triangles each
    //
    // side vertex layout:
    //   bottom_i = 2*i
    //   top_i    = 2*i + 1
    //
    // For each segment i..i+1:
    //   tri 1: bottom_i, bottom_{i+1}, top_i
    //   tri 2: bottom_{i+1}, top_{i+1}, top_i
    //
    // This winds CCW when normals point to the chosen outward side.
    for i in 0..(n - 1) {
        let bi = (2 * i) as u32;
        let ti = bi + 1;
        let bi2 = (2 * (i + 1)) as u32;
        let ti2 = bi2 + 1;

        // first triangle
        indices.push(bi);
        indices.push(bi2);
        indices.push(ti);

        // second triangle
        indices.push(bi2);
        indices.push(ti2);
        indices.push(ti);
    }

    // 3.2) Top cap: fan from vertex 0 of the top ring
    //
    // top cap base index offset:
    let top0 = top_cap_vert_base as u32;
    for i in 1..(n - 1) {
        // triangles: (top0, top0 + i, top0 + i + 1)
        indices.push(top0);
        indices.push(top0 + i as u32);
        indices.push(top0 + (i + 1) as u32);
    }

    // 3.3) Bottom cap: same fan but flipped winding
    let bot0 = bot_cap_vert_base as u32;
    for i in 1..(n - 1) {
        // flip to get normal (0, -1, 0)
        indices.push(bot0);
        indices.push(bot0 + (i + 1) as u32);
        indices.push(bot0 + i as u32);
    }

    // -----------------
    // BUILD MESH
    // -----------------
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::all());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));

    mesh
}
pub fn outline_top_from_face(bottom_face: &[(Vec3, Vec2)], height: f32, y_offset: f32) -> Mesh {
    assert!(
        bottom_face.len() >= 3,
        "Need at least 3 vertices for a polygon"
    );

    let n = bottom_face.len();
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(n);
    let mut indices: Vec<u32> = Vec::with_capacity(n * 2);

    // Use the average Y from the bottom face (in case it's not exactly uniform)
    let base_y = bottom_face.iter().map(|(v, _)| v.y).sum::<f32>() / n as f32;
    let top_y = base_y + height + y_offset;

    // Positions: vertices of the top ring (same x,z as the prism top)
    for (v, _) in bottom_face.iter() {
        positions.push([v.x, top_y, v.z]);
    }

    // Edges: connect each vertex to the next, and last to first
    for i in 0..n {
        let next = (i + 1) % n;
        indices.push(i as u32);
        indices.push(next as u32);
    }

    let mut mesh = Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::all());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}
fn extrude_polygon_xz_to_polyline_vertices(polygon_xz: &[Vec2], y0: f32, y1: f32) -> Vec<Vec3> {
    let n = polygon_xz.len();
    if n == 0 {
        return Vec::new();
    }

    let mut verts = Vec::new();

    let b = |i: usize| Vec3::new(polygon_xz[i].x, y0, polygon_xz[i].y);
    let t = |i: usize| Vec3::new(polygon_xz[i].x, y1, polygon_xz[i].y);

    // Start at bottom[0], go up to top[0]
    verts.push(b(0));
    verts.push(t(0));

    // --- Top ring with vertical detours ---
    //
    // We do:
    // t_i -> t_{i+1} (top edge)
    //      -> b_{i+1} -> t_{i+1} (vertical edge down+up)
    //
    // That covers:
    // - all top edges exactly once
    // - all vertical edges at least once
    for i in 0..n {
        let next = (i + 1) % n;

        // top edge: t_i -> t_next
        verts.push(t(next));

        // vertical detour at 'next': t_next -> b_next -> t_next
        verts.push(b(next));
        verts.push(t(next));
    }
    // We are now back at t(0).

    // Go down to b0 (vertical edge 0 again)
    verts.push(b(0));

    // --- Bottom ring ---
    //
    // Walk around the bottom cycle once:
    // b0 -> b1 -> b2 -> ... -> b_{n-1} -> b0
    for i in 0..n {
        let next = (i + 1) % n;
        verts.push(b(next));
    }

    verts
}
