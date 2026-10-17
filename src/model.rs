//! The invoice as typed into the form, and the amounts computed from it.
//!
//! Everything the user types is kept as text so that a half-typed value never
//! gets lost; it is parsed and validated only when the invoice is computed.

use chrono::{Datelike, Duration, Local, NaiveDate};
use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};

/// Days from the invoice date to the due date in the empty "Default"
/// template, and so in every template created from it. Templates already
/// saved keep their own value, which the form edits ("make it due after").
pub const DEFAULT_DUE_DAYS: u32 = 21;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Party {
    pub name: String,
    pub street: String,
    pub city: String,
    /// IČO / company ID.
    pub ico: String,
    /// DIČ / tax ID.
    pub dic: String,
    /// IČ DPH / VAT ID.
    pub ic_dph: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Item {
    pub description: String,
    pub quantity: String,
    pub unit: String,
    pub unit_price: String,
    /// Used when quantity and unit price do not both give it.
    pub net_amount: String,
    pub vat_rate: String,
}

impl Item {
    /// A line nothing was typed into; it is left out of the invoice. The VAT
    /// rate does not count, as new lines inherit it.
    pub fn is_blank(&self) -> bool {
        [&self.description, &self.quantity, &self.unit, &self.unit_price, &self.net_amount]
            .iter()
            .all(|s| s.trim().is_empty())
    }

    /// Quantity × unit price, when both are numbers.
    pub fn computed_net(&self) -> Option<Decimal> {
        Some(round2(parse_decimal(&self.quantity)? * parse_decimal(&self.unit_price)?))
    }
}

/// Which date a template proposes as the invoice date when it is loaded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DateRule {
    #[default]
    Today,
    /// Billing the previous month: dated, delivered and covering up to its last day.
    EndOfPreviousMonth,
}

impl DateRule {
    pub const ALL: [DateRule; 2] = [DateRule::Today, DateRule::EndOfPreviousMonth];

    pub fn label(self) -> &'static str {
        match self {
            DateRule::Today => "today",
            DateRule::EndOfPreviousMonth => "last day of previous month",
        }
    }
}

/// Everything on the invoice. A field left empty is left out of the PDF.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Form {
    pub customer: Party,
    pub supplier: Party,
    pub iban: String,

    pub invoice_number: String,
    pub order_number: String,
    pub invoice_date: String,
    pub due_date: String,
    pub delivery_date: String,
    pub date_rule: DateRule,
    /// Days from the invoice date to the due date, applied when a template is loaded.
    pub due_days: u32,

    pub items: Vec<Item>,

    pub include_qr: bool,
    pub qr_message: String,

    /// The paragraph, printed
    /// when the agreement date is filled in.
    pub agreement_date: String,
    pub period_from: String,
    pub period_to: String,

    pub header_text: String,
    pub footer_text: String,
}

impl Default for Form {
    /// The empty "Default" template.
    fn default() -> Self {
        Form {
            customer: Party::default(),
            supplier: Party::default(),
            iban: String::new(),
            invoice_number: String::new(),
            order_number: String::new(),
            invoice_date: String::new(),
            due_date: String::new(),
            delivery_date: String::new(),
            date_rule: DateRule::Today,
            due_days: DEFAULT_DUE_DAYS,
            items: vec![Item::default()],
            include_qr: false,
            qr_message: String::new(),
            agreement_date: String::new(),
            period_from: String::new(),
            period_to: String::new(),
            header_text: String::new(),
            footer_text: String::new(),
        }
    }
}

impl Form {
    /// Fill the dates from `today` the way the template formulas do: dated by
    /// the date rule, due `due_days` later. The delivery date and the billing
    /// period are only refreshed when the template uses them.
    pub fn reset_dates(&mut self, today: NaiveDate) {
        let end_of_prev = today.with_day(1).unwrap() - Duration::days(1);
        let issued = match self.date_rule {
            DateRule::Today => today,
            DateRule::EndOfPreviousMonth => end_of_prev,
        };
        self.invoice_date = format_date(issued);
        self.due_date = format_date(issued + Duration::days(self.due_days.into()));
        if !self.delivery_date.trim().is_empty() {
            self.delivery_date = format_date(match self.date_rule {
                DateRule::Today => today,
                DateRule::EndOfPreviousMonth => end_of_prev,
            });
        }
        if !self.period_from.trim().is_empty() || !self.period_to.trim().is_empty() {
            self.period_from = format_date(end_of_prev.with_day(1).unwrap());
            self.period_to = format_date(end_of_prev);
        }
    }

    /// Grow or shrink the item list, keeping what was typed into the lines that stay.
    pub fn set_item_count(&mut self, count: usize) {
        let template = Item {
            vat_rate: self.items.last().map(|i| i.vat_rate.clone()).unwrap_or_default(),
            unit: self.items.last().map(|i| i.unit.clone()).unwrap_or_default(),
            ..Item::default()
        };
        self.items.resize(count.max(1), template);
    }
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// `30.9.2026`, as the templates print dates.
pub fn format_date(date: NaiveDate) -> String {
    format!("{}.{}.{}", date.day(), date.month(), date.year())
}

/// Accepts `30.9.2026`, `30. 9. 2026` and `2026-09-30`.
pub fn parse_date(text: &str) -> Option<NaiveDate> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    NaiveDate::parse_from_str(&compact, "%d.%m.%Y")
        .or_else(|_| NaiveDate::parse_from_str(&compact, "%Y-%m-%d"))
        .ok()
}

/// Accepts `1 234,50`, `1234.5`, `38,50 €`, `23 %`.
pub fn parse_decimal(text: &str) -> Option<Decimal> {
    let cleaned: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '€' | '%' | '\u{a0}' | '\u{202f}'))
        .map(|c| if c == ',' { '.' } else { c })
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    cleaned.parse().ok()
}

/// Why `text` is not a valid company ID, or `None` when it is. A company ID has eight
/// digits (spaces between them are allowed); the last is a check digit:
/// the first seven weighted 8..2 and summed, it is `(11 - sum % 11) % 10`.
pub fn ico_error(text: &str) -> Option<String> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.len() != 8 || !compact.bytes().all(|b| b.is_ascii_digit()) {
        return Some(format!("\"{}\" must be 8 digits.", text.trim()));
    }
    let digits: Vec<u32> = compact.bytes().map(|b| u32::from(b - b'0')).collect();
    let sum: u32 = digits[..7].iter().zip((2..=8).rev()).map(|(d, w)| d * w).sum();
    if (11 - sum % 11) % 10 != digits[7] {
        return Some(format!("\"{}\" has a wrong check digit; probably a typo.", text.trim()));
    }
    None
}

fn round2(value: Decimal) -> Decimal {
    value.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
}

/// Group digits by three with a no-break space and use a decimal comma.
fn format_number(value: Decimal, decimals: u32) -> String {
    let value = value.round_dp_with_strategy(decimals, RoundingStrategy::MidpointAwayFromZero);
    let text = format!("{:.*}", decimals as usize, value.abs());
    let (int, frac) = text.split_once('.').unwrap_or((&text, ""));
    let mut grouped = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push('\u{a0}');
        }
        grouped.push(c);
    }
    let sign = if value.is_sign_negative() && !value.is_zero() { "-" } else { "" };
    if frac.is_empty() {
        format!("{sign}{grouped}")
    } else {
        format!("{sign}{grouped},{frac}")
    }
}

/// `6 776,00 €`
pub fn format_money(value: Decimal) -> String {
    format!("{}\u{a0}€", format_number(value, 2))
}

/// `23,00 %`
pub fn format_percent(value: Decimal) -> String {
    format!("{}\u{a0}%", format_number(value, 2))
}

/// A quantity without trailing zeros: `176`, `7,5`.
pub fn format_quantity(value: Decimal) -> String {
    let normalized = value.normalize();
    format_number(normalized, normalized.scale())
}

/// A VAT rate as used inside labels: `23`, `5,5`.
pub fn format_rate(value: Decimal) -> String {
    format_quantity(value)
}

/// One computed invoice line.
#[derive(Debug, Clone)]
pub struct Line {
    pub description: String,
    pub quantity: Option<Decimal>,
    pub unit: String,
    pub unit_price: Option<Decimal>,
    pub net: Decimal,
    pub vat_rate: Decimal,
    pub vat: Decimal,
    pub subtotal: Decimal,
}

/// Net and VAT summed over the lines that share one rate.
#[derive(Debug, Clone)]
pub struct RateTotal {
    pub rate: Decimal,
    pub net: Decimal,
    pub vat: Decimal,
}

/// The invoice with every value parsed and every amount computed.
#[derive(Debug, Clone)]
pub struct Invoice {
    pub form: Form,
    pub invoice_date: NaiveDate,
    pub due_date: NaiveDate,
    pub delivery_date: Option<NaiveDate>,
    pub lines: Vec<Line>,
    pub rates: Vec<RateTotal>,
    pub total: Decimal,
    /// Whether any line uses quantity, unit or unit price, so that the
    /// table shows those columns.
    pub show_quantity: bool,
}

impl Invoice {
    /// Parse and compute; on failure every problem is reported at once.
    pub fn compute(form: &Form) -> Result<Invoice, Vec<String>> {
        let mut errors = Vec::new();

        let mut date = |label: &str, text: &str| match parse_date(text) {
            Some(d) => Some(d),
            None => {
                errors.push(format!("{label}: \"{text}\" is not a date (use d.m.yyyy)."));
                None
            }
        };
        let invoice_date = date("Invoice date", &form.invoice_date);
        let due_date = date("Due date", &form.due_date);
        let delivery_date = if form.delivery_date.trim().is_empty() {
            None
        } else {
            date("Delivery date", &form.delivery_date)
        };
        if !form.agreement_date.trim().is_empty() {
            for (label, text) in [
                ("Period from", &form.period_from),
                ("Period to", &form.period_to),
            ] {
                if !text.trim().is_empty() {
                    date(label, text);
                }
            }
        }

        for (label, value) in [
            ("Customer name", &form.customer.name),
            ("Supplier name", &form.supplier.name),
            ("Invoice number", &form.invoice_number),
        ] {
            if value.trim().is_empty() {
                errors.push(format!("{label} is empty."));
            }
        }
        for (label, ico) in [
            ("Customer IČO", &form.customer.ico),
            ("Supplier IČO", &form.supplier.ico),
        ] {
            if let Some(problem) = ico_error(ico).filter(|_| !ico.trim().is_empty()) {
                errors.push(format!("{label}: {problem}"));
            }
        }

        let mut lines = Vec::new();
        for (i, item) in form.items.iter().enumerate() {
            if item.is_blank() {
                continue;
            }
            let n = i + 1;
            let mut number = |what: &str, text: &str| {
                if text.trim().is_empty() {
                    return None;
                }
                let value = parse_decimal(text);
                if value.is_none() {
                    errors.push(format!("Line {n}: {what} \"{text}\" is not a number."));
                }
                value
            };
            let quantity = number("quantity", &item.quantity);
            let unit_price = number("unit price", &item.unit_price);
            let typed_net = number("net amount", &item.net_amount).map(round2);
            let vat_rate = number("VAT %", &item.vat_rate);
            let net = item.computed_net().or(typed_net);
            if net.is_none() && item.net_amount.trim().is_empty() {
                errors.push(format!(
                    "Line {n}: fill in quantity and unit price, or the net amount."
                ));
            }
            if vat_rate.is_none() && item.vat_rate.trim().is_empty() {
                errors.push(format!("Line {n}: VAT % is empty."));
            }
            if let (Some(net), Some(rate)) = (net, vat_rate) {
                let vat = round2(net * rate / Decimal::ONE_HUNDRED);
                lines.push(Line {
                    description: item.description.trim().to_string(),
                    quantity,
                    unit: item.unit.trim().to_string(),
                    unit_price,
                    net,
                    vat_rate: rate,
                    vat,
                    subtotal: net + vat,
                });
            }
        }
        if lines.is_empty() && errors.is_empty() {
            errors.push("There are no items.".into());
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        let mut rates: Vec<RateTotal> = Vec::new();
        for line in &lines {
            match rates.iter_mut().find(|r| r.rate == line.vat_rate) {
                Some(r) => {
                    r.net += line.net;
                    r.vat += line.vat;
                }
                None => rates.push(RateTotal {
                    rate: line.vat_rate,
                    net: line.net,
                    vat: line.vat,
                }),
            }
        }
        rates.sort_by(|a, b| b.rate.cmp(&a.rate));
        let total = lines.iter().map(|l| l.subtotal).sum();
        let show_quantity = lines
            .iter()
            .any(|l| l.quantity.is_some() || l.unit_price.is_some() || !l.unit.is_empty());

        Ok(Invoice {
            form: form.clone(),
            invoice_date: invoice_date.unwrap(),
            due_date: due_date.unwrap(),
            delivery_date,
            lines,
            rates,
            total,
            show_quantity,
        })
    }

    /// `01.09.2026-30.09.2026`
    pub fn period(&self) -> String {
        let show = |text: &str| match parse_date(text) {
            Some(d) => d.format("%d.%m.%Y").to_string(),
            None => text.trim().to_string(),
        };
        let (from, to) = (show(&self.form.period_from), show(&self.form.period_to));
        match (from.is_empty(), to.is_empty()) {
            (false, false) => format!("{from}-{to}"),
            _ => format!("{from}{to}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    fn dec(s: &str) -> Decimal {
        s.parse().unwrap()
    }

    #[test]
    fn formats_like_the_templates() {
        assert_eq!(format_money(dec("6776")), "6\u{a0}776,00\u{a0}€");
        assert_eq!(format_money(dec("291.59")), "291,59\u{a0}€");
        assert_eq!(format_money(dec("1234567.005")), "1\u{a0}234\u{a0}567,01\u{a0}€");
        assert_eq!(format_percent(dec("23")), "23,00\u{a0}%");
        assert_eq!(format_quantity(dec("176.00")), "176");
        assert_eq!(format_quantity(dec("7.50")), "7,5");
        assert_eq!(format_date(NaiveDate::from_ymd_opt(2026, 9, 30).unwrap()), "30.9.2026");
    }

    #[test]
    fn checks_ico() {
        for valid in ["12345679", "87654326", "123 456 79", " 00151742 "] {
            assert_eq!(ico_error(valid), None, "{valid}");
        }
        // A check digit of 0 or 1 comes from remainders 1 and 0.
        assert_eq!(ico_error("00000001"), None);
        // Too short, too long, a letter O, a wrong last digit, two digits swapped.
        for invalid in ["1234567", "123456790", "1234567O", "", "12345678", "21345679"] {
            assert!(ico_error(invalid).is_some(), "{invalid}");
        }
    }

    #[test]
    fn invalid_ico_blocks_the_invoice_but_empty_is_fine() {
        let mut form = fixtures::service();
        form.customer.ico = "87654325".into();
        let errors = Invoice::compute(&form).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].starts_with("Customer IČO:"), "{errors:?}");

        form.customer.ico.clear();
        assert!(Invoice::compute(&form).is_ok());
    }

    #[test]
    fn parses_loose_input() {
        assert_eq!(parse_decimal("38,50 €"), Some(dec("38.50")));
        assert_eq!(parse_decimal("6 776,00"), Some(dec("6776.00")));
        assert_eq!(parse_decimal("23 %"), Some(dec("23")));
        assert_eq!(parse_decimal(""), None);
        assert_eq!(parse_date("30. 9. 2026"), NaiveDate::from_ymd_opt(2026, 9, 30));
        assert_eq!(parse_date("2026-09-30"), NaiveDate::from_ymd_opt(2026, 9, 30));
    }

    #[test]
    fn quantity_times_unit_price() {
        let inv = Invoice::compute(&fixtures::hourly()).unwrap();
        assert!(inv.show_quantity);
        assert_eq!(inv.period(), "01.09.2026-30.09.2026");
        assert_eq!(inv.lines[0].net, dec("6776.00"));
        assert_eq!(inv.lines[0].vat, dec("1558.48"));
        assert_eq!(inv.total, dec("8334.48"));
    }

    #[test]
    fn typed_net_amount() {
        let inv = Invoice::compute(&fixtures::service()).unwrap();
        assert!(!inv.show_quantity);
        assert_eq!(inv.delivery_date, None);
        assert_eq!(inv.lines[0].vat, dec("67.07"));
        assert_eq!(inv.total, dec("358.66"));
    }

    #[test]
    fn groups_totals_by_rate() {
        let mut form = fixtures::service();
        form.set_item_count(3);
        form.items[0].net_amount = "100".into();
        form.items[1].net_amount = "50".into();
        form.items[2].net_amount = "10".into();
        form.items[2].vat_rate = "5".into();
        let inv = Invoice::compute(&form).unwrap();
        assert_eq!(inv.rates.len(), 2);
        assert_eq!(inv.rates[0].net, dec("150"));
        assert_eq!(inv.rates[1].vat, dec("0.50"));
        assert_eq!(inv.total, dec("195.00"));
    }

    #[test]
    fn previous_month_dates() {
        let mut form = fixtures::hourly();
        form.reset_dates(NaiveDate::from_ymd_opt(2026, 10, 11).unwrap());
        assert_eq!(form.invoice_date, "30.9.2026");
        assert_eq!(form.due_date, "21.10.2026");
        assert_eq!(form.delivery_date, "30.9.2026");
        assert_eq!(form.period_from, "1.9.2026");
        assert_eq!(form.period_to, "30.9.2026");
    }

    #[test]
    fn empty_fields_are_left_out() {
        let mut form = Form::default();
        form.reset_dates(NaiveDate::from_ymd_opt(2026, 10, 12).unwrap());
        assert_eq!(form.delivery_date, "");
        assert_eq!(form.period_from, "");
        form.customer.name = "C".into();
        form.supplier.name = "S".into();
        form.invoice_number = "1".into();
        assert_eq!(Invoice::compute(&form).unwrap_err(), vec!["There are no items."]);

        form.set_item_count(3);
        form.items[1].description = "Work".into();
        form.items[1].quantity = "2".into();
        form.items[1].net_amount = "100".into();
        form.items[1].vat_rate = "23".into();
        let inv = Invoice::compute(&form).unwrap();
        assert_eq!(inv.lines.len(), 1);
        // Quantity without unit price: the typed net amount is used.
        assert_eq!(inv.lines[0].net, dec("100"));
        assert!(inv.show_quantity);

        form.items[1].unit_price = "60".into();
        let inv = Invoice::compute(&form).unwrap();
        assert_eq!(inv.lines[0].net, dec("120.00"));
    }
}
