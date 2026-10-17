use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Invoicer")
            .with_inner_size([1100.0, 860.0])
            .with_min_inner_size([700.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "invoicer",
        options,
        Box::new(|_cc| Ok(Box::new(invoicer::app::App::new()))),
    )
}
