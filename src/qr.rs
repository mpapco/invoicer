//! The PayMe payment QR code, built with the `payme-qr` crate.

use payme_qr::qr::Matrix;
use payme_qr::{Payment, Preset, reference};

use crate::model::Invoice;

/// The payment the QR code carries: the supplier's account, the total and the
/// invoice number as the variable symbol. It deliberately carries no date, so
/// the payer's bank does not schedule the payment for the due date.
pub fn payment(invoice: &Invoice) -> Payment {
    let form = &invoice.form;
    let variable_symbol = reference::digits_only(&form.invoice_number);
    let reference = if variable_symbol.len() > 10 {
        String::new()
    } else {
        reference::build(&reference::Symbols {
            vs: variable_symbol,
            ..reference::Symbols::default()
        })
    };
    let message = if form.qr_message.trim().is_empty() {
        format!("Faktura {}", form.invoice_number.trim())
    } else {
        form.qr_message.trim().to_string()
    };
    Payment {
        preset: Preset::Invoice,
        iban: form.iban.clone(),
        creditor_name: form.supplier.name.clone(),
        amount: format!("{:.2}", invoice.total),
        reference,
        message,
        ..Payment::default()
    }
}

pub fn matrix(invoice: &Invoice) -> Result<Matrix, Vec<String>> {
    payme_qr::generate(&payment(invoice))
        .map(|(_, matrix)| matrix)
        .map_err(|errors| errors.iter().map(|e| format!("QR code: {e}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    #[test]
    fn the_link_carries_no_date() {
        let invoice = Invoice::compute(&fixtures::service()).unwrap();
        let (link, _) = payme_qr::generate(&payment(&invoice)).unwrap();
        assert!(!link.contains("DT="), "{link}");
        assert!(link.contains("AM=358.66"), "{link}");
        assert!(link.contains("PI=%2FVS2026015"), "{link}");
    }
}
