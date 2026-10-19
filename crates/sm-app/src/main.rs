//! Smart Mix — punto de entrada.

mod app;
mod texts;
mod theme;
mod widgets;
mod worker;

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .init();

    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1320.0, 840.0])
            .with_min_inner_size([980.0, 640.0])
            .with_title("Smart Mix — Avant Garden Music"),
        renderer: eframe::Renderer::Wgpu,
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
