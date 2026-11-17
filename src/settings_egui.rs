//! egui Settings window.

use crate::config::{BackendPref, FitMode, UiToolkit, WallpaperMode};
use crate::settings_model::SettingsModel;
use crate::tray;
use anyhow::Result;
use eframe::egui;
use winit::platform::windows::EventLoopBuilderExtWindows;

pub fn open() -> Result<()> {
    let model = SettingsModel::load()?;
    // Tray opens Settings on a worker thread; Windows winit panics unless any_thread is set.
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([560.0, 720.0])
            .with_title("WallFX Settings"),
        event_loop_builder: Some(Box::new(|builder| {
            builder.with_any_thread(true);
        })),
        run_and_return: true,
        ..Default::default()
    };
    eframe::run_native(
        "WallFX Settings",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(SettingsApp {
                model,
                status: String::new(),
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("egui settings failed: {e}"))?;
    Ok(())
}

struct SettingsApp {
    model: SettingsModel,
    status: String,
}

impl eframe::App for SettingsApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("WallFX Settings");
            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Settings UI");
                egui::ComboBox::from_id_salt("toolkit")
                    .selected_text(self.model.toolkit.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.model.toolkit, UiToolkit::Egui, "egui");
                        ui.selectable_value(
                            &mut self.model.toolkit,
                            UiToolkit::Win32,
                            "Native Win32",
                        );
                    });
            });

            ui.horizontal(|ui| {
                ui.label("Mode");
                egui::ComboBox::from_id_salt("mode")
                    .selected_text(self.model.mode.label())
                    .show_ui(ui, |ui| {
                        for m in [
                            WallpaperMode::Auto,
                            WallpaperMode::Video,
                            WallpaperMode::Parallax,
                        ] {
                            ui.selectable_value(&mut self.model.mode, m, m.label());
                        }
                    });
            });

            ui.horizontal(|ui| {
                ui.label("Default source");
                ui.text_edit_singleline(&mut self.model.video);
                if ui.button("Browse…").clicked() {
                    if let Ok(Some(path)) = crate::dialog::browse_wallpaper_source() {
                        self.model.video = path.display().to_string();
                    }
                }
            });
            ui.label("Video file, parallax .zip, or color image next to a *-depth map.");

            ui.checkbox(&mut self.model.loop_playback, "Loop (video)");
            ui.checkbox(&mut self.model.mute, "Mute (video)");
            ui.checkbox(&mut self.model.preview, "Preview window (not desktop)");
            ui.checkbox(&mut self.model.startup, "Start with Windows");
            ui.checkbox(&mut self.model.expand, "self.expand (self-extract)");
            ui.checkbox(&mut self.model.time_sync, "Multi-monitor time sync");

            ui.horizontal(|ui| {
                ui.label("Backend");
                egui::ComboBox::from_id_salt("backend")
                    .selected_text(self.model.backend.label())
                    .show_ui(ui, |ui| {
                        for b in [
                            BackendPref::Default,
                            BackendPref::Auto,
                            BackendPref::Mpv,
                            BackendPref::Ffmpeg,
                        ] {
                            ui.selectable_value(&mut self.model.backend, b, b.label());
                        }
                    });
            });

            ui.horizontal(|ui| {
                ui.label("Fit");
                egui::ComboBox::from_id_salt("fit")
                    .selected_text(self.model.fit.label())
                    .show_ui(ui, |ui| {
                        for f in [
                            FitMode::Fill,
                            FitMode::Contain,
                            FitMode::Stretch,
                            FitMode::Native,
                        ] {
                            ui.selectable_value(&mut self.model.fit, f, f.label());
                        }
                    });
            });

            ui.horizontal(|ui| {
                ui.label("Max decode W");
                ui.add(egui::DragValue::new(&mut self.model.max_decode_w).speed(10));
                ui.label("H");
                ui.add(egui::DragValue::new(&mut self.model.max_decode_h).speed(10));
            });

            ui.collapsing("Monitors (per-display video)", |ui| {
                let ids: Vec<_> = self.model.monitors.iter().map(|m| m.id.clone()).collect();
                for id in ids {
                    let label = self
                        .model
                        .monitors
                        .iter()
                        .find(|m| m.id == id)
                        .map(|m| {
                            format!(
                                "{} {}x{}{}",
                                m.name,
                                m.rect.right - m.rect.left,
                                m.rect.bottom - m.rect.top,
                                if m.is_primary { " (primary)" } else { "" }
                            )
                        })
                        .unwrap_or_else(|| id.clone());

                    ui.group(|ui| {
                        let mut checked = self.model.monitor_checked(&id);
                        if ui.checkbox(&mut checked, &label).changed() {
                            self.model.set_monitor_checked(&id, checked);
                        }
                        ui.horizontal(|ui| {
                            ui.label("Video");
                            let mut path = self.model.per_monitor_path(&id);
                            if ui.text_edit_singleline(&mut path).changed() {
                                self.model.set_per_monitor_path(&id, path.clone());
                            }
                            if ui.button("Browse…").clicked() {
                                if let Ok(Some(p)) = crate::dialog::browse_wallpaper_source() {
                                    self.model
                                        .set_per_monitor_path(&id, p.display().to_string());
                                }
                            }
                            if ui.button("Clear").clicked() {
                                self.model.set_per_monitor_path(&id, String::new());
                            }
                        });
                        ui.label("Empty = use default video");
                    });
                }
            });

            ui.collapsing("Parallax (depth map)", |ui| {
                ui.add(
                    egui::Slider::new(&mut self.model.parallax.strength, 0.0..=0.15)
                        .text("Strength"),
                );
                ui.checkbox(&mut self.model.parallax.invert_depth, "Invert depth");
                ui.label("Pack format: color.jpg + color-depth.jpg (or a .zip of both).");
                ui.label("Example: wallpapers/parallax/argent-tower.zip");
            });

            ui.collapsing("Color grade + LUT", |ui| {
                ui.checkbox(&mut self.model.grade.enabled, "Enable color grade");
                ui.add(
                    egui::Slider::new(&mut self.model.grade.brightness, -0.5..=0.5)
                        .text("Brightness"),
                );
                ui.add(
                    egui::Slider::new(&mut self.model.grade.contrast, 0.5..=1.5).text("Contrast"),
                );
                ui.add(
                    egui::Slider::new(&mut self.model.grade.saturation, 0.0..=2.0)
                        .text("Saturation"),
                );
                ui.add(egui::Slider::new(&mut self.model.grade.gamma, 0.5..=1.5).text("Gamma"));
                ui.horizontal(|ui| {
                    ui.label("LUT");
                    ui.text_edit_singleline(&mut self.model.grade.lut);
                    if ui.button("Browse…").clicked() {
                        if let Ok(Some(path)) = crate::dialog::browse_lut() {
                            self.model.grade.lut = path.display().to_string();
                        }
                    }
                    if ui.button("Clear").clicked() {
                        self.model.grade.lut.clear();
                    }
                });
                ui.label("Supports .cube / .3dl (ffmpeg lut3d). Put files in luts\\.");
                ui.checkbox(
                    &mut self.model.grade.bake,
                    "Bake grade to cache\\graded (slower first Apply, lighter playback)",
                );
            });

            ui.collapsing("Unlock animation", |ui| {
                ui.checkbox(
                    &mut self.model.unlock.pause_when_locked,
                    "Pause / hide when locked",
                );
                ui.checkbox(&mut self.model.unlock.fade_on_unlock, "Fade in on unlock");
                ui.horizontal(|ui| {
                    ui.label("Fade ms");
                    ui.add(egui::DragValue::new(&mut self.model.unlock.fade_ms).range(50..=5000));
                });
            });

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Apply").clicked() {
                    match self.model.apply() {
                        Ok(_) => {
                            self.status = if self.model.toolkit != UiToolkit::Egui {
                                "Saved. Toolkit changed — reopen Settings.".into()
                            } else {
                                "Saved.".into()
                            };
                            tray::notify_settings_applied();
                        }
                        Err(err) => self.status = format!("Error: {err:#}"),
                    }
                }
                if ui.button("Close").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            if !self.status.is_empty() {
                ui.label(&self.status);
            }
        });
    }
}
