#![forbid(unsafe_code)]
use egui::{Align2, Area, CentralPanel, Frame, Margin};
use std::fmt::Display;

pub enum MainMenuAction {
    None,
    NewGame,
    Settings,
    Exit,
}
pub fn main_menu(ctx: &mut egui::Context, offset_x: f32, width: f32) -> MainMenuAction {
    CentralPanel::default().show(ctx, |_ui| {});
    let mut action = MainMenuAction::None;
    // How far from the center we want the menu panel

    Area::new("main_menu".into())
        // Anchor to window center, then offset a bit to the left
        .anchor(Align2::CENTER_CENTER, egui::vec2(offset_x, 0.0))
        .show(ctx, |ui| {
            // Draw a framed "panel"
            Frame::default()
                //.corner_radius(5.0.into())
                .inner_margin(Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(width);

                    ui.vertical_centered(|ui| {
                        ui.heading("Main Menu");
                        ui.add_space(12.0);

                        if ui.button("New Game").clicked() {
                            action = MainMenuAction::NewGame;
                        }

                        ui.add_space(4.0);

                        if ui.button("Settings").clicked() {
                            action = MainMenuAction::Settings;
                        }

                        ui.add_space(4.0);

                        if ui.button("Exit").clicked() {
                            // Quit the app
                            action = MainMenuAction::Exit;
                        }
                    });
                });
        });
    action
}
pub struct Settings {
    pub music_volume: f32,
    pub llm_mode: LLMMode,
}
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum LLMMode {
    Cuda,
    Cpu,
    None,
}
pub enum SettingsMenuAction {
    None,
    Save,
    Return,
}
pub fn settings_menu(
    ctx: &mut egui::Context,
    offset_x: f32,
    width: f32,
    settings: &mut Settings,
) -> SettingsMenuAction {
    let mut action = SettingsMenuAction::None;
    egui::CentralPanel::default().show(ctx, |_ui| {});
    egui::Area::new("main_menu".into())
        // Anchor to window center, then offset a bit to the left
        .anchor(Align2::CENTER_CENTER, egui::vec2(offset_x, 0.0))
        .show(ctx, |ui| {
            egui::Frame::default()
                //.corner_radius(5.0.into())
                .inner_margin(Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(width);
                    ui.vertical_centered(|ui| {
                        ui.heading("Settings");
                        ui.add_space(12.0);
                        let slider = egui::Slider::new(&mut settings.music_volume, 0.0..=1.0)
                            .clamping(egui::SliderClamping::Always)
                            .text("Music volume");

                        if ui.add(slider).changed() {
                            // Apply to the actual audio channel
                        }
                        ui.add_space(4.0);
                        egui::ComboBox::from_label("LLM Mode")
                            .selected_text(match settings.llm_mode {
                                LLMMode::Cuda => "CUDA",
                                LLMMode::Cpu => "CPU",
                                LLMMode::None => "None",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut settings.llm_mode, LLMMode::Cuda, "CUDA");
                                ui.selectable_value(&mut settings.llm_mode, LLMMode::Cpu, "CPU");
                                ui.selectable_value(&mut settings.llm_mode, LLMMode::None, "None");
                            });
                        ui.add_space(4.0);
                        if ui.button("Save").clicked() {
                            action = SettingsMenuAction::Save;
                        }
                        if ui.button("Return").clicked() {
                            action = SettingsMenuAction::Return;
                        }
                    });
                });
        });
    action
}
pub struct NewWorldSettings<Civ> {
    pub world_type: world_generation::WorldType,
    pub player_count: usize,
    pub selected_civs: Vec<Option<Civ>>,
}
pub enum NewGameMenuAction {
    None,
    Start,
    Return,
}
pub fn new_game_menu<CivId: PartialEq + Eq + std::hash::Hash + Copy, Civ: Display + Ord>(
    ctx: &mut egui::Context,
    offset_x: f32,
    width: f32,
    settings: &mut NewWorldSettings<CivId>,
    civs: &std::collections::HashMap<CivId, Civ>,
) -> NewGameMenuAction {
    let mut action = NewGameMenuAction::None;
    let mut civ_list = civs.iter().collect::<Vec<_>>();
    civ_list.sort_by_key(|e| e.1);
    egui::CentralPanel::default().show(ctx, |_ui| {});
    egui::Area::new("main_menu".into())
        .anchor(Align2::CENTER_CENTER, egui::vec2(offset_x, 0.0))
        .show(ctx, |ui| {
            ui.set_width(width);
            ui.vertical_centered(|ui| {
                ui.heading("New Game");
                ui.add_space(12.0);
                //ui.radio_value(temp_params.world_type, alternative, atoms);
                egui::ComboBox::from_label("World Type")
                    .selected_text(format!("{:?}", settings.world_type))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut settings.world_type,
                            world_generation::WorldType::Default,
                            "Default",
                        );
                        ui.selectable_value(
                            &mut settings.world_type,
                            world_generation::WorldType::Flat,
                            "Flat",
                        );
                    });
                ui.add_space(4.0);
                let slider = egui::Slider::new(
                    &mut settings.player_count, // or your actual field name
                    1..=4,
                )
                .clamping(egui::SliderClamping::Always)
                .text("Player Count");
                ui.add(slider);
                ui.add_space(4.0);
                for i in 0..settings.player_count {
                    let selected_civ = settings.selected_civs.get(i);
                    egui::ComboBox::from_label(format!("Player {} Civ", i + 1))
                        .selected_text(if let Some(selected_civ) = selected_civ.unwrap() {
                            format!("{}", &civs.get(selected_civ).unwrap())
                        } else {
                            "Select Civ".to_string()
                        })
                        .show_ui(ui, |ui| {
                            for (civ_id, civ) in civ_list.iter() {
                                ui.selectable_value(
                                    settings.selected_civs.get_mut(i).unwrap(),
                                    Some(**civ_id),
                                    format!("{}", civ),
                                );
                            }
                        });
                }
                let start_button_enabled = settings
                    .selected_civs
                    .iter()
                    .take(settings.player_count)
                    .all(|civ| civ.is_some());
                let start_button = egui::Button::new("Start");
                if ui.add_enabled(start_button_enabled, start_button).clicked() {
                    action = NewGameMenuAction::Start;
                }
                ui.add_space(4.0);
                if ui.button("Return").clicked() {
                    action = NewGameMenuAction::Return;
                }
            });
        });
    action
}
