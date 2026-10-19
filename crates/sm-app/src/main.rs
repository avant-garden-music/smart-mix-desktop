//! Smart Mix — punto de entrada.

mod app;
mod texts;
mod theme;
mod usb;
mod widgets;
mod worker;

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .init();

    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1320.0, 840.0])
            .with_min_inner_size([980.0, 640.0])
            .with_title("Smart Mix — Avant Garden Music"),
        // glow: wgpu 0.19 entra en pánico al redimensionar (Surface::configure con
// tamaños corruptos bajo DPI fraccional); glow es igualmente acelerado y
// estable en resize. Revisar al subir a egui 0.28+.
renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Smart Mix",
        opts,
        Box::new(|cc| match app::SmartMixApp::new(cc) {
            Ok(a) => Box::new(a),
            Err(e) => {
                eprintln!("Error iniciando Smart Mix: {e:#}");
                std::process::exit(1);
            }
        }),
    )
}
