//! Renders every saved template with today's dates, for checking the layout
//! without the GUI: `cargo run --example sample -- OUT_DIR`.
//!
//! A template that is not a complete invoice by itself (say, the hours are
//! filled in each month) is reported and skipped.

use invoicer::model::{Invoice, today};
use invoicer::store::{self, State};

fn main() -> anyhow::Result<()> {
    let out = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    let state = State::load();
    for name in state.template_names() {
        if store::is_default(&name) {
            continue;
        }
        let mut form = state.template(&name).expect("listed");
        form.reset_dates(today());
        if form.invoice_number.trim().is_empty() {
            form.invoice_number = "0000001".into();
        }
        let invoice = match Invoice::compute(&form) {
            Ok(invoice) => invoice,
            Err(errors) => {
                eprintln!("{name}: skipped: {}", errors.join(" "));
                continue;
            }
        };
        let qr = if form.include_qr {
            Some(invoicer::qr::matrix(&invoice).map_err(|e| anyhow::anyhow!(e.join("\n")))?)
        } else {
            None
        };
        let path = out.join(format!("sample_{}.pdf", name.replace(['/', ' '], "_")));
        invoicer::pdf::write(&invoice, qr.as_ref(), &path)?;
        println!("{}", path.display());
    }
    Ok(())
}
