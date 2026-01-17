use bevy::{
    asset::RenderAssetUsages,
    color::palettes::css::{BLACK, RED},
    ecs::system::SystemState,
    input_focus::InputFocus,
    light::{NotShadowCaster, light_consts::lux},
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    state::state::OnEnter,
};
use bevy_easings::Ease;
use bevy_persistent::Persistent;
use bevy_prototype_lyon::prelude::{ShapeBuilder, ShapeBuilderBase};
use bevy_rts_camera::Ground;
use bevy_tokio_tasks::TokioTasksRuntime;
use clap::ValueEnum;
use colorgrad::Gradient;
use llm_api::{settlement_names::SettlementNameCtx, unit_spawn_barks::UnitSpawnBarkCtx};
use rand::Rng;
use std::{collections::HashMap, ops::Deref};
pub use world_generation::CellId;

use crate::{
    AppState, CURRENT_OS, Cell, CellHighlight, GameState, LLMProvider, LLMSettings, Random,
    Selection, Unit, llm,
};
#[derive(Resource, Default)]
pub struct WorldMap(pub Option<world_generation::WorldMap>);

impl Deref for WorldMap {
    type Target = world_generation::WorldMap;

    fn deref(&self) -> &Self::Target {
        if let Some(a) = self.0.as_ref() {
            a
        } else {
            panic!("WorldMap not generated yet");
        }
    }
}
impl core::ops::DerefMut for WorldMap {
    fn deref_mut(&mut self) -> &mut Self::Target {
        if let Some(a) = self.0.as_mut() {
            a
        } else {
            panic!("WorldMap not generated yet");
        }
    }
}
#[derive(Clone, Copy, ValueEnum, Debug)]
pub enum WorldType {
    Default = 0,
    Small = 1,
    Large = 2,
    Flat = 3,
}
impl std::fmt::Display for WorldType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorldType::Default => write!(f, "default"),
            WorldType::Flat => write!(f, "flat"),
            WorldType::Small => write!(f, "small"),
            WorldType::Large => write!(f, "large"),
        }
    }
}
impl From<WorldType> for world_generation::WorldType {
    fn from(value: WorldType) -> Self {
        match value {
            WorldType::Default => world_generation::WorldType::Default,
            WorldType::Flat => world_generation::WorldType::Flat,
            WorldType::Small => world_generation::WorldType::Small,
            WorldType::Large => world_generation::WorldType::Large,
        }
    }
}
impl WorldType {
    pub fn get_params(&self) -> world_generation::WorldGenerationParams {
        let a: world_generation::WorldType = (*self).into();
        a.get_params()
    }
}

#[derive(Resource)]
pub struct WorldGenerationParams(pub Option<world_generation::WorldGenerationParams>);

pub struct WorldPlugin;
impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldMap>();
        app.add_computed_state::<GenerationPhase>();
        app.add_sub_state::<GenerationState>();
        app.add_systems(OnEnter(GenerationState::World), gen_world);
        app.add_systems(
            OnEnter(GenerationState::Settlements),
            generate_settlement_name,
        );
        app.add_systems(
            OnEnter(GenerationState::UnitSpawn),
            generate_unit_spawn_barks,
        );
        app.add_systems(OnEnter(GenerationState::Spawn), spawn_world);
        app.add_systems(
            OnEnter(GenerationState::Finshed),
            (remove_marked::<GenerationScreen>, generated_screen),
        );
        app.add_systems(
            OnExit(GenerationState::Finshed),
            remove_marked::<GeneratedScreen>,
        );
        app.add_systems(
            Update,
            button_system.run_if(in_state(GenerationState::Finshed)),
        );
    }
}
fn generate_unit_spawn_barks(
    mut rng: ResMut<Random<crate::RandomRng>>,
    runtime: ResMut<TokioTasksRuntime>,
    mut game_state: ResMut<GameState>,
    llm_cpu: Res<Persistent<LLMSettings>>,
    llm_providers: Res<Assets<LLMProvider>>,
) {
    let temp = rng.0.as_mut().unwrap().random_range(0.3..0.5);
    let llm_path = llm_cpu
        .llm_mode
        .as_ref()
        .and_then(|l| {
            llm_providers.iter().find(|p| p.1.id == *l).map(|p| {
                p.1.meta
                    .iter()
                    .find(|m| m.os == CURRENT_OS)
                    .map(|m| m.path.clone())
            })
        })
        .flatten();
    for player in game_state.players.values_mut() {
        let player_id = player.id;
        for unit in player.civ.units.iter() {
            player.unit_spawn_barks.insert(unit.name.clone(), vec![]);
        }
        let units = player.civ.units.clone();

        let civ_description = player.civ.description.clone();
        let civ_name = player.civ.name.clone();
        let llm_path = llm_path.clone();
        runtime.spawn_background_task(move |mut ctx| async move {
            for unit in units.iter() {
                info!("Generating barks for unit type: {}", unit.name);

                let unit_type = unit.name.clone();
                let seed_barks = unit.seed_barks.clone();
                let unit_description = unit.description.clone();
                let llm_path = llm_path.clone();
                if let Ok(barks) = llm::unit_spawn_barks(
                    llm_path,
                    UnitSpawnBarkCtx {
                        civilisation_name: civ_name.clone(),
                        civ_description:civ_description.clone(),
                        unit_type: unit_type.clone(),
                        seed_barks,
                        description: unit_description,
                    },
                    temp,
                )
                .await
                {
                    let unit_type = unit_type.clone();
                    ctx.run_on_main_thread(move |ctx| {
                        let world = ctx.world;
                        let (mut game_state, mut next_state) = {
                            let mut system_state = SystemState::<(
                                ResMut<GameState>,
                                ResMut<NextState<GenerationState>>,
                            )>::new(world);
                            system_state.get_mut(world)
                        };
                        let player = game_state.players.get_mut(&player_id).unwrap();
                        player.unit_spawn_barks.insert(unit_type.clone(), barks);
                        if game_state
                            .players
                            .values()
                            .all(|p| p.unit_spawn_barks.values().all(|b| !b.is_empty()))
                        {
                            next_state.set(GenerationState::Spawn);
                        }
                    })
                    .await;
                }
            }
        });
    }
}

fn generate_settlement_name(
    mut rng: ResMut<Random<crate::RandomRng>>,
    runtime: ResMut<TokioTasksRuntime>,
    game_state: Res<GameState>,
    llm_cpu: Res<Persistent<LLMSettings>>,
    llm_providers: Res<Assets<LLMProvider>>,
) {
    let temp = rng.0.as_mut().unwrap().random_range(0.3..0.5);
    let llm_path = llm_cpu
        .llm_mode
        .as_ref()
        .and_then(|l| {
            llm_providers.iter().find(|p| p.1.id == *l).map(|p| {
                p.1.meta
                    .iter()
                    .find(|m| m.os == CURRENT_OS)
                    .map(|m| m.path.clone())
            })
        })
        .flatten();
    for player in game_state.players.values() {
        let civ_name = player.settlement_context.civilisation_name.clone();
        let civ_description = player.settlement_context.description.clone();
        let seed_names = player.settlement_context.seed_names.clone();
        let player_id = player.id;
        let llm_path = llm_path.clone();
        runtime.spawn_background_task(move |mut ctx| async move {
            if let Ok(names) = llm::settlement_names(
                llm_path.clone(),
                SettlementNameCtx {
                    civilisation_name: civ_name,
                    description: civ_description,
                    seed_names,
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
                            ResMut<NextState<GenerationState>>,
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
                        next_state.set(GenerationState::UnitSpawn);
                    }
                })
                .await;
            }
        });
    }
}
fn spawn_world(
    world_map: Res<WorldMap>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    mut next_generation_state: ResMut<NextState<GenerationState>>,
) {
    let scale = world_map.scale;
    let parchment_handle: Handle<Image> = asset_server.load("textures/brown-texture.png");
    let g = colorgrad::GradientBuilder::new()
        .css("#001a33 0%, #003a6b 18%, #0f7a8a 32%, #bfe9e9 42%, #f2e6c8 48%, #e8d7a1 52%, #a7c88a 62%, #5b7f3a 72%, #8c8f93 85%, #cdd2d8 93%, #ffffff 100%   ")
        .build::<colorgrad::LinearGradient>().unwrap();
    let mut height_material_cache = HashMap::<u8, Handle<StandardMaterial>>::new();
    let outline_material = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        unlit: true,

        ..Default::default()
    });
    let map_box = world_map.bounds();
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

        let polygon = bevy_prototype_lyon::prelude::shapes::Polygon {
            points: vertices.clone(),
            closed: true,
        };
        vertices.push(vertices[0]);
        // let vertices3: Vec<(Vec3, Vec2)> = vertices
        //     .iter()
        //     .map(|(v, uv)| (v.extend(0.0).xzy(), *uv))
        //     .collect();
        //vertices3.reverse();
        let height = world_map.get_raw_height(&CellId(v_cell.site()));
        let color = g.at(height);
        assert!(color.to_css_hex() != "#000000");
        let color = bevy::color::Color::srgba(color.r, color.g, color.b, 1.0); //.lighter(0.2);
        let height_key = (height * 100.0).round() as u8;
        assert!((0.0..=1.0).contains(&height));
        let material = if let Some(mat) = height_material_cache.get(&height_key) {
            mat.clone()
        } else {
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
        let cell_shape = ShapeBuilder::with(&polygon)
            .fill(color.with_saturation(color.saturation() / 2.0))
            .stroke((BLACK, 0.1))
            .build();
        //let pos = world_map.get_position_for_cell(CellId(v_cell.site()));
        commands.spawn((
            cell_shape,
            Transform::from_xyz(
                // Distribute shapes from -X_EXTENT/2 to +X_EXTENT/2.
                v_cell.site_position().x as f32 * scale,
                v_cell.site_position().y as f32 * scale,
                0.0,
            ),
        ));
        let scaled_height = height * world_map.height_scale;
        let height_vertices = &vertices
            .iter()
            .map(|v| {
                temp::PolyVert::new(
                    *v,
                    world_map.get_height_at_vertex(
                        (*v / scale)
                            + vec2(
                                v_cell.site_position().x as f32,
                                v_cell.site_position().y as f32,
                            ),
                    ),
                )
            })
            .collect::<Vec<_>>();
        let mesh = temp::build_top_cap_mesh_convex_normalized_uv(height_vertices);
        // let temp = height_vertices
        //     .iter()
        //     .map(|p| (p.p, p.h))
        //     .collect::<Vec<_>>();
        // let mesh = build_extruded_with_caps(&temp, scaled_height,
        //     v_cell.site_position().to_vec2() * scale,map_box);
        //let mesh = Cuboid::new(1.0, scaled_height, 1.0);
        // if height < 0.5 {
        //     let mesh = build_extruded_with_caps(
        //         &vertices
        //             .iter()
        //             .map(|v| (*v, (0.5 - height) * scale * 0.25))
        //             .collect::<Vec<_>>(),
        //         (0.5 - height) * scale * 0.25,
        //         v_cell.site_position().to_vec2() * scale,
        //         map_box,
        //     );

        //     commands.spawn((
        //         Mesh3d(meshes.add(mesh)),
        //         MeshMaterial3d(ocean_material.clone()),
        //         Transform::from_xyz(
        //             // Distribute shapes from -X_EXTENT/2 to +X_EXTENT/2.
        //             v_cell.site_position().x as f32 * scale,
        //             scaled_height,
        //             v_cell.site_position().y as f32 * scale,
        //         ),
        //         Ground,
        //         NotShadowCaster,
        //     ));
        // }
        let temp = height_vertices
            .iter()
            .map(|p| (p.p, p.h))
            .collect::<Vec<_>>();
        let line = Polyline3d::new(extrude_polygon_xz_to_polyline_vertices(
            &temp,
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
    commands.insert_resource(AmbientLight {
        color: Color::srgb_u8(58, 135, 184),
        brightness: 20000.0,
        ..default()
    });
    let width = map_box.1.x - map_box.0.x;
    let height = map_box.1.y - map_box.0.y;
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::new(Vec3::Y, vec2(width / 2.0, height / 2.0)))),
        MeshMaterial3d(ocean_material),
        Transform::from_xyz(
            map_box.0.x + (width / 2.0),
            world_map.height_scale * 0.5,
            map_box.0.y + (height / 2.0),
        ),
        Ground,
        NotShadowCaster,
    ));
    commands.spawn((
        DirectionalLight {
            shadows_enabled: true,
            illuminance: lux::RAW_SUNLIGHT,
            ..default()
        },
        //cascade_shadow_config,
        Transform::from_xyz(0.0, 200.0, -200.0).looking_at(vec3(0.0, 0.0, 0.0), Vec3::Y),
    ));
    next_generation_state.set(GenerationState::Finshed);
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
    pathfinding: Res<crate::pathfinding::PathFinding>,
) {
    if let Selection::Unit(unit_entity) = *selected {
        let crate::pathfinding::PathFinding { graph, nodes } = pathfinding.as_ref();
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

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct GenerationPhase;
impl ComputedStates for GenerationPhase {
    type SourceStates = Option<AppState>;

    fn compute(sources: Self::SourceStates) -> Option<Self> {
        if let Some(AppState::Generating) = sources {
            Some(GenerationPhase)
        } else {
            None
        }
    }
}
#[derive(SubStates, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[source(GenerationPhase = GenerationPhase)]
enum GenerationState {
    #[default]
    World,
    Settlements,
    UnitSpawn,
    Spawn,
    Finshed,
}
#[derive(Component)]
struct GenerationScreen;
fn gen_world(
    mut commands: Commands,
    args: Res<WorldGenerationParams>,
    rng: ResMut<crate::Random<crate::RandomRng>>,
    runtime: ResMut<TokioTasksRuntime>,
) {
    info!("Generating world...");
    let a = *args.0.as_ref().unwrap();
    let rng = rng.0.as_ref().unwrap().clone();
    commands.spawn((
        GenerationScreen,
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
                    Text::new("Generating World..."),
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
                    Text::new("Please Stand By"),
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
    runtime.spawn_background_task(move |mut ctx| async move {
        let mut rng = rng;
        let generated_world = world_generation::generate_world(a, &mut rng).unwrap();
        ctx.run_on_main_thread(move |ctx| {
            let world = ctx.world;
            let (mut world_map, mut next_state) = {
                let mut system_state = SystemState::<(
                    ResMut<WorldMap>,
                    ResMut<NextState<GenerationState>>,
                )>::new(world);
                system_state.get_mut(world)
            };
            world_map.0 = Some(generated_world);
            info!("World generated.");
            next_state.set(GenerationState::Settlements);
        })
        .await;
    });
    //next_state.set(AppState::InGame);
}
// fn remove_generation_screen(mut commands: Commands, query: Query<Entity, With<GenerationScreen>>) {
//     for entity in query.iter() {
//         commands.entity(entity).despawn();
//     }
// }

fn remove_marked<M: Component>(mut commands: Commands, query: Query<Entity, With<M>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}
#[derive(Component)]
struct GeneratedScreen;
fn generated_screen(mut commands: Commands) {
    commands.spawn((
        GeneratedScreen,
        Node {
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            flex_direction: FlexDirection::Column,
            position_type: PositionType::Absolute,
            top: Val::Percent(0.0),
            ..default()
        },
        children![
            (
                Node { ..default() },
                children![(
                    Text::new("World Generated!"),
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
                Button,
                BorderColor::all(Color::WHITE),
                BorderRadius::MAX,
                BackgroundColor(Color::BLACK),
                children![(
                    Text::new("Continue"),
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
const NORMAL_BUTTON: Color = Color::srgb(0.15, 0.15, 0.15);
const HOVERED_BUTTON: Color = Color::srgb(0.25, 0.25, 0.25);
const PRESSED_BUTTON: Color = Color::srgb(0.35, 0.75, 0.35);

fn button_system(
    mut input_focus: ResMut<InputFocus>,
    mut interaction_query: Query<
        (
            Entity,
            &Interaction,
            &mut BackgroundColor,
            &mut BorderColor,
            &mut Button,
        ),
        Changed<Interaction>,
    >,
    mut next_state: ResMut<NextState<AppState>>,
) {
    for (entity, interaction, mut color, mut border_color, mut button) in &mut interaction_query {
        match *interaction {
            Interaction::Pressed => {
                input_focus.set(entity);
                *color = PRESSED_BUTTON.into();
                *border_color = BorderColor::all(RED);
                next_state.set(AppState::InGame);
                // The accessibility system's only update the button's state when the `Button` component is marked as changed.
                button.set_changed();
            }
            Interaction::Hovered => {
                input_focus.set(entity);
                *color = HOVERED_BUTTON.into();
                *border_color = BorderColor::all(Color::WHITE);
                button.set_changed();
            }
            Interaction::None => {
                input_focus.clear();
                *color = NORMAL_BUTTON.into();
                *border_color = BorderColor::all(Color::BLACK);
            }
        }
    }
}
/// Build a single mesh: extruded strip + top & bottom caps.
/// `path` is in XZ (Vec2(x, z)), extrusion along +Y by `height`.
pub fn build_extruded_with_caps(
    path: &[(Vec2, f32)],
    _height: f32,
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
            (path[1].0 - path[0].0).normalize()
        } else if i == n - 1 {
            (path[i].0 - path[i - 1].0).normalize()
        } else {
            let d1 = (path[i].0 - path[i - 1].0).normalize();
            let d2 = (path[i + 1].0 - path[i].0).normalize();
            (d1 + d2).normalize()
        };

        // outward normal in XZ (left of the path):
        // n2 = (-dz, dx)
        let n2 = Vec2::new(-dir.y, dir.x).normalize();
        let normal3 = [n2.x, 0.0, n2.y];

        // bottom vertex
        positions.push([p.0.x, 0.0, p.0.y]);
        normals.push(normal3);
        //let u = i as f32 / (n as f32 - 1.0);
        uvs.push([0.0, 0.0]);

        // top vertex
        positions.push([p.0.x, p.1, p.0.y]);
        normals.push(normal3);
        uvs.push([0.0, 0.0]);
    }

    // -----------------
    // 2) TOP CAP VERTICES (y = height, normal +Y)
    // -----------------
    for p in path.iter().take(n) {
        positions.push([p.0.x, p.1, p.0.y]);
        normals.push([0.0, 1.0, 0.0]);
        // simple planar UV (you can rescale/center as needed)
        // let world_pos = p + world_center;
        // let mut uv = ((world_pos - min) / size) * 4.0;
        // uv = uv.fract_gl();
        uvs.push(p.0.fract_gl().to_array());
    }

    // -----------------
    // 3) BOTTOM CAP VERTICES (y = 0, normal -Y)
    // -----------------
    for p in path.iter().take(n) {
        positions.push([p.0.x, 0.0, p.0.y]);
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
fn extrude_polygon_xz_to_polyline_vertices(
    polygon_xz: &[(Vec2, f32)],
    y0: f32,
    _y1: f32,
) -> Vec<Vec3> {
    let n = polygon_xz.len();
    if n == 0 {
        return Vec::new();
    }

    let mut verts = Vec::new();

    let b = |i: usize| Vec3::new(polygon_xz[i].0.x, y0, polygon_xz[i].0.y);
    let t = |i: usize| Vec3::new(polygon_xz[i].0.x, polygon_xz[i].1, polygon_xz[i].0.y);

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

mod temp {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
    use bevy::prelude::*;

    #[derive(Clone, Copy, Debug)]
    pub struct PolyVert {
        /// Footprint (x,z) stored as (x,y)
        pub p: Vec2,
        /// Height -> Y
        pub h: f32,
    }
    impl PolyVert {
        pub fn new(p: Vec2, h: f32) -> Self {
            Self { p, h }
        }
    }
    /// Top-cap mesh for a *convex* polygon (triangle fan).
    /// - Positions: (x, y=height, z)
    /// - UVs: normalized to 0..1 over the polygon's footprint AABB
    /// - Normals: computed from the (possibly non-planar) cap triangles (smooth per-vertex)
    ///
    /// Notes:
    /// - Convex only (fan triangulation).
    /// - If you want the cap to appear perfectly flat-shaded upward, set all normals to (0,1,0) instead.
    pub fn build_top_cap_mesh_convex_normalized_uv(boundary: &[PolyVert]) -> Mesh {
        assert!(boundary.len() >= 3, "Polygon needs at least 3 vertices");

        // Ensure CCW winding in footprint so triangles face +Y consistently.
        let mut b: Vec<PolyVert> = boundary.to_vec();
        if signed_area_2d(&b) > 0.0 {
            b.reverse();
        }

        // AABB in footprint space for UV normalization
        let (min, max) = bounds_2d(&b);
        let size = (max - min).max(Vec2::splat(1e-6)); // avoid div-by-zero

        let uv_of = |p: Vec2| -> [f32; 2] {
            let q = (p - min) / size; // 0..1 over bounds
            [q.x, q.y]
        };

        let mut positions: Vec<[f32; 3]> = Vec::with_capacity(b.len());
        let mut normals: Vec<[f32; 3]> = vec![[0.0, 0.0, 0.0]; b.len()];
        let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(b.len());
        let mut indices: Vec<u32> = Vec::with_capacity((b.len().saturating_sub(2)) * 3);

        // Vertices
        for v in &b {
            positions.push([v.p.x, v.h, v.p.y]);
            uvs.push(uv_of(v.p));
        }

        // Indices (triangle fan from vertex 0)
        for i in 1..(b.len() - 1) {
            indices.extend_from_slice(&[0, i as u32, (i as u32 + 1)]);
        }

        // Compute smooth normals from cap triangles
        compute_smooth_normals_full(&positions, &indices, &mut normals);

        // Assemble mesh
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::all());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        mesh.insert_indices(Indices::U32(indices));
        mesh
    }

    // -------------------------
    // helpers
    // -------------------------

    fn signed_area_2d(v: &[PolyVert]) -> f32 {
        let mut area = 0.0;
        for i in 0..v.len() {
            let a = v[i].p;
            let b = v[(i + 1) % v.len()].p;
            area += a.x * b.y - b.x * a.y;
        }
        0.5 * area
    }

    fn bounds_2d(v: &[PolyVert]) -> (Vec2, Vec2) {
        let mut min = Vec2::splat(f32::INFINITY);
        let mut max = Vec2::splat(f32::NEG_INFINITY);
        for vert in v {
            min = min.min(vert.p);
            max = max.max(vert.p);
        }
        (min, max)
    }

    fn compute_smooth_normals_full(
        positions: &[[f32; 3]],
        indices: &[u32],
        normals: &mut [[f32; 3]],
    ) {
        // zero
        for n in normals.iter_mut() {
            *n = [0.0, 0.0, 0.0];
        }

        for tri in indices.chunks_exact(3) {
            let i0 = tri[0] as usize;
            let i1 = tri[1] as usize;
            let i2 = tri[2] as usize;

            let p0 = Vec3::from_array(positions[i0]);
            let p1 = Vec3::from_array(positions[i1]);
            let p2 = Vec3::from_array(positions[i2]);

            let n = (p1 - p0).cross(p2 - p0); // area-weighted
            if n.length_squared() == 0.0 {
                continue;
            }

            for &i in &[i0, i1, i2] {
                normals[i][0] += n.x;
                normals[i][1] += n.y;
                normals[i][2] += n.z;
            }
        }

        for n in normals.iter_mut() {
            let v = Vec3::new(n[0], n[1], n[2]);
            let v = if v.length_squared() > 0.0 {
                v.normalize()
            } else {
                Vec3::Y
            };
            *n = [v.x, v.y, v.z];
        }
    }
}
