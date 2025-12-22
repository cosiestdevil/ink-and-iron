use bevy::prelude::*;
use bevy_egui::{
    EguiContexts, EguiPrimaryContextPass,
    egui::{self, Align2, Margin},
};
use bevy_kira_audio::{AudioChannel, AudioControl};

use crate::{AppState, AudioSettings, Civilisation, GameState, Music};
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

    // Optional: central background
    egui::CentralPanel::default().show(ctx, |_ui| {});

    // How far from the center we want the menu panel

    egui::Area::new("main_menu".into())
        // Anchor to window center, then offset a bit to the left
        .anchor(Align2::CENTER_CENTER, egui::vec2(MENU_OFFSET_X, 0.0))
        .show(ctx, |ui| {
            // Draw a framed "panel"
            egui::Frame::default()
                //.corner_radius(5.0.into())
                .inner_margin(Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(MENU_WIDTH);

                    ui.vertical_centered(|ui| {
                        ui.heading("Main Menu");
                        ui.add_space(12.0);

                        if ui.button("New Game").clicked() {
                            next_menu_state.set(MenuState::NewGame);
                        }

                        ui.add_space(4.0);

                        if ui.button("Settings").clicked() {
                            next_menu_state.set(MenuState::Settings);
                        }

                        ui.add_space(4.0);

                        if ui.button("Exit").clicked() {
                            // Quit the app
                            exit_events.write(AppExit::Success);
                        }
                    });
                });
        });
}

fn new_game_menu(
    mut commands: Commands,
    mut contexts: EguiContexts,
    mut next_state: ResMut<NextState<AppState>>,
    mut next_menu_state: ResMut<NextState<MenuState>>,
    mut params: ResMut<crate::generate::WorldGenerationParams>,
    mut temp_params: Local<Option<world_generation::WorldGenerationParams>>,
    mut player_count: Local<Option<u8>>,
    civs: Res<Assets<Civilisation>>,
    mut selected_civs: Local<Option<Vec<Option<AssetId<Civilisation>>>>>,
) {
    let ctx = contexts.ctx_mut().unwrap();
    let temp_params = temp_params.get_or_insert_with(|| params.0.unwrap());
    let player_count = player_count.get_or_insert_with(|| 1);
    let selected_civs = selected_civs.get_or_insert_with(|| vec![None,None,None,None]);
    egui::CentralPanel::default().show(ctx, |_ui| {});
    egui::Area::new("main_menu".into())
        .anchor(Align2::CENTER_CENTER, egui::vec2(MENU_OFFSET_X, 0.0))
        .show(ctx, |ui| {
            ui.set_width(MENU_WIDTH);
            ui.vertical_centered(|ui| {
                ui.heading("New Game");
                ui.add_space(12.0);
                //ui.radio_value(temp_params.world_type, alternative, atoms);
                egui::ComboBox::from_label("World Type")
                    .selected_text(format!("{:?}", temp_params.world_type))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut temp_params.world_type,
                            world_generation::WorldType::Default,
                            "Default",
                        );
                        ui.selectable_value(
                            &mut temp_params.world_type,
                            world_generation::WorldType::Flat,
                            "Flat",
                        );
                    });
                ui.add_space(4.0);
                let slider = egui::Slider::new(
                    player_count, // or your actual field name
                    1..=4,
                )
                .clamping(egui::SliderClamping::Always)
                .text("Player Count");
                ui.add(slider);
                ui.add_space(4.0);
                for i in 0..*player_count {
                    let selected_civ = selected_civs.get(i as usize);
                    egui::ComboBox::from_label(format!("Player {} Civ",i+1))
                        .selected_text(if let Some(selected_civ) = selected_civ.unwrap() {
                            &civs.get(*selected_civ).unwrap().name
                        } else {
                            "Select Civ"
                        })
                        .show_ui(ui, |ui| {
                            for (civ_id, civ) in civs.iter() {
                                ui.selectable_value(
                                    selected_civs.get_mut(i as usize).unwrap(),
                                    Some(civ_id),
                                    civ.name.clone(),
                                );
                            }
                        });
                }
                let start_button_enabled = selected_civs
                    .iter()
                    .take(*player_count as usize)
                    .all(|civ| civ.is_some());
                let start_button = egui::Button::new("Start");
                if ui.add_enabled(start_button_enabled, start_button).clicked() {
                    params.0 = Some(*temp_params);
                    commands.insert_resource(GameState::new(
                        *player_count as usize,
                        selected_civs,
                        civs.as_ref(),
                    ));
                    next_state.set(AppState::Generating);
                }
                ui.add_space(4.0);
                if ui.button("Return").clicked() {
                    *temp_params = params.0.unwrap();
                    next_menu_state.set(MenuState::Main);
                }
            });
        });
}

fn settings_menu(
    mut contexts: EguiContexts,
    mut next_menu_state: ResMut<NextState<MenuState>>,
    mut audio_settings: ResMut<bevy_persistent::Persistent<AudioSettings>>,
    mut temp_audio_settings: Local<Option<AudioSettings>>,
    music: Res<AudioChannel<Music>>,
) {
    let ctx = contexts.ctx_mut().unwrap();
    let temp_audio_settings = temp_audio_settings.get_or_insert_with(|| (**audio_settings).clone());
    egui::CentralPanel::default().show(ctx, |_ui| {});
    egui::Area::new("main_menu".into())
        // Anchor to window center, then offset a bit to the left
        .anchor(Align2::CENTER_CENTER, egui::vec2(MENU_OFFSET_X, 0.0))
        .show(ctx, |ui| {
            egui::Frame::default()
                //.corner_radius(5.0.into())
                .inner_margin(Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(MENU_WIDTH);
                    ui.vertical_centered(|ui| {
                        ui.heading("Settings");
                        ui.add_space(12.0);

                        // --- Music volume slider ---
                        // 0.0 = mute, 1.0 = full volume

                        let slider = egui::Slider::new(
                            &mut temp_audio_settings.music_volume, // or your actual field name
                            0.0..=1.0,
                        )
                        .clamping(egui::SliderClamping::Always)
                        .text("Music volume");

                        if ui.add(slider).changed() {
                            // Apply to the actual audio channel
                            music.set_volume(crate::volume_from_slider(
                                temp_audio_settings.music_volume,
                            ));
                        }
                        ui.add_space(4.0);
                        if ui.button("Save").clicked() {
                            audio_settings
                                .set(temp_audio_settings.clone())
                                .expect("Failed to save audio settings");
                            music
                                .set_volume(crate::volume_from_slider(audio_settings.music_volume));

                            next_menu_state.set(MenuState::Main);
                        }
                        if ui.button("Return").clicked() {
                            *temp_audio_settings = (**audio_settings).clone();
                            music
                                .set_volume(crate::volume_from_slider(audio_settings.music_volume));
                            next_menu_state.set(MenuState::Main);
                        }
                    });
                });
        });
}
