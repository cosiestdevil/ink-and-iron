use bevy::camera::{Viewport, visibility::RenderLayers};
use bevy::prelude::*;
use bevy::render::render_resource::BlendState;
use bevy::window::PrimaryWindow;
use bevy_egui::egui::{ScrollArea, Stroke};
use bevy_egui::{
    EguiContext, EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass,
    PrimaryEguiContext, egui,
};

use crate::{AppState, GameState, Selection, SettlementCenter, TurnStart, Unit};

pub struct UIPlugin;
impl Plugin for UIPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EguiPlugin::default());
        app.add_systems(Startup, setup_ui_camera);
        app.add_systems(
            EguiPrimaryContextPass,
            ui_example_system.run_if(in_state(AppState::InGame)),
        );
    }
}

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
    time: Res<Time>,
) -> Result {
    let ctx = contexts.ctx_mut()?;

    let mut left = egui::SidePanel::left("left_panel")
        .resizable(true)
        .show(ctx, |ui| {
            egui::TopBottomPanel::bottom("my_side_panel_bottom")
                .exact_height(ui.available_height() / 5.0)
                .show_inside(ui, |ui| {
                    ScrollArea::vertical().show(ui, |ui| {
                        ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                            let active_player = game_state.active_player;
                            let player = game_state.players.get_mut(&active_player).unwrap();
                            let mut remove_indices = vec![];
                            for (i, notification) in
                                player.notifications.iter_mut().enumerate().rev()
                            {
                                notification.timer.tick(time.delta());
                                if notification.timer.is_finished() {
                                    remove_indices.push(i);
                                } else {
                                    ui.add_space(4.0);
                                    egui::Frame::default()
                                        .stroke(Stroke::new(1.0, egui::Color32::LIGHT_GRAY))
                                        .inner_margin(4.0)
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                if let Some(icon) = notification.icon.as_ref() {
                                                    ui.add(egui::widgets::Image::new(
                                                        egui::load::SizedTexture::new(
                                                            *icon,
                                                            [32.0, 32.0],
                                                        ),
                                                    ));
                                                }
                                                ui.label(&notification.message);
                                            });
                                        });
                                }
                            }
                            // Remove finished notifications
                            for &i in remove_indices.iter().rev() {
                                player.notifications.remove(i);
                            }
                        });
                    });
                });
            let active_player = game_state.active_player;
            let player = game_state.players.get_mut(&active_player).unwrap();
            ui.label(player.civ.name.clone());
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
                        ui.label(unit.name.clone());
                        ui.label(format!("Speed: {}/{}", unit.used_speed, unit.speed));
                        ui.label(format!(
                            "Health: {:.0}/{:.0}",
                            unit.health.ceil(),
                            unit.max_health.ceil()
                        ));
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
                        let version = option_env!("VERSION_TAG").unwrap_or("Custom");
                        ui.label(version);
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
