use bevy::{
    prelude::*,
    window::{PrimaryWindow, WindowMode},
};
use bevy_egui::{EguiContexts, EguiPrimaryContextPass};
use bevy_kira_audio::{AudioChannel, AudioControl};
use menu::{FullscreenMode, Settings};

use crate::{AppState, AudioSettings, Civilisation, GameState, LLMSettings, Music, VideoSettings};
pub struct MenuPlugin;
impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_computed_state::<MainMenu>();
        app.add_sub_state::<MenuState>();
        app.add_systems(
            EguiPrimaryContextPass,
            main_menu.run_if(in_state(MenuState::Main)),
        );
        app.add_systems(
            EguiPrimaryContextPass,
            settings_menu.run_if(in_state(MenuState::Settings)),
        );
        app.add_systems(
            EguiPrimaryContextPass,
            new_game_menu.run_if(in_state(MenuState::NewGame)),
        );
    }
}
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct MainMenu;
impl ComputedStates for MainMenu {
    type SourceStates = Option<AppState>;

    fn compute(sources: Self::SourceStates) -> Option<Self> {
        if let Some(AppState::Menu) = sources {
            Some(MainMenu)
        } else {
            None
        }
    }
}
#[derive(SubStates, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[source(MainMenu = MainMenu)]
enum MenuState {
    #[default]
    Main,
    Settings,
    NewGame,
}
const MENU_OFFSET_X: f32 = -200.0; // negative = left of center
const MENU_WIDTH: f32 = 220.0;
fn main_menu(
    mut contexts: EguiContexts,
    mut next_menu_state: ResMut<NextState<MenuState>>,
    mut exit_events: MessageWriter<bevy::app::AppExit>,
) {
    let ctx = contexts.ctx_mut().unwrap();

    let action = menu::main_menu(ctx, MENU_OFFSET_X, MENU_WIDTH);
    match action {
        menu::MainMenuAction::NewGame => {
            next_menu_state.set(MenuState::NewGame);
        }
        menu::MainMenuAction::Settings => {
            next_menu_state.set(MenuState::Settings);
        }
        menu::MainMenuAction::Exit => {
            exit_events.write(AppExit::Success);
        }
        menu::MainMenuAction::None => {}
    }
}

fn new_game_menu(
    mut commands: Commands,
    mut contexts: EguiContexts,
    mut next_state: ResMut<NextState<AppState>>,
    mut next_menu_state: ResMut<NextState<MenuState>>,
    //mut params: ResMut<crate::generate::WorldGenerationParams>,
    mut temp_params: Local<Option<world_generation::WorldGenerationParams>>,
    mut player_count: Local<Option<u8>>,
    civs: Res<Assets<Civilisation>>,
    mut selected_civs: Local<Option<Vec<Option<AssetId<Civilisation>>>>>,
) {
    let ctx = contexts.ctx_mut().unwrap();
    let temp_params = temp_params.get_or_insert_with(|| crate::generate::WorldType::Default.get_params());
    let player_count = player_count.get_or_insert_with(|| 1);
    let selected_civs = selected_civs.get_or_insert_with(|| vec![None, None, None, None]);
    let mut settings = menu::NewWorldSettings {
        world_type: temp_params.world_type,
        player_count: *player_count as usize,
        selected_civs: selected_civs.clone(),
    };
    let civ_map = civs
        .iter()
        .map(|(id, civ)| (id, civ.name.clone()))
        .collect::<std::collections::HashMap<_, _>>();
    let action = menu::new_game_menu(ctx, MENU_OFFSET_X, MENU_WIDTH, &mut settings, &civ_map);
    *temp_params = settings.world_type.get_params();
    *player_count = settings.player_count as u8;
    *selected_civs = settings.selected_civs.clone();
    match action {
        menu::NewGameMenuAction::None => {}
        menu::NewGameMenuAction::Start => {
            commands.insert_resource(crate::generate::WorldGenerationParams(Some(*temp_params)));
            //*params = crate::generate::WorldGenerationParams(Some(*temp_params));
            commands.insert_resource(GameState::new(
                settings.player_count,
                &mut settings.selected_civs,
                civs.as_ref(),
            ));
            next_state.set(AppState::Generating);
        }
        menu::NewGameMenuAction::Return => {
            next_menu_state.set(MenuState::Main);
        }
    }
}

fn settings_menu(
    mut contexts: EguiContexts,
    mut next_menu_state: ResMut<NextState<MenuState>>,
    mut audio_settings: ResMut<bevy_persistent::Persistent<AudioSettings>>,
    mut llm_settings: ResMut<bevy_persistent::Persistent<LLMSettings>>,
    mut video_settings: ResMut<bevy_persistent::Persistent<VideoSettings>>,
    mut temp_settings: Local<Option<::menu::Settings>>,
    music: Res<AudioChannel<Music>>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
) {
    let ctx = contexts.ctx_mut().unwrap();
    let temp_settings = temp_settings.get_or_insert_with(|| Settings {
        music_volume: audio_settings.music_volume,
        llm_mode: llm_settings.llm_mode.into(),
        window_mode: window.mode.into_settings(),
    });
    let action = menu::settings_menu(ctx, MENU_OFFSET_X, MENU_WIDTH, temp_settings);
    music.set_volume(crate::volume_from_slider(temp_settings.music_volume));
    match action {
        menu::SettingsMenuAction::None => {}
        menu::SettingsMenuAction::Save => {
            audio_settings
                .update(|settings| {
                    settings.music_volume = temp_settings.music_volume;
                })
                .expect("Failed to save audio settings");
            llm_settings
                .update(|settings| {
                    settings.llm_mode = temp_settings.llm_mode.into();
                })
                .expect("Failed to save LLM settings");
            video_settings
                .update(|settings| {
                    settings.window_mode = temp_settings.window_mode;
                })
                .expect("Failed to save video settings");
            window.mode = match temp_settings.window_mode {
                FullscreenMode::Windowed => WindowMode::Windowed,
                FullscreenMode::BorderlessFullscreen => {
                    WindowMode::BorderlessFullscreen(MonitorSelection::Current)
                }
                FullscreenMode::Fullscreen => {
                    WindowMode::Fullscreen(MonitorSelection::Current, VideoModeSelection::Current)
                }
            };
            next_menu_state.set(MenuState::Main);
        }
        menu::SettingsMenuAction::Return => {
            temp_settings.music_volume = audio_settings.music_volume;
            music.set_volume(crate::volume_from_slider(temp_settings.music_volume));
            next_menu_state.set(MenuState::Main);
        }
    }
}
trait IntoSettings<S: Copy>: Copy {
    fn into_settings(self) -> S;
}
impl IntoSettings<FullscreenMode> for WindowMode {
    #[inline]
    fn into_settings(self) -> FullscreenMode {
        match self {
            WindowMode::Windowed => FullscreenMode::Windowed,
            WindowMode::BorderlessFullscreen(monitor_selection) => {
                FullscreenMode::BorderlessFullscreen
            }
            WindowMode::Fullscreen(monitor_selection, video_mode_selection) => {
                FullscreenMode::Fullscreen
            }
        }
    }
}
