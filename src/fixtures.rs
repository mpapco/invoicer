//! Fictional invoices for tests. Real customers and suppliers belong in the
//! user's saved templates, never in the code.

use crate::model::{DateRule, Form, Item, Party};

/// The sample IBAN of the PayMe Payment Link Standard.
pub const IBAN: &str = "SK68 0720 0002 8919 8742 6353";

pub fn supplier() -> Party {
    Party {
        name: "Example Supplier s.r.o.".into(),
        street: "Hlavná 1".into(),
        city: "040 01 Košice".into(),
        // A made-up number with a valid check digit.
        ico: "12345679".into(),
        dic: "2020000001".into(),
        ic_dph: String::new(),
    }
}

pub fn customer() -> Party {
    Party {
        name: "Example Customer s.r.o.".into(),
        street: "Obchodná 2".into(),
        city: "811 01 Bratislava".into(),
        ico: "87654326".into(),
        dic: "2020000002".into(),
        ic_dph: "SK2020000002".into(),
    }
}

/// One line with a typed net amount and a payment QR code:
/// 291,59 € + 23 % VAT = 358,66 €.
pub fn service() -> Form {
    Form {
        customer: customer(),
        supplier: supplier(),
        iban: IBAN.into(),
        invoice_number: "2026015".into(),
        invoice_date: "12.10.2026".into(),
        due_date: "2.11.2026".into(),
        items: vec![Item {
            description: "Administrative services".into(),
            net_amount: "291,59".into(),
            vat_rate: "23".into(),
            ..Item::default()
        }],
        include_qr: true,
        ..Form::default()
    }
}

/// Billing September 2026 by the hour, with delivery date and agreement
/// note: 176 × 38,50 € = 6 776,00 € + 23 % VAT = 8 334,48 €.
pub fn hourly() -> Form {
    Form {
        customer: customer(),
        supplier: supplier(),
        iban: IBAN.into(),
        invoice_number: "2026016".into(),
        order_number: "PO-2026-001".into(),
        invoice_date: "30.9.2026".into(),
        due_date: "21.10.2026".into(),
        delivery_date: "30.9.2026".into(),
        date_rule: DateRule::EndOfPreviousMonth,
        items: vec![Item {
            description: "Software development".into(),
            quantity: "176".into(),
            unit: "hod".into(),
            unit_price: "38,50".into(),
            vat_rate: "23".into(),
            ..Item::default()
        }],
        agreement_date: "1.3.2026".into(),
        period_from: "1.9.2026".into(),
        period_to: "30.9.2026".into(),
        ..Form::default()
    }
}
