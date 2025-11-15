#![allow(clippy::too_many_arguments)]
use std::{collections::HashMap, f32::consts::PI, sync::OnceLock, time::Duration};

use crate::{
    generate::{CellId, WorldMap},
    llm::SettlementNameCtx,
    pathfinding::ToVec2,
};
use bevy::{
    asset::RenderAssetUsages,
    camera::{Viewport, visibility::RenderLayers},
    color::palettes::css::BLACK,
    ecs::system::SystemState,
    log::{BoxedLayer, LogPlugin, tracing_subscriber::Layer},
    math::VectorSpace,
    mesh::{CuboidMeshBuilder, Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::BlendState,
    window::PrimaryWindow,
};
use bevy_easings::{Ease, EasingsPlugin};
use bevy_egui::{
    EguiContext, EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass,
    PrimaryEguiContext,
    egui::{self, Response, Ui},
};
use bevy_kira_audio::prelude::*;
use bevy_pancam::{PanCam, PanCamPlugin};
use bevy_panorbit_camera::{PanOrbitCamera, PanOrbitCameraPlugin};
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
//use bevy_top_down_camera::*;
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
use tracing_appender::{non_blocking::WorkerGuard, rolling};
static LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

fn custom_layer(_app: &mut App) -> Option<BoxedLayer> {
    let file_appender = rolling::daily("logs", "app.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let _ = LOG_GUARD.set(guard);
    Some(
        bevy::log::tracing_subscriber::fmt::layer()
            .with_writer(non_blocking)
            .with_file(true)
            .with_line_number(true)
            .boxed(),
    )
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
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        present_mode: bevy::window::PresentMode::AutoNoVsync,
                        ..default()
                    }),
                    ..default()
                })
                .set(LogPlugin {
                    custom_layer,
                    ..default()
                }),
            //PanCamPlugin,
            MeshPickingPlugin,
            ShapePlugin,
            AudioPlugin,
            EasingsPlugin::default(),
        ))
        .add_plugins(bevy_tokio_tasks::TokioTasksPlugin::default())
        .add_plugins(EguiPlugin::default())
        .add_plugins(PanOrbitCameraPlugin)
        .add_message::<TurnStart>()
        .init_state::<AppState>()
        .insert_resource(GameState::new(1))
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
        .add_systems(
            EguiPrimaryContextPass,
            ui_example_system.run_if(in_state(AppState::InGame)),
        )
        .run();
    Ok(())
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
fn generate_settlement_name(
    mut rng: ResMut<Random<ChaCha20Rng>>,
    runtime: ResMut<TokioTasksRuntime>,
    game_state: Res<GameState>,
) {
    let temp = rng.0.random_range(0.3..0.5);
    for player in game_state.players.values() {
        let civ_name = player.settlement_context.civilisation_name.clone();
        let player_id = player.id;
        runtime.spawn_background_task(move |mut ctx| async move {
            if let Ok(names) = llm::settlement_names(
                SettlementNameCtx {
                    civilisation_name: civ_name,
                },
                temp,
            )
            .await
            {
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
fn min_max_componentwise<I>(mut iter: I) -> Option<(Vec2, Vec2)>
where
    I: Iterator<Item = Vec2>,
{
    let first = iter.next()?; // early-return None if empty

    let (min, max) = iter.fold((first, first), |(min, max), v| (min.min(v), max.max(v)));

    Some((min, max))
}

fn smooth01(t: f32) -> f32 {
    // standard smoothstep from 0..1
    return t * t * (3.0 - 2.0 * t);
}
fn move_sun(mut light: Query<(&mut Transform, &mut DirectionalLight)>, time: Res<Time>) {
    let noon = vec3(0.0, 200.0, -200.0);
    let day_t = (time.elapsed_secs() % 300.0) / 300.0;
    let sunrise = vec3(-200.0, 0.0, 0.0);
    let sunset = vec3(200.0, 0.0, 0.0);
    let midnight = vec3(0.0, -200.0, 200.0);
     let l0 = 0.165; // midnight -> sunrise
    let l1 = 0.33;  // sunrise -> noon
    let l2 = 0.33;  // noon -> sunset
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
    let map_box = min_max_componentwise(
        world_map
            .voronoi
            .iter_cells()
            .map(|c| c.site_position().to_vec2() * scale),
    )
    .unwrap();
    let mut height_material_cache = HashMap::<u8, Handle<StandardMaterial>>::new();
    let mut height_ocean_material_cache = HashMap::<u8, Handle<StandardMaterial>>::new();
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
        let mut height = *(world_map.cell_height.get(&CellId(v_cell.site())).unwrap());
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

        let scaled_height = height * scale * 0.25;
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
            ));
        }
        let line = Polyline3d::new(extrude_polygon_xz_to_polyline_vertices(
            &vertices,
            0.0,
            scaled_height,
        ));
        //let outline_mesh = meshes.add(outline_top_from_face(&vertices3, height * scale, 0.001));
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
            children![(
                Mesh3d(outline_mesh),
                MeshMaterial3d(outline_material.clone()),
                Transform::IDENTITY
            )],
        ));
        cell.observe(click_cell).observe(over_cell);
    }

    commands.spawn((
        DirectionalLight::default(),
        Transform::from_xyz(0.0, 200.0, -200.0).looking_at(vec3(0.0, 0.0, 0.0), Vec3::Y),
    ));
    for player in game_state.players.values_mut() {
        let mut pos = random.0.sample(
            Uniform::<Vec2>::new(Vec2::ZERO, Vec2::new(16.0 * scale, 9.0 * scale)).unwrap(),
        );
        let cell_id = world_map.get_cell_for_position(pos);
        if let Some(cell_id) = cell_id {
            pos = world_map.voronoi.cell(cell_id.0).site_position().to_vec2() * scale;

            let settlement_mesh = meshes.add(Cuboid::from_length(scale * 0.025));
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
            let settlment = SettlementCenter {
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
                    Camera3d { ..default() },
                    PanOrbitCamera {
                        enabled: player.order == 0,
                        ..default()
                    },
                    Transform::from_translation(pos.extend(20.0 * scale).xzy())
                        .looking_at(pos.extend(0.0).xzy(), Vec3::Y),
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
            let v_cell = world_map.voronoi.cell(cell_id.0);
            let height =
                *(world_map.cell_height.get(&CellId(v_cell.site())).unwrap()) * (scale * 0.25);
            let mut settlement = commands.spawn((
                Mesh3d(settlement_mesh),
                MeshMaterial3d(materials.add(player.color)),
                Transform::from_translation(pos.extend(height + (scale * 0.025)).xzy()),
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
    mut game_state: ResMut<GameState>,
    selected: Res<Selection>,
    mut settlements: Query<&mut SettlementCenter>,
    mut units: Query<&mut Unit>,
    mut turn_start: MessageWriter<TurnStart>,
) -> Result {
    let ctx = contexts.ctx_mut()?;

    let mut left = egui::SidePanel::left("left_panel")
        .resizable(true)
        .show(ctx, |ui| {
            ui.label("Left resizeable panel");
            match *selected {
                Selection::None => {}
                Selection::Unit(_entity) => {}
                Selection::Settlement(entity) => {
                    let mut settlement = settlements.get_mut(entity).unwrap();
                    ui.label(settlement.name.clone());
                    if let Some(ref job) = settlement.construction {
                        job.progress_label(ui);
                    } else {
                        ui.label("No Construction Queued");
                    }
                    for job in settlement.available_constructions.clone().iter() {
                        if job.available_button(ui, true).clicked() {
                            settlement.construction = Some(job.clone());
                        }
                    }
                }
            }
            ui.allocate_rect(ui.available_rect_before_wrap(), egui::Sense::hover());
        })
        .response
        .rect
        .width(); // height is ignored, as the panel has a hight of 100% of the screen
    let right = 0;
    // let mut right = egui::SidePanel::right("right_panel")
    //     .resizable(true)
    //     .show(ctx, |ui| {
    //         ui.label("Right resizeable panel");

    //         ui.allocate_rect(ui.available_rect_before_wrap(), egui::Sense::hover());
    //     })
    //     .response
    //     .rect
    //     .width(); // height is ignored, as the panel has a height of 100% of the screen
    let top = 0;
    // let mut top = egui::TopBottomPanel::top("top_panel")
    //     .resizable(true)
    //     .show(ctx, |ui| {
    //         ui.label("Top resizeable panel");
    //         ui.allocate_rect(ui.available_rect_before_wrap(), egui::Sense::hover());
    //     })
    //     .response
    //     .rect
    //     .height(); // width is ignored, as the panel has a width of 100% of the screen
    let mut bottom = egui::TopBottomPanel::bottom("bottom_panel")
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                match *selected {
                    Selection::None => {}
                    Selection::Unit(entity) => {
                        let unit = units.get_mut(entity).unwrap();
                        ui.label(format!("Unit"));
                        ui.label(format!("Speed: {}/{}", unit.used_speed, unit.speed));
                    }
                    Selection::Settlement(_entity) => {}
                }
                ui.separator();
                let avail = ui.available_size_before_wrap();
                ui.allocate_ui_with_layout(
                    avail,
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        // These will appear stuck to the right edge:
                        if ui
                            .add_enabled(
                                game_state.turn_ready_to_end,
                                egui::widgets::Button::new("Next Turn"),
                            )
                            .clicked()
                        {
                            let current_player = game_state.active_player;
                            let current_player = game_state.players.get(&current_player);
                            if let Some(current_player) = current_player {
                                let next_player = game_state.players.values().find(|p| {
                                    p.order == (current_player.order + 1) % game_state.players.len()
                                });
                                if let Some(next_player) = next_player {
                                    turn_start.write(TurnStart {
                                        player: next_player.id,
                                    });
                                    game_state.active_player = next_player.id;
                                }
                            }
                        }
                        ui.separator();
                        ui.label("v1.2.3");
                    },
                );
            });
            ui.allocate_rect(ui.available_rect_before_wrap(), egui::Sense::hover());
        })
        .response
        .rect
        .height(); // width is ignored, as the panel has a width of 100% of the screen

    // Scale from logical units to physical units.
    left *= window.scale_factor();
    //right *= window.scale_factor();
    //top *= window.scale_factor();
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
    let active_player = game_state.active_player;
    let player = game_state.players.get(&active_player).unwrap();
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
    for _turn in turn_start.read() {
        game_state.turn_ready_to_end = false;
    }
}
fn turn_start(
    mut commands: Commands,
    mut cameras: Query<(&mut Camera, &mut PanCam, Entity), Without<EguiContext>>,
    mut turn_start: MessageReader<TurnStart>,
    mut units: Query<&mut Unit>,
    mut settlements: Query<(&mut SettlementCenter, &Transform)>,
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
        for (i, civ) in civs.iter().take(player_count).enumerate() {
            let t = i as f32 / (player_count + 1) as f32;
            let color = Color::hsl(360.0 * t, 0.95, 0.7);
            let player = Player {
                order: i,
                id: PlayerId(i),
                local: true,
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
    world_map: Res<WorldMap>,
) {
    if event.button == PointerButton::Primary {
        if let Ok(cell) = cells.get(event.entity) {
            let height = world_map.cell_height.get(&cell.cell_id).unwrap();

            info!("Height: {height}");
        }
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
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let (graph, nodes) =
        pathfinding::get_graph(world_map.voronoi.clone(), world_map.cell_height.clone());
    if let Selection::Unit(unit_entity) = *selected {
        for (e, _highlight) in highlights.iter() {
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
    pub fn progress_label(&self, ui: &mut Ui) {
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

/// Create a prism mesh from a list of 3D vertices defining the *bottom* face of the prism.
/// All vertices should share the same Y coordinate (e.g. 0.0).
pub fn prism_from_face(bottom_face: &[(Vec3, Vec2)], height: f32) -> Mesh {
    assert!(
        bottom_face.len() >= 3,
        "Need at least 3 vertices for a polygon"
    );
    let n = bottom_face.len();
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(n * 2);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(n * 2);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(n * 2);
    let mut indices: Vec<u32> = Vec::new();
    // --- Build vertices: bottom ring then top ring ---
    // Bottom vertices (as given)
    for (v, uv) in bottom_face.iter() {
        positions.push([v.x, v.y, v.z]);
        // Rough normal for bottom face (pointing downwards)
        normals.push([0.0, -1.0, 0.0]);
        uvs.push([v.x, v.z]);
        // simple planar UV, tweak as you like
    }
    // Top vertices (extruded by height along +Y)
    for (v, uv) in bottom_face.iter() {
        positions.push([v.x, v.y + height, v.z]);
        // Rough normal for top face (pointing up)
        normals.push([0.0, 1.0, 0.0]);
        uvs.push([v.x, v.z]);
    }
    // ------------- SIDE VERTICES (separate from caps) -------------

    // Compute smooth side normals per vertex (average of adjacent edge normals)
    let mut side_normals: Vec<Vec3> = vec![Vec3::ZERO; n];

    for i in 0..n {
        let (v_i, _) = bottom_face[i];
        let (v_next, _) = bottom_face[(i + 1) % n];

        let edge = v_next - v_i;
        // "up" is +Y; cross to get outward normal.
        // Depending on your winding (CW/CCW), you may need edge.cross(Vec3::Y) instead.
        let face_normal = Vec3::Y.cross(edge).normalize_or_zero();

        side_normals[i] += face_normal;
        side_normals[(i + 1) % n] += face_normal;
    }

    for nrm in &mut side_normals {
        *nrm = nrm.normalize_or_zero();
    }

    // Compute a simple "u" coordinate along the perimeter for side UVs
    let mut edge_lengths = vec![0.0_f32; n + 1];
    let mut perimeter = 0.0_f32;

    for i in 0..n {
        let (v_i, _) = bottom_face[i];
        let (v_next, _) = bottom_face[(i + 1) % n];
        perimeter += (v_next - v_i).length();
        edge_lengths[i + 1] = perimeter;
    }

    // Side bottom ring: indices [2n .. 3n)
    let side_bottom_offset = positions.len() as u32;
    for i in 0..n {
        let (v, _) = bottom_face[i];
        let t = if perimeter > 0.0 {
            edge_lengths[i] / perimeter
        } else {
            0.0
        };
        let nrm = side_normals[i];

        positions.push([v.x, v.y, v.z]);
        normals.push([nrm.x, nrm.y, nrm.z]);
        uvs.push([t, 0.0]); // v=0 at bottom
    }

    // Side top ring: indices [3n .. 4n)
    let side_top_offset = positions.len() as u32;
    for i in 0..n {
        let (v, _) = bottom_face[i];
        let t = if perimeter > 0.0 {
            edge_lengths[i] / perimeter
        } else {
            0.0
        };
        let nrm = side_normals[i];

        positions.push([v.x, v.y + height, v.z]);
        normals.push([nrm.x, nrm.y, nrm.z]);
        uvs.push([t, 1.0]); // v=1 at top
    }

    // --- Faces ---
    // 1. Bottom face: fan triangulation (0 is the center vertex of the fan)
    // Assumes a convex polygon and properly ordered vertices.
    for i in 1..(n - 1) {
        indices.push(0 as u32);
        indices.push((i + 1) as u32);
        indices.push(i as u32);
    }
    // 2. Top face: same fan but on the top ring, with reversed winding
    // Top ring starts at index n
    let top_offset = n as u32;
    for i in 1..(n - 1) {
        indices.push(top_offset);
        // center
        indices.push(top_offset + i as u32);
        indices.push(top_offset + (i + 1) as u32);
    }
    // 3. Side faces: quads split into two triangles per edge
    // For each edge (i -> next) on the polygon:
    for i in 0..n {
        let next = (i + 1) % n;
        let bi = i as u32;
        // bottom i
        let bn = next as u32;
        // bottom next
        let ti = top_offset + i as u32;
        // top i
        let tn = top_offset + next as u32;
        // top next
        // Side quad as two triangles.
        // Adjust order if your normals appear inverted.
        indices.extend_from_slice(&[
            bi, bn, tn, // first triangle
            bi, tn, ti,
            // second triangle
        ]);
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::all());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}
pub fn build_strip_mesh(path: &[Vec2], height: f32) -> Mesh {
    assert!(path.len() >= 2, "Need at least 2 points for a strip");

    let n = path.len();
    let vert_count = n * 2;

    // Bevy expects Vec<[f32; N]> for attributes
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(vert_count);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(vert_count);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(vert_count);

    // --- build vertices & normals ---
    for i in 0..n {
        let p = path[i];

        // 1) tangent along path (in XZ as Vec2)
        let dir: Vec2 = if i == 0 {
            (path[1] - path[0]).normalize()
        } else if i == n - 1 {
            (path[i] - path[i - 1]).normalize()
        } else {
            let d1 = (path[i] - path[i - 1]).normalize();
            let d2 = (path[i + 1] - path[i]).normalize();
            (d1 + d2).normalize()
        };

        // 2) left-hand normal in XZ plane: (-dz, dx)
        //    If you want the other side, just negate this.
        let n2 = Vec2::new(-dir.y, dir.x).normalize();

        let normal3 = [n2.x, 0.0, n2.y];

        // bottom vertex (y = 0)
        positions.push([p.x, 0.0, p.y]);
        normals.push(normal3);
        // simple UVs: u along path, v along height
        let u = i as f32 / (n as f32 - 1.0);
        uvs.push([u, 0.0]);

        // top vertex (y = height)
        positions.push([p.x, height, p.y]);
        normals.push(normal3);
        uvs.push([u, 1.0]);
    }

    // --- triangle strip indices: 0,1,2,3,... ---
    let mut indices: Vec<u32> = Vec::with_capacity(vert_count);
    for i in 0..vert_count {
        indices.push(i as u32);
    }

    // --- build mesh ---
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleStrip, RenderAssetUsages::all());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));

    mesh
}

/// Build a single mesh: extruded strip + top & bottom caps.
/// `path` is in XZ (Vec2(x, z)), extrusion along +Y by `height`.
pub fn build_extruded_with_caps(
    path: &[Vec2],
    height: f32,
    world_center: Vec2,
    map_box: (Vec2, Vec2),
) -> Mesh {
    assert!(path.len() >= 3, "Need at least 3 points for caps");
    let (min, max) = map_box;
    let size = max - min;
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
        let u = i as f32 / (n as f32 - 1.0);
        uvs.push([0.0, 0.0]);

        // top vertex
        positions.push([p.x, height, p.y]);
        normals.push(normal3);
        uvs.push([0.0, 0.0]);
    }

    // -----------------
    // 2) TOP CAP VERTICES (y = height, normal +Y)
    // -----------------
    for i in 0..n {
        let p = path[i];
        positions.push([p.x, height, p.y]);
        normals.push([0.0, 1.0, 0.0]);
        // simple planar UV (you can rescale/center as needed)
        let world_pos = p + world_center;
        let mut uv = ((world_pos - min) / size) * 4.0;
        uv = uv.fract_gl();
        uvs.push(p.fract_gl().to_array());
    }

    // -----------------
    // 3) BOTTOM CAP VERTICES (y = 0, normal -Y)
    // -----------------
    for i in 0..n {
        let p = path[i];
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
fn centroid(points: &[(Vec2, Vec2)]) -> Option<Vec2> {
    if points.is_empty() {
        return None;
    }

    // Sum all points
    let sum = points
        .iter()
        .map(|(v, w)| v)
        .copied()
        .reduce(|a, b| a + b)
        .unwrap();

    // Divide by the number of points to get the average
    Some(sum / points.len() as f32)
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
