use bevy::{
    color::palettes::css::RED, ecs::system::SystemState, input_focus::InputFocus, prelude::*,
    state::state::OnEnter,
};
use bevy_easings::Ease;
use bevy_tokio_tasks::TokioTasksRuntime;
use std::ops::Deref;
pub use world_generation::*;

use crate::AppState;
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
        app.add_systems(OnEnter(AppState::Generating), gen_world);
        app.add_systems(OnExit(AppState::Generating), remove_marked::<GenerationScreen>);
        app.add_systems(OnEnter(AppState::Generated), generated_screen);
        app.add_systems(OnExit(AppState::Generated), remove_marked::<GeneratedScreen>);
        app.add_systems(Update, button_system.run_if(in_state(AppState::Generated)));
    }
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
                    SystemState::<(ResMut<WorldMap>, ResMut<NextState<AppState>>)>::new(world);
                system_state.get_mut(world)
            };
            world_map.0 = Some(generated_world);
            info!("World generated.");
            next_state.set(AppState::Generated);
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
