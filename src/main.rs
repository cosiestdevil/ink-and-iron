#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(clippy::too_many_arguments)]
use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    time::Duration,
};

use crate::{
    generate::{CellId, WorldMap, WorldType},
    llm::SettlementNameCtx,
};
use bevy::{
    asset::{AssetLoader, LoadContext, LoadedFolder, io::Reader, ron},
    camera::Exposure,
    input_focus::InputFocus,
    light::AtmosphereEnvironmentMapLight,
    log::LogPlugin,
    math::bounding::Aabb2d,
    pbr::Atmosphere,
    post_process::bloom::Bloom,
    prelude::*,
};
use bevy_easings::{Ease, EasingsPlugin};
use bevy_egui::{
    EguiContext, EguiContexts, EguiTextureHandle,
    egui::{self, Response, Ui},
};
use bevy_kira_audio::prelude::*;
use bevy_persistent::{Persistent, StorageFormat};
use bevy_prototype_lyon::plugin::ShapePlugin;
use bevy_rts_camera::{RtsCamera, RtsCameraControls, RtsCameraPlugin};
use bevy_tokio_tasks::TokioTasksRuntime;
use clap::Parser;
use colorgrad::Gradient;
use num::Num;
use rand::{Rng, SeedableRng, distr::Uniform};
use rand_chacha::ChaCha20Rng;
use serde::Deserialize;
use thiserror::Error;
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
    #[arg(long, default_value_t = WorldType::Default)]
    world_type: WorldType,
    #[arg(long, default_value_t = false)]
    llm_cpu: bool,
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
#[derive(Resource)]
struct LlmCpu(bool);
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
            bevy_panic_handler::PanicHandler::new().build(),
        ))
        .add_audio_channel::<Music>()
        .add_plugins(bevy_tokio_tasks::TokioTasksPlugin::default())
        .add_plugins(crate::ui::UIPlugin)
        .add_plugins(crate::generate::WorldPlugin)
        .add_plugins(crate::menu::MenuPlugin)
        .add_message::<TurnStart>()
        .init_asset::<Civilisation>()
        .init_asset_loader::<CivilisationAssetLoader>()
        .init_state::<AppState>()
        .init_resource::<InputFocus>()
        .init_resource::<crate::pathfinding::PathFinding>()
        .init_resource::<LoadedFolders>()
        .insert_resource(LlmCpu(args.llm_cpu))
        .insert_resource(Seed(args.seed.clone()))
        //.insert_resource(GameState::new(2))
        .insert_resource::<Random<RandomRng>>(Random(None))
        .insert_resource(Selection::None)
        .insert_resource::<generate::WorldGenerationParams>((&args).into())
        .add_systems(
            Startup,
            (
                load_settings,
                startup_screens,
                setup_rng,
                archive_old_logs,
                load_civs,
            ),
        )
        .add_systems(
            Update,
            start_background_audio.run_if(resource_added::<Persistent<AudioSettings>>),
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
#[derive(Resource,Default)]
struct LoadedFolders{
    civs:Option<Handle<LoadedFolder>>
}
fn load_civs(asset_server:Res<AssetServer>,mut folders:ResMut<LoadedFolders>){
    folders.civs = Some(asset_server.load_folder("civilisations"));
}
fn load_settings(mut commands: Commands) {
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
) {
    let scale = world_map.scale;

    let map_box = world_map.bounds();

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
            // let unit_mesh = meshes.add(Cylinder::new(
            //     world_map.entity_scale,
            //     world_map.entity_scale,
            // ));
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
                    .chain(vec![
                        //     ConstructionJob::Unit(UnitConstuction {
                        //         name: "Fighter".to_string(),
                        //         cost: 2.0,
                        //         progress: 0.0,
                        //         template: Box::new(UnitTemplate {
                        //             mesh: Mesh3d(unit_mesh),
                        //             material: MeshMaterial3d(player_mat.clone()),
                        //             unit: Unit {
                        //                 name: "Fighter".to_string(),
                        //                 max_health: 10.0,
                        //                 health: 10.0,
                        //                 range: 1,
                        //                 speed: 5.0,
                        //                 used_speed: 0.0,
                        //                 current_cell: cell_id,
                        //                 next_cell: None,
                        //                 goal: None,
                        //                 move_timer: None,
                        //                 controller: player.id,
                        //                 icon: contexts.add_image(EguiTextureHandle::Strong(
                        //                     asset_server.load("icons/fighter.png"),
                        //                 )),
                        //             },
                        //         }),
                        //     }),
                        ConstructionJob::Sink(SinkConstuction {
                            cost: 5.0,
                            progress: 0.0,
                        }),
                    ])
                    .collect::<Vec<_>>(),
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

fn turn_start(
    mut commands: Commands,
    mut cameras: Query<(&mut Camera, Entity, &mut RtsCameraControls), Without<EguiContext>>,
    mut turn_start: MessageReader<TurnStart>,
    mut units: Query<&mut Unit>,
    mut settlements: Query<&mut SettlementCenter>,
    mut selected: ResMut<Selection>,
    highlights: Query<Entity, With<CellHighlight>>,
    mut game_state: ResMut<GameState>,
    world_map: Res<WorldMap>,
    mut pathfinding: ResMut<crate::pathfinding::PathFinding>,
) {
    let (graph, nodes) = pathfinding::get_graph(&world_map);
    *pathfinding = pathfinding::PathFinding { graph, nodes };
    for turn in turn_start.read() {
        let player = game_state.players.get_mut(&turn.player).unwrap();
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
                                    .pop()
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
    unit_spawn_barks: Vec<String>,
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
    pub settlement_name_seeds: Vec<String>,
}
#[derive(Default)]
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

#[derive(Debug, Deserialize, Clone)]
struct UnitType {
    pub name: String,
    pub default_cost: f32,
    pub health: f32,
    pub range: usize,
    pub speed: f32,
    pub mesh_path: String,
    pub icon_path: String,
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
    fn new(player_count: usize, civs: Vec<Civilisation>) -> Self {
        let mut players = HashMap::with_capacity(player_count);
        for (i, civ) in civs.iter().take(player_count).enumerate() {
            let t = i as f32 / (player_count + 1) as f32;
            let color = Color::hsl(360.0 * t, 0.95, 0.7);
            let player = Player {
                order: i,
                id: PlayerId(i),
                _local: true,
                settlement_names: vec![],
                settlement_context: SettlementNameCtx {
                    civilisation_name: civ.name.to_string(),
                    seed_names: civ.settlement_name_seeds.clone(),
                },
                civ: civ.clone(),
                camera_entity: None,
                color,
                unit_spawn_barks: vec![],
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
                            defender.health -= 1.0;
                            if defender.health <= 0.0 {
                                commands.entity(event.entity).despawn();
                            }
                        }
                        if distance <= defender.range {
                            attacker.health -= 1.0;
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
