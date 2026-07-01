#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use std::sync::Arc;

fn main() -> eframe::Result {
    env_logger::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1024.0, 1024.0]),
        multisampling: 4,
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        "glow-bloom — simple",
        options,
        Box::new(|cc| Ok(Box::new(SimpleApp::new(cc)))),
    )
}

struct SimpleApp {
    bloom_renderer: Arc<egui::mutex::Mutex<glow_bloom::bloom::BloomRenderer>>,
}

impl SimpleApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let gl = cc.gl.as_ref().expect("Need glow backend");
        let font_data = include_bytes!("../assets/Courier 10 Pitch Bold.otf").to_vec();
        let renderer = glow_bloom::bloom::BloomRenderer::new(gl, font_data);
        Self {
            bloom_renderer: Arc::new(egui::mutex::Mutex::new(renderer)),
        }
    }
}

impl eframe::App for SimpleApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::BLACK))
            .show_inside(ui, |ui| {
                ui.add(
                    glow_bloom::bloom::BloomText::new(
                        Arc::clone(&self.bloom_renderer),
                        "Hello, world!",
                    )
                    .intensity(1.2)
                    .font_scale(36.0),
                );
            });
    }

    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        if let Some(gl) = gl {
            self.bloom_renderer.lock().destroy(gl);
        }
    }
}
