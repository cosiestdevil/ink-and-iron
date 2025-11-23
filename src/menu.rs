use bevy::prelude::*;
use bevy_egui::{
    EguiContexts, EguiPrimaryContextPass,
    egui::{self, Align2, Margin},
};
use bevy_kira_audio::{AudioChannel, AudioControl};

use crate::{AppState, AudioSettings, Music};
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
    mut next_state: ResMut<NextState<AppState>>,
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
                            next_state.set(AppState::Generating);
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
fn settings_menu(
    mut contexts: EguiContexts,
    mut next_menu_state: ResMut<NextState<MenuState>>,
    mut audio_settings: ResMut<AudioSettings>,
    music: Res<AudioChannel<Music>>,
) {
    let ctx = contexts.ctx_mut().unwrap();
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
                    });
                    // --- Music volume slider ---
                    // 0.0 = mute, 1.0 = full volume
                    let slider = egui::Slider::new(
                        &mut audio_settings.music_volume, // or your actual field name
                        0.0..=1.0,
                    )
                    .clamping(egui::SliderClamping::Always)
                    .text("Music volume");

                    if ui.add(slider).changed() {
                        // Apply to the actual audio channel
                        music.set_volume(((1.0 - audio_settings.music_volume) * -40.) - 10.);
                    }
                    ui.add_space(4.0);

                        if ui.button("Return").clicked() {
                            next_menu_state.set(MenuState::Main);
                        }
                });
        });
}
