use bevy::{
    asset::RenderAssetUsages, color::palettes::css::RED, ecs::system::SystemState, input_focus::InputFocus, light::{NotShadowCaster, light_consts::lux}, mesh::{Indices, PrimitiveTopology}, prelude::*, state::state::OnEnter
};
use bevy_easings::Ease;
use bevy_rts_camera::Ground;
use bevy_tokio_tasks::TokioTasksRuntime;
use colorgrad::Gradient;
use llm_api::SettlementNameCtx;
use rand::Rng;
use std::{collections::HashMap, ops::Deref};
pub use world_generation::*;

use crate::{AppState, Cell, CellHighlight, GameState, Random, Selection, Unit, llm};
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
#[derive(Resource)]
pub struct WorldGenerationParams(pub Option<world_generation::WorldGenerationParams>);
impl From<&crate::Args> for WorldGenerationParams {
    fn from(value: &crate::Args) -> Self {
        WorldGenerationParams(Some(world_generation::WorldGenerationParams {
            width: value.width,
            height: value.height,
            plate_count: value.plate_count,
            plate_size: value.plate_size,
            continent_count: value.continent_count,
            continent_size: value.continent_size,
            ocean_count: value.ocean_count,
            ocean_size: value.ocean_size,
            scale: 30.0,
        }))
    }
}
pub struct WorldPlugin;
impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldMap>();
        app.add_computed_state::<GenerationPhase>();
        app.add_sub_state::<GenerationState>();
        app.add_systems(OnEnter(GenerationState::World), gen_world);
        app.add_systems(OnEnter(GenerationState::Settlements), generate_settlement_name);
        app.add_systems(OnEnter(GenerationState::Spawn), spawn_world);
        app.add_systems(OnEnter(GenerationState::Finshed),(remove_marked::<GenerationScreen>, generated_screen));
        app.add_systems(OnExit(GenerationState::Finshed), remove_marked::<GeneratedScreen>);
        app.add_systems(Update, button_system.run_if(in_state(GenerationState::Finshed)));
    }
}
fn generate_settlement_name(
    mut rng: ResMut<Random<crate::RandomRng>>,
    runtime: ResMut<TokioTasksRuntime>,
    game_state: Res<GameState>,
) {
    let temp = rng.0.as_mut().unwrap().random_range(0.3..0.5);
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
                        next_state.set(GenerationState::Spawn);
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
                let mut system_state =
                    SystemState::<(ResMut<WorldMap>, ResMut<NextState<GenerationState>>)>::new(world);
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

fn remove_marked<M:Component>(mut commands: Commands, query: Query<Entity, With<M>>) {
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
