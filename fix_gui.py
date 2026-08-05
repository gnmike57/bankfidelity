import re

with open('src/app/gui.rs', 'r') as f:
    content = f.read()

old_code = """                    let engine_color = match engine_state {
                        "STALLED" => egui::Color32::from_rgb(255, 80, 80),
                        "BUSY" => egui::Color32::YELLOW,
                        _ => egui::Color32::LIGHT_GREEN,
                    };
                    ui.colored_label(engine_color, format!("ENG: {}", engine_state));"""

new_code = """                    let engine_color = match engine_state {
                        "STALLED" => egui::Color32::from_rgb(255, 80, 80),
                        "BUSY" => egui::Color32::YELLOW,
                        _ => egui::Color32::LIGHT_GREEN,
                    };
                    ui.colored_label(engine_color, format!("ENG: {}", engine_state));
                    ui.separator();
                    
                    let mut all_healthy = true;
                    if let Some(health) = &self.api_health {
                        for res in health {
                            if res.status == crate::app::api_verification::VerificationStatus::Failed {
                                all_healthy = false;
                            }
                        }
                    }
                    if self.api_health.is_some() {
                        if all_healthy {
                            ui.colored_label(egui::Color32::LIGHT_GREEN, "API: HEALTHY");
                        } else {
                            ui.colored_label(egui::Color32::from_rgb(255, 80, 80), "API: DEGRADED");
                        }
                    } else {
                        ui.colored_label(egui::Color32::GRAY, "API: UNKNOWN");
                    }"""

if old_code in content:
    content = content.replace(old_code, new_code)
    with open('src/app/gui.rs', 'w') as f:
        f.write(content)
    print("Patched draw_status_bar to include API health")
else:
    print("Old code not found")
