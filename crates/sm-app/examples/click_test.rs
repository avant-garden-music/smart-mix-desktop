//! Test mínimo: ¿responden los widgets de egui a clics en esta máquina?

struct ClickTest {
    count: u32,
}

impl eframe::App for ClickTest {
    fn update(&mut self, ctx: &egui::Context, _f: &mut eframe::Frame) {
        if ctx.input(|i| i.pointer.any_click()) {
            eprintln!(
                "[test] ptr click {:?}",
                ctx.input(|i| i.pointer.interact_pos())
            );
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            let resp = ui.button(format!("clics: {}", self.count));
            if resp.clicked() {
                self.count += 1;
                eprintln!("[test] BUTTON CLICKED count={}", self.count);
            }
            // log de hover 1 vez por ~500 ms
            use std::sync::atomic::{AtomicU64, Ordering};
            static LAST: AtomicU64 = AtomicU64::new(0);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            let last = LAST.load(Ordering::Relaxed);
            if now - last > 500 {
                LAST.store(now, Ordering::Relaxed);
                eprintln!(
                    "[test] hovered={} rect={:?} ptr={:?} ppp={}",
                    resp.hovered(),
                    resp.rect,
                    ctx.input(|i| i.pointer.interact_pos()),
                    ctx.pixels_per_point()
                );
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 200.0])
            .with_position([100.0, 100.0])
            .with_title("CLICK TEST"),
        ..Default::default()
    };
    eframe::run_native(
        "clicktest",
        opts,
        Box::new(|_cc| Box::new(ClickTest { count: 0 })),
    )
}
