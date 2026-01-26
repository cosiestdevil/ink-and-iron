#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(clippy::too_many_arguments)]
#![forbid(unsafe_code)]
use std::{
    collections::{HashMap, VecDeque}, env, hash::Hash, path::Path, time::Duration
};

use crate::{
    generate::{CellId, WorldMap},
    llm::SettlementNameCtx,
};
use bevy::{
    asset::{AssetLoader, LoadContext, LoadedFolder, io::Reader},
    camera::{
        Exposure,
        visibility::{NoFrustumCulling, RenderLayers},
    },
    ecs::system::SystemState,
    input_focus::InputFocus,
    light::AtmosphereEnvironmentMapLight,
    log::LogPlugin,
    math::bounding::Aabb2d,
    mesh::{Indices, PrimitiveTopology},
    pbr::{Atmosphere, AtmosphereSettings, ScatteringMedium},
    post_process::bloom::Bloom,
    prelude::*,
    window::PrimaryWindow,
};
use bevy_easings::{Ease, EasingsPlugin};
use bevy_egui::{
    EguiContext, EguiContexts, EguiTextureHandle,
    egui::{self, Response, Ui},
};
use bevy_kira_audio::prelude::*;
use bevy_persistent::{Persistent, StorageFormat};
use bevy_prototype_lyon::{
    plugin::ShapePlugin,
    prelude::{ShapeBuilder, ShapeBuilderBase},
};
use bevy_rts_camera::{RtsCamera, RtsCameraControls, RtsCameraPlugin};
use bevy_steamworks::SteamworksPlugin;
use bevy_tokio_tasks::TokioTasksRuntime;
use clap::Parser;
use colorgrad::Gradient;
use geo::{CoordsIter, unary_union};
use num::Num;
use rand::{Rng, SeedableRng, distr::Uniform};
use rand_chacha::ChaCha20Rng;
use serde::Deserialize;
use thiserror::Error;
mod generate;
mod llm;
mod minimap;
mod pathfinding;
#[derive(Parser, Debug)]
struct Args {
    #[arg(long)]
    seed: Option<String>,
    #[arg(long)]
    llm_mode: Option<Option<String>>,
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
pub mod render_layers;
#[derive(Resource)]
struct LlmModeOverride(Option<Option<String>>);
const STEAM_APP_ID: u32 = match u32::from_str_radix(env!("STEAM_APP_ID"), 10) {
    Ok(id) => id,
    Err(_) => panic!(
        "STEAM_APP_ID environment variable not set or invalid. Please set it to your Steam App ID."
    ),
};
fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    #[cfg(not(debug_assertions))]
    if bevy_steamworks::restart_app_if_necessary(STEAM_APP_ID.into()) {
        return Ok(());
    }
    App::new()
        .add_plugins(SteamworksPlugin::init_app(STEAM_APP_ID).unwrap())
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        present_mode: bevy::window::PresentMode::AutoVsync,
                        ..default()
                    }),
                    ..default()
                })
                .set(LogPlugin {
                    custom_layer: logs::custom_layer,
                    ..default()
                }),
        )
        .add_plugins((MeshPickingPlugin, ShapePlugin, AudioPlugin, RtsCameraPlugin))
        .add_plugins(EasingsPlugin::default())
        .add_plugins(bevy_panic_handler::PanicHandler::new().build())
        .add_audio_channel::<Music>()
        .add_plugins(bevy_tokio_tasks::TokioTasksPlugin::default())
        .add_plugins(crate::ui::UIPlugin)
        .add_plugins(crate::generate::WorldPlugin)
        .add_plugins(crate::menu::MenuPlugin)
        .add_message::<TurnStart>()
        .init_asset::<Civilisation>()
        .init_asset_loader::<CivilisationAssetLoader>()
        .init_asset::<LLMProvider>()
        .init_asset_loader::<LLMProviderAssetLoader>()
        .init_state::<AppState>()
        .init_resource::<InputFocus>()
        .init_resource::<crate::pathfinding::PathFinding>()
        .init_resource::<LoadedFolders>()
        .insert_resource(Seed(args.seed.clone()))
        .insert_resource(LlmModeOverride(args.llm_mode))
        //.insert_resource(GameState::new(2))
        .insert_resource::<Random<RandomRng>>(Random(None))
        .insert_resource(Selection::None)
        //.insert_resource::<generate::WorldGenerationParams>((&args).into())
        .add_systems(
            Startup,
            (
                load_settings,
                startup_screens,
                setup_rng,
                archive_old_logs,
                load_civs,
                minimap::setup,
            ),
        )
        .add_systems(
            Update,
            start_background_audio.run_if(resource_added::<Persistent<AudioSettings>>),
        )
        .add_systems(Update, loaded.run_if(in_state(AppState::Loading)))
        .add_systems(OnExit(AppState::Loading), remove_startup_screen)
        .add_systems(
            OnEnter(AppState::InGame),
            (startup, minimap::spawn_minimap_camera),
        )
        .add_systems(
            Update,
            (
                move_unit,
                turn_start_async,
                deselect,
                reset_turn_ready_to_end,
                move_sun,
                debug_notification,
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
#[derive(Resource, Default)]
struct LoadedFolders {
    civs: Option<Handle<LoadedFolder>>,
    llm_providers: Option<Handle<LoadedFolder>>,
}
fn load_civs(asset_server: Res<AssetServer>, mut folders: ResMut<LoadedFolders>) {
    folders.civs = Some(asset_server.load_folder("civilisations"));
    folders.llm_providers = Some(asset_server.load_folder("llm-providers"))
}
fn load_settings(
    mut commands: Commands,
    llm_mode_override: Res<LlmModeOverride>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
) {
    let config_dir = dirs::config_dir().unwrap().join(env!("CARGO_PKG_NAME"));
    commands.insert_resource(
        Persistent::<AudioSettings>::builder()
            .name("audio settings")
            .format(StorageFormat::Toml)
            .path(config_dir.join("audio_settings.toml"))
            .default(AudioSettings::default())
            .build()
            .expect("Failed to init audio settings"),
    );
    let video_settings = Persistent::<VideoSettings>::builder()
        .name("video settings")
        .format(StorageFormat::Toml)
        .path(config_dir.join("video_settings.toml"))
        .default(VideoSettings::default())
        .build()
        .expect("Failed to init video settings");
    window.mode = match video_settings.get().window_mode {
        ::menu::FullscreenMode::Windowed => bevy::window::WindowMode::Windowed,
        ::menu::FullscreenMode::BorderlessFullscreen => {
            bevy::window::WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        }
        ::menu::FullscreenMode::Fullscreen => bevy::window::WindowMode::Fullscreen(
            MonitorSelection::Current,
            VideoModeSelection::Current,
        ),
    };
    commands.insert_resource(video_settings);
    let mut llm_settings = Persistent::<LLMSettings>::builder()
        .name("llm settings")
        .format(StorageFormat::Toml)
        .path(config_dir.join("llm_settings.toml"))
        .default(LLMSettings::default())
        .build()
        .expect("Failed to init llm settings");
    if let Some(llm_override) = &llm_mode_override.0 {
        llm_settings
            .update(|settings| {
                settings.llm_mode = llm_override.clone();
            })
            .expect("Failed to override llm mode from command line");
    }
    commands.insert_resource(llm_settings);
}
fn loaded(
    mut next_state: ResMut<NextState<AppState>>,
    mut timer: Local<Option<Timer>>,
    time: Res<Time>,
    audio_settings: Option<Res<Persistent<AudioSettings>>>,
) {
    let timer = timer.get_or_insert(Timer::new(Duration::from_secs(5), TimerMode::Once));
    timer.tick(time.delta());
    if audio_settings.is_some() && timer.is_finished() {
        next_state.set(AppState::Menu);
    }
}
#[derive(Resource)]
struct Music;
#[derive(Component)]
struct StartupScreen;

#[derive(Resource)]
struct Seed(Option<String>);

#[derive(Resource, serde::Serialize, serde::Deserialize, Clone)]
struct AudioSettings {
    music_volume: f32,
}
#[derive(Resource, serde::Serialize, serde::Deserialize, Clone, Default)]
struct LLMSettings {
    llm_mode: Option<String>,
}
#[derive(Resource, serde::Serialize, serde::Deserialize, Clone)]
struct VideoSettings {
    window_mode: ::menu::FullscreenMode,
}
impl Default for AudioSettings {
    fn default() -> Self {
        AudioSettings { music_volume: 1.0 }
    }
}
impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            window_mode: ::menu::FullscreenMode::Windowed,
        }
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
    audio_settings: Res<Persistent<AudioSettings>>,
) {
    audio
        .play(asset_server.load("sounds/Pixel Kingdom.wav"))
        .fade_in(AudioTween::new(
            Duration::from_secs(2),
            AudioEasing::InPowi(2),
        ))
        .with_volume(volume_from_slider(audio_settings.music_volume))
        .looped();
}
fn volume_from_slider(slider: f32) -> f32 {
    let min_db = Decibels::SILENCE.0;
    let max_db = Decibels::IDENTITY.0;
    let shaped = slider.powf(2.0);
    min_db + (max_db - min_db) * shaped
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
fn move_sun(
    mut light: Query<(&mut Transform, &mut DirectionalLight)>,
    mut time_of_day: Local<Option<Timer>>,
    time: Res<Time>,
) {
    let timer =
        time_of_day.get_or_insert(Timer::new(Duration::from_secs(300), TimerMode::Repeating));
    timer.tick(time.delta());
    let noon = vec3(0.0, 200.0, -200.0);
    let day_t = timer.fraction();
    let sunrise = vec3(-200.0, 0.0, 0.0);
    let sunset = vec3(200.0, 0.0, 0.0);
    let midnight = vec3(0.0, -200.0, 200.0);
    let l0 = 0.33; //  noon -> sunset
    let l1 = 0.165; // sunset -> midnight
    let l2 = 0.165; // midnight -> sunrise
    // ensure they add up to 1.0 exactly
    let l3 = 1.0 - (l0 + l1 + l2); // ~0.33,sunrise -> noon 

    let t = day_t.fract(); // wrap into 0..1

    let pos = if t < l0 {
        // noon → sunset
        let u = t / l0;
        let s = smooth01(u);
        noon.lerp(sunset, s)
    } else if t < l0 + l1 {
        // sunset → midnight
        let u = (t - l0) / l1;
        let s = smooth01(u);
        sunset.lerp(midnight, s)
    } else if t < l0 + l1 + l2 {
        // midnight → sunrise
        let u = (t - (l0 + l1)) / l2;
        let s = smooth01(u);
        midnight.lerp(sunrise, s)
    } else {
        // sunrise → noon
        let u = (t - (l0 + l1 + l2)) / l3;
        let s = smooth01(u);
        sunrise.lerp(noon, s)
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
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: ResMut<AssetServer>,
    mut contexts: EguiContexts,
    mut scattering_mediums: ResMut<Assets<ScatteringMedium>>,
) {
    let scale = world_map.scale;

    let map_box = world_map.bounds();

    for player in game_state.players.values_mut() {
        let mut valid_settlment_cells = world_map.get_valid_settlement_cells();
        if valid_settlment_cells.is_empty() {
            valid_settlment_cells = world_map.iter_cells().map(|c| CellId(c.site())).collect();
        }
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
            //let cell_vertices = world_map.get_vertices_for_cell(cell_id);
            let pos = world_map.get_position_for_cell(cell_id);
            let player_mat = materials.add(player.color);
            let settlement_mesh = meshes.add(Cuboid::from_length(world_map.entity_scale));
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
                turns_till_growth: 1,
                controlled_cells: world_map.get_neighbours(cell_id),
                available_constructions: player
                    .civ
                    .units
                    .iter()
                    .map(|u| {
                        ConstructionJob::Unit(u.to_construction(
                            player.id,
                            cell_id,
                            asset_server.as_ref(),
                            &mut contexts,
                            MeshMaterial3d(player_mat.clone()),
                        ))
                    })
                    .chain(vec![ConstructionJob::Sink(SinkConstuction {
                        cost: 5.0,
                        progress: 0.0,
                    })])
                    .collect::<Vec<_>>(),
            };
            let camera_entity = commands
                .spawn((
                    Camera3d { ..default() },
                    // PanOrbitCamera {
                    //     enabled: player.order == 0,
                    //     ..default()
                    // },
                    Atmosphere::earthlike(scattering_mediums.add(ScatteringMedium::default())),
                    // Can be adjusted to change the scene scale and rendering quality
                    AtmosphereSettings::default(),
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
                    RenderLayers::from_layers(&[render_layers::WORLD]), // PanCam {
                                                                        //     enabled: player.order == 0,
                                                                        //     //max_scale: 1.0,
                                                                        //     ..default()
                                                                        // },
                ))
                .id();
            player.camera_entity = Some(camera_entity);
            let mut controlled_polys = Vec::new();
            for neighbour in settlment.controlled_cells.iter() {
                controlled_polys.push(world_map.get_cell_shape(*neighbour));
            }
            let controlled_vertices = get_hull(controlled_polys, pos.xz(), scale);
            let polygon = bevy_prototype_lyon::prelude::shapes::Polygon {
                points: controlled_vertices.clone(),
                closed: true,
            };

            let mut settlement = commands.spawn((
                Mesh3d(settlement_mesh),
                MeshMaterial3d(player_mat.clone()),
                Transform::from_translation(pos),
                settlment,
                RenderLayers::from_layers(&[render_layers::WORLD]),
            ));
            settlement.observe(settlement_grows);
            settlement.observe(click_settlement);
            let settlement_entity = settlement.id();
            commands.spawn((
                minimap::MinimapControlledArea(settlement_entity),
                ShapeBuilder::with(&polygon).fill(player.color).build(),
                Transform::from_translation(pos.xzy().with_z(4.0)),
                RenderLayers::from_layers(&[render_layers::MINIMAP]),
            ));
            let mut ribbons_vertices = controlled_vertices
                .iter()
                .map(|v| {
                    v.extend(
                        world_map
                            .get_height_at_vertex((*v + pos.xz()) / world_map.scale)
                            .max(0.5)
                            * world_map.height_scale,
                    )
                    .xzy()
                })
                .collect::<Vec<_>>();
            ribbons_vertices.push(*ribbons_vertices.first().unwrap());
            let ribbon_mesh = polyline_ribbon_mesh_3d(&ribbons_vertices, 0.1, Vec3::Y);
            commands.spawn((
                ControlledArea(settlement_entity),
                Mesh3d(meshes.add(ribbon_mesh)),
                MeshMaterial3d(player_mat.clone()),
                NoFrustumCulling,
                RenderLayers::from_layers(&[render_layers::WORLD]),
                Transform::from_translation(pos.with_y(0.01)),
            ));
        }
    }

    // Egui camera.
}
fn get_hull(polys: Vec<geo::Polygon>, offset: Vec2, scale: f32) -> Vec<Vec2> {
    let multi_polygon = unary_union(polys.iter());
    multi_polygon
        .exterior_coords_iter()
        .map(|c| (vec2(c.x as f32, c.y as f32) * scale) - offset)
        .collect::<Vec<_>>()
}
#[derive(EntityEvent)]
struct SettlementGrows {
    #[event_target]
    target_entity: Entity,
}
#[derive(Component)]
pub struct ControlledArea(pub Entity);
fn settlement_grows(
    event: On<SettlementGrows>,
    mut settlements: Query<&mut SettlementCenter>,
    minimap_controlled_areas: Query<(Entity, &minimap::MinimapControlledArea)>,
    controlled_areas: Query<(Entity, &ControlledArea)>,
    world_map: Res<WorldMap>,
    mut commands: Commands,
    pathfinding: Res<crate::pathfinding::PathFinding>,
    game_state: Res<GameState>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let scale = world_map.scale;
    let crate::pathfinding::PathFinding { graph, nodes } = pathfinding.as_ref();
    let entity = event.target_entity;
    let mut settlement = settlements.get_mut(entity).unwrap();

    if settlement.controller != game_state.active_player {
        return;
    }
    let player = game_state.players.get(&settlement.controller).unwrap();
    let pos = world_map.get_position_for_cell(settlement.cell);
    let controlled_cells = &settlement.controlled_cells;
    let mut un_controlled_cells = vec![];
    for cell in controlled_cells.iter() {
        let neighbours = world_map.get_neighbours(*cell);
        let un_controlled_neighbours = neighbours
            .into_iter()
            .filter(|n| !controlled_cells.contains(n) && *n != settlement.cell)
            .collect::<Vec<_>>();
        if un_controlled_neighbours.is_empty() {
            continue;
        }
        un_controlled_cells.extend(un_controlled_neighbours);
    }
    let mut closest_distance = u8::MAX;
    let mut closest_cell = None;
    for cell in un_controlled_cells.iter() {
        let path = crate::pathfinding::a_star(settlement.cell, *cell, graph, nodes, &world_map);
        if path.is_none() {
            continue;
        }
        let path = path.unwrap();
        let distance = path.len() as u8;
        if distance < closest_distance {
            closest_distance = distance;
            closest_cell = Some(*cell);
        }
    }
    if closest_cell.is_none() {
        return;
    }
    let closest_cell = closest_cell.unwrap();
    settlement.controlled_cells.push(closest_cell);
    let minimap_controlled_area_entity = minimap_controlled_areas
        .iter()
        .find(|(_, area)| area.0 == entity)
        .map(|(e, _)| e)
        .unwrap();
    let controlled_area_entity = controlled_areas
        .iter()
        .find(|(_, area)| area.0 == entity)
        .map(|(e, _)| e)
        .unwrap();
    let mut controlled_polys = Vec::new();
    for neighbour in settlement.controlled_cells.iter() {
        controlled_polys.push(world_map.get_cell_shape(*neighbour));
    }
    let controlled_vertices = get_hull(controlled_polys, pos.xz(), scale);
    let polygon = bevy_prototype_lyon::prelude::shapes::Polygon {
        points: controlled_vertices.clone(),
        closed: true,
    };
    let mut ribbons_vertices = controlled_vertices
        .iter()
        .map(|v| {
            v.extend(
                world_map
                    .get_height_at_vertex((*v + pos.xz()) / world_map.scale)
                    .max(0.5)
                    * world_map.height_scale,
            )
            .xzy()
        })
        .collect::<Vec<_>>();
    ribbons_vertices.push(*ribbons_vertices.first().unwrap());
    let outline_mesh = polyline_ribbon_mesh_3d(&ribbons_vertices, 0.1, Vec3::Y);
    commands
        .entity(controlled_area_entity)
        .insert(Mesh3d(meshes.add(outline_mesh)));
    commands
        .entity(minimap_controlled_area_entity)
        .insert(ShapeBuilder::with(&polygon).fill(player.color).build());
}

pub fn polyline_ribbon_mesh_3d(points: &[Vec3], half_width: f32, up: Vec3) -> Mesh {
    assert!(points.len() >= 2);

    // 1) Compute per-point tangents (smoothed using neighbors)
    let mut tangents = Vec::with_capacity(points.len());
    for i in 0..points.len() {
        let prev = if i > 0 { points[i - 1] } else { points[i] };
        let next = if i + 1 < points.len() {
            points[i + 1]
        } else {
            points[i]
        };
        let t = (next - prev).normalize_or_zero();
        tangents.push(if t.length_squared() > 0.0 { t } else { Vec3::Z });
    }

    // 2) Build a stable normal along the curve using parallel transport
    let mut normals = Vec::with_capacity(points.len());

    // Pick an initial reference that isn't parallel to the first tangent
    let t0 = tangents[0];
    let ref_axis = if t0.dot(up).abs() < 0.9 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let mut n = (ref_axis - t0 * t0.dot(ref_axis)).normalize_or_zero();
    if n.length_squared() == 0.0 {
        n = Vec3::X;
    }
    normals.push(n);

    for i in 1..points.len() {
        let t_prev = tangents[i - 1];
        let t_curr = tangents[i];

        let axis = t_prev.cross(t_curr);
        let axis_len2 = axis.length_squared();

        if axis_len2 < 1e-10 {
            // Tangents almost the same -> keep normal
            normals.push(n);
            continue;
        }

        let axis_n = axis / axis_len2.sqrt();
        let dot = t_prev.dot(t_curr).clamp(-1.0, 1.0);
        let angle = dot.acos();

        let q = Quat::from_axis_angle(axis_n, angle);
        n = (q * n).normalize_or_zero();
        normals.push(n);
    }

    // 3) Generate ribbon vertices: p +/- binormal * half_width
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(points.len() * 2);
    let mut mesh_normals: Vec<[f32; 3]> = Vec::with_capacity(points.len() * 2);
    let mut indices: Vec<u32> = Vec::with_capacity((points.len() - 1) * 6);

    for i in 0..points.len() {
        let p = points[i];
        let t = tangents[i];
        let n = normals[i];

        // Width direction
        let mut b = t.cross(n).normalize_or_zero();
        if b.length_squared() == 0.0 {
            // Fallback if something degenerate happened
            b = Vec3::X;
        }

        let left = p + b * half_width;
        let right = p - b * half_width;

        positions.push([left.x, left.y, left.z]);
        positions.push([right.x, right.y, right.z]);

        // Ribbon "faces" toward +n (lighting optional)
        mesh_normals.push([n.x, n.y, n.z]);
        mesh_normals.push([n.x, n.y, n.z]);
    }

    for i in 0..(points.len() - 1) {
        let v0 = (i * 2) as u32;
        let v1 = v0 + 1;
        let v2 = v0 + 2;
        let v3 = v0 + 3;

        indices.extend_from_slice(&[v0, v2, v1]);
        indices.extend_from_slice(&[v2, v3, v1]);
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, mesh_normals);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

fn debug_notification(
    input: Res<ButtonInput<KeyCode>>,
    mut game_state: ResMut<GameState>,
    time: Res<Time>,
) {
    if input.just_pressed(KeyCode::KeyN) {
        let active_player = game_state.active_player;
        let player = game_state.players.get_mut(&active_player).unwrap();
        player.add_notification(format!("This is a test notification! {:?}", time.elapsed()));
    }
}
fn turn_start_async(
    runtime: ResMut<TokioTasksRuntime>,
    world_map: Res<WorldMap>,
    mut turn_start: MessageReader<TurnStart>,
    mut commands: Commands,
    mut cameras: Query<(&mut Camera, Entity, &mut RtsCameraControls), Without<EguiContext>>,
    mut units: Query<&mut Unit>,
    mut settlements: Query<(Entity, &mut SettlementCenter)>,
    mut selected: ResMut<Selection>,
    highlights: Query<Entity, With<CellHighlight>>,
    mut game_state: ResMut<GameState>,
) {
    for turn in turn_start.read() {
        let pathfinding_map = world_map.clone();
        let turn_player = turn.player;
        runtime.spawn_background_task(move |mut ctx| async move {
            let _ = info_span!("turn_start_pathfinding").entered();
            let (graph, nodes) = pathfinding::get_graph(&pathfinding_map);
            ctx.run_on_main_thread(move |ctx| {
                let world = ctx.world;
                let mut system_state =
                    SystemState::<(ResMut<crate::pathfinding::PathFinding>,)>::new(world);
                let (mut pathfinding,) = { system_state.get_mut(world) };
                *pathfinding = crate::pathfinding::PathFinding { graph, nodes };
                system_state.apply(world);
            })
            .await;
        });
        let player = game_state.players.get_mut(&turn_player).unwrap();
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
        info!("Turn started for player: {:?}", turn_player);
        *selected = Selection::None;
        for entity in highlights.iter() {
            let mut highlight = commands.entity(entity);
            highlight.despawn();
        }
        for mut unit in units.iter_mut().filter(|u| u.controller == turn_player) {
            unit.used_speed = 0.0;
        }
        for (entity, mut settlement) in settlements
            .iter_mut()
            .filter(|(_, s)| s.controller == turn_player)
        {
            let production = settlement.production;
            let cell = settlement.cell;
            let mut completed_construction = false;
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
                                let template = unit_constuction.get_template(*cell);
                                let unit_icon = template.unit.icon;
                                let mut unit = commands.spawn((
                                    template,
                                    //MeshMaterial2d(materials.add(player.color)),
                                    Transform::from_translation(pos),
                                ));
                                unit.observe(click_unit);
                                let bark = player
                                    .unit_spawn_barks
                                    .get_mut(&unit_constuction.name)
                                    .and_then(|barks| barks.pop())
                                    .unwrap_or("Unit spawned!".to_string());
                                player.add_notification_with_icon(bark, unit_icon);
                                completed_construction = true;

                                settlement.construction = None;
                            } else {
                                info!("No space for unit!");
                            }
                        }
                    }
                    ConstructionJob::Sink(sink_constuction) => {
                        if sink_constuction.add_progress(production) {
                            completed_construction = true;
                            settlement.construction = None;
                        }
                    }
                }
            }
            if completed_construction {
                for con in settlement.available_constructions.iter_mut() {
                    con.increase();
                }
            }
            settlement.turns_till_growth -= 1;
            if settlement.turns_till_growth == 0 {
                settlement.turns_till_growth = 1;
                commands
                    .entity(entity)
                    .trigger(|e| SettlementGrows { target_entity: e });
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
    unit_spawn_barks: HashMap<String, Vec<String>>,
    notifications: VecDeque<Notification>,
    civ: Civilisation,
}
impl Player {
    fn add_notification(&mut self, message: String) {
        self.notifications.push_back(Notification {
            message,
            icon: None,
            timer: Timer::from_seconds(5.0, TimerMode::Once),
        });
    }
    fn add_notification_with_icon(&mut self, message: String, icon: egui::TextureId) {
        self.notifications.push_back(Notification {
            message,
            icon: Some(icon),
            timer: Timer::from_seconds(5.0, TimerMode::Once),
        });
    }
}
struct Notification {
    pub message: String,
    pub timer: Timer,
    pub icon: Option<egui::TextureId>,
}

#[derive(TypePath, Debug, Deserialize, Clone, Asset)]
struct Civilisation {
    pub name: String,
    pub units: Vec<UnitType>,
    pub description: String,
    pub settlement_name_seeds: Vec<String>,
}
#[derive(Default, TypePath)]
struct CivilisationAssetLoader;

/// Possible errors that can be produced by [`CivilisationAssetLoader`]
#[non_exhaustive]
#[derive(Debug, Error)]
enum CivilisationAssetLoaderError {
    /// An [IO](std::io) Error
    #[error("Could not load asset: {0}")]
    Io(#[from] std::io::Error),
    /// A [RON](ron) Error
    #[error("Could not parse RON: {0}")]
    RonSpannedError(#[from] ron::error::SpannedError),
}

impl AssetLoader for CivilisationAssetLoader {
    type Asset = Civilisation;
    type Settings = ();
    type Error = CivilisationAssetLoaderError;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let custom_asset = ron::de::from_bytes::<Civilisation>(&bytes)?;
        Ok(custom_asset)
    }

    fn extensions(&self) -> &[&str] {
        &["civ.ron"]
    }
}

#[derive(TypePath, Debug, Deserialize, Clone, Asset)]
struct LLMProvider {
    pub name: String,
    pub id: String,
    pub meta: Vec<LibMeta>,
}
#[derive(Default, TypePath)]
struct LLMProviderAssetLoader;
#[non_exhaustive]
#[derive(Debug, Error)]
enum LLMProviderAssetLoaderError {
    /// An [IO](std::io) Error
    #[error("Could not load asset: {0}")]
    Io(#[from] std::io::Error),
    /// A [RON](ron) Error
    #[error("Could not parse RON: {0}")]
    RonSpannedError(#[from] ron::error::SpannedError),
}
impl AssetLoader for LLMProviderAssetLoader {
    type Asset = LLMProvider;
    type Settings = ();
    type Error = LLMProviderAssetLoaderError;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let custom_asset = ron::de::from_bytes::<Self::Asset>(&bytes)?;
        Ok(custom_asset)
    }

    fn extensions(&self) -> &[&str] {
        &["llm.ron"]
    }
}
#[derive(Debug, Deserialize, Clone)]
struct LibMeta {
    pub os: LibOperatingSystem,
    pub path: String,
}
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub enum LibOperatingSystem {
    Windows,
    Linux,
    MacOS,
}
pub const CURRENT_OS: LibOperatingSystem = {
    if cfg!(target_os = "windows") {
        LibOperatingSystem::Windows
    } else if cfg!(target_os = "macos") {
        LibOperatingSystem::MacOS
    } else if cfg!(target_os = "linux") {
        LibOperatingSystem::Linux
    } else {
        panic!("Unknown OS")
    }
};

#[derive(Debug, Deserialize, Clone)]
struct UnitType {
    pub name: String,
    pub default_cost: f32,
    pub health: f32,
    pub range: usize,
    pub speed: f32,
    pub mesh_path: String,
    pub icon_path: String,
    pub seed_barks: Vec<String>,
    pub description: String,
}

impl UnitType {
    fn to_construction(
        &self,
        controller: PlayerId,
        cell: CellId,
        asset_server: &AssetServer,
        contexts: &mut EguiContexts,
        material: MeshMaterial3d<StandardMaterial>,
    ) -> UnitConstuction {
        UnitConstuction {
            cost: self.default_cost,
            progress: 0.0,
            template: Box::new(UnitTemplate {
                unit: Unit {
                    name: self.name.clone(),
                    max_health: self.health,
                    health: self.health,
                    range: self.range,
                    controller,
                    speed: self.speed,
                    used_speed: 0.0,
                    current_cell: cell,
                    next_cell: None,
                    goal: None,
                    move_timer: None,
                    icon: contexts.add_image(EguiTextureHandle::Strong(
                        asset_server.load(self.icon_path.clone()),
                    )),
                },
                mesh: Mesh3d(
                    asset_server.load(
                        GltfAssetLabel::Primitive {
                            mesh: 0,
                            primitive: 0,
                        }
                        .from_asset(self.mesh_path.clone()),
                    ),
                ),
                material,
            }),
            name: self.name.clone(),
        }
    }
}
#[derive(Resource)]
struct GameState {
    players: HashMap<PlayerId, Player>,
    active_player: PlayerId,
    turn_ready_to_end: bool,
}
impl GameState {
    fn new(
        player_count: usize,
        selected_civs: &mut [Option<AssetId<Civilisation>>],
        civs: &Assets<Civilisation>,
    ) -> Self {
        let mut players = HashMap::with_capacity(player_count);
        for i in 0..player_count {
            let civ = if let Some(civ_id) = selected_civs.get(i).and_then(|c| c.as_ref()) {
                civs.get(*civ_id).unwrap().clone()
            } else {
                let civs_vec = civs.iter().map(|(_, c)| c.clone()).collect::<Vec<_>>();
                let mut rng = ChaCha20Rng::from_os_rng();
                let civ_i = rng.sample(Uniform::new(0, civs_vec.len()).unwrap());
                civs_vec.get(civ_i).unwrap().clone()
            };
            let t = i as f32 / (player_count + 1) as f32;
            let color = Color::hsl(360.0 * t, 0.95, 0.7);
            let player = Player {
                order: i,
                id: PlayerId(i),
                _local: true,
                settlement_names: vec![],
                settlement_context: SettlementNameCtx {
                    civilisation_name: civ.name.to_string(),
                    description: civ.description.to_string(),
                    seed_names: civ.settlement_name_seeds.clone(),
                },
                civ: civ.clone(),
                camera_entity: None,
                color,
                unit_spawn_barks: HashMap::new(),
                notifications: VecDeque::new(),
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
fn set_unit_next_cell(
    mut units: Query<&mut Unit>,
    world_map: Res<WorldMap>,
    pathfinding: Res<crate::pathfinding::PathFinding>,
) {
    for mut unit in units.iter_mut() {
        if let Some(goal) = unit.goal {
            if unit.current_cell == goal {
                unit.goal = None;
                info!("Unit reached goal");

                continue;
            }
            if unit.next_cell.is_none() {
                let crate::pathfinding::PathFinding { graph, nodes } = pathfinding.as_ref();
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
fn click_unit(
    mut event: On<Pointer<Click>>,
    mut commands: Commands,
    mut selection: ResMut<Selection>,
    mut units: Query<&mut Unit>,
    mut random: ResMut<Random<RandomRng>>,
    game_state: Res<GameState>,
    world_map: Res<WorldMap>,
    pathfinding: Res<crate::pathfinding::PathFinding>,
) {
    if event.button == PointerButton::Primary {
        let controller = units.get(event.entity).unwrap().controller;
        if controller == game_state.active_player {
            *selection = Selection::Unit(event.entity);
        } else {
            match *selection {
                Selection::None => {}
                Selection::Unit(entity) => {
                    let [mut attacker, mut defender] =
                        units.get_many_mut([entity, event.entity]).unwrap();
                    let crate::pathfinding::PathFinding { graph, nodes } = pathfinding.as_ref();
                    let result = pathfinding::a_star(
                        attacker.current_cell,
                        defender.current_cell,
                        graph,
                        nodes,
                        &world_map,
                    );
                    if let Some(result) = result {
                        let distance = result.len() - 1;
                        if distance <= attacker.range {
                            let damage = random
                                .0
                                .as_mut()
                                .unwrap()
                                .sample(Uniform::new(1.0, 3.0).unwrap());
                            defender.health -= damage;
                            if defender.health <= 0.0 {
                                commands.entity(event.entity).despawn();
                            }
                        }
                        if distance <= defender.range {
                            let damage = random
                                .0
                                .as_mut()
                                .unwrap()
                                .sample(Uniform::new(0.5, 1.5).unwrap());
                            attacker.health -= damage;
                            if attacker.health <= 0.0 {
                                commands.entity(entity).despawn();
                            }
                        }
                    }
                }
                Selection::Settlement(_entity) => {}
            }
        }
        event.propagate(false);
    }
}
fn click_settlement(
    mut event: On<Pointer<Click>>,
    mut selected_unit: ResMut<Selection>,
    settlements: Query<&SettlementCenter>,
    game_state: Res<GameState>,
) {
    if event.button == PointerButton::Primary {
        let settlement = settlements.get(event.entity).unwrap();
        if settlement.controller == game_state.active_player {
            *selected_unit = Selection::Settlement(event.entity);
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

#[derive(Component, Clone, Debug)]
struct Unit {
    name: String,
    max_health: f32,
    health: f32,
    range: usize,
    controller: PlayerId,
    speed: f32,
    used_speed: f32,
    current_cell: CellId,
    next_cell: Option<CellId>,
    goal: Option<CellId>,
    move_timer: Option<Timer>,
    icon: egui::TextureId,
}

#[derive(Component)]
struct SettlementCenter {
    cell: CellId,
    controller: PlayerId,
    construction: Option<ConstructionJob>,
    production: f32,
    available_constructions: Vec<ConstructionJob>,
    name: String,
    controlled_cells: Vec<CellId>,
    turns_till_growth: u8,
}
trait Construction {
    fn add_progress(&mut self, progress: f32) -> bool;
    fn cost(&self) -> f32;
    fn progress(&self) -> f32;
}

#[derive(Debug, Clone)]
struct UnitConstuction {
    cost: f32,
    progress: f32,
    template: Box<UnitTemplate>,
    name: String,
}
impl UnitConstuction {
    fn get_template(&self, cell: CellId) -> UnitTemplate {
        UnitTemplate {
            unit: Unit {
                current_cell: cell,
                ..self.template.unit.clone()
            },
            ..*self.template.clone()
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
#[derive(Clone)]
struct SinkConstuction {
    cost: f32,
    progress: f32,
}
impl Construction for SinkConstuction {
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
#[derive(Bundle, Clone, Debug)]
struct UnitTemplate {
    unit: Unit,
    mesh: Mesh3d,
    material: MeshMaterial3d<StandardMaterial>,
}
#[derive(Clone)]
enum ConstructionJob {
    Unit(UnitConstuction),
    Sink(SinkConstuction),
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
            ConstructionJob::Sink(sink) => {
                ui.label(format!("Sink: {}/{}", sink.progress(), sink.cost()));
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
            ConstructionJob::Sink(sink) => ui.add_enabled(
                enabled,
                egui::widgets::Button::new(format!("Sink: {}", sink.cost)),
            ),
        }
    }
    pub fn increase(&mut self) {
        match self {
            ConstructionJob::Unit(unit_constuction) => {
                unit_constuction.cost = unit_constuction.cost.powf(1.5);
            }
            ConstructionJob::Sink(sink_constuction) => {
                sink_constuction.cost = sink_constuction.cost.powf(1.5)
            }
        }
    }
}
