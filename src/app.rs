use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, RichText};
use egui_extras::DatePickerButton;

use crate::model::{
    DateRule, Form, Invoice, Party, format_date, format_money, format_percent, parse_date, today,
};
use crate::store::{self, DEFAULT_TEMPLATE, State};

const MAX_LINES: usize = 200;
const ERROR: Color32 = Color32::from_rgb(0xd0, 0x40, 0x40);
const WARNING: Color32 = Color32::from_rgb(0xc0, 0x80, 0x00);

pub struct App {
    state: State,
    form: Form,
    dialog: Option<Dialog>,
    status: Option<Status>,
}

enum Dialog {
    SavePdf { file_name: String, folder: String, errors: Vec<String> },
    SaveTemplate { name: String, error: Option<String> },
    DeleteTemplate { name: String },
}

enum Status {
    Saved(PathBuf),
    Info(String),
    Failed(String),
}

impl App {
    pub fn new() -> Self {
        let state = State::load();
        let form = state
            .draft
            .clone()
            .or_else(|| state.template(&state.selected))
            .unwrap_or_default();
        let mut app = App { state, form, dialog: None, status: None };
        app.prepare_form();
        app
    }

    /// Today's dates and the number after the last invoice issued.
    fn prepare_form(&mut self) {
        if self.form.items.is_empty() {
            self.form.set_item_count(1);
        }
        self.form.reset_dates(today());
        if !self.state.last_invoice_number.trim().is_empty() {
            self.form.invoice_number = store::next_number(&self.state.last_invoice_number);
        }
    }

    fn load_template(&mut self, name: &str) {
        if let Some(form) = self.state.template(name) {
            let number = self.form.invoice_number.clone();
            self.form = form;
            self.state.selected = name.to_string();
            self.prepare_form();
            // Keep a number typed by hand: the sequence is shared by all templates.
            if self.state.last_invoice_number.trim().is_empty() && !number.trim().is_empty() {
                self.form.invoice_number = number;
            }
            self.status = None;
        }
    }

    fn persist(&mut self) {
        self.state.draft = Some(self.form.clone());
        if let Err(e) = self.state.save() {
            self.status = Some(Status::Failed(format!("Could not save settings: {e:#}")));
        }
    }

    fn default_folder(&self) -> PathBuf {
        self.state
            .output_dir
            .clone()
            .or_else(dirs::document_dir)
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn default_file_name(&self) -> String {
        let clean = |s: &str| -> String {
            s.trim()
                .chars()
                .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' })
                .collect()
        };
        let number = clean(&self.form.invoice_number);
        if store::is_default(&self.state.selected) {
            format!("Invoice_{number}")
        } else {
            format!("Invoice_{}_{number}", clean(&self.state.selected))
        }
    }

    /// Validate, render and write the PDF; on success remember the number and folder.
    fn generate(&mut self, path: &Path) -> Result<(), Vec<String>> {
        let invoice = Invoice::compute(&self.form)?;
        let qr = if self.form.include_qr {
            Some(crate::qr::matrix(&invoice)?)
        } else {
            None
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| vec![format!("{}: {e}", dir.display())])?;
        }
        crate::pdf::write(&invoice, qr.as_ref(), path).map_err(|e| vec![format!("{e:#}")])?;

        self.state.last_invoice_number = self.form.invoice_number.trim().to_string();
        self.state.output_dir = path.parent().map(Path::to_path_buf);
        self.persist();
        Ok(())
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let computed = Invoice::compute(&self.form);
        let qr_check = match (&computed, self.form.include_qr) {
            (Ok(inv), true) => crate::qr::matrix(inv).err(),
            _ => None,
        };

        egui::Panel::top("templates").show(ui, |ui| {
            ui.add_space(4.0);
            self.template_bar(ui);
            ui.add_space(4.0);
        });

        egui::Panel::bottom("actions").show(ui, |ui| {
            ui.add_space(6.0);
            let mut problems: Vec<String> = computed.as_ref().err().cloned().unwrap_or_default();
            problems.extend(qr_check.iter().flatten().cloned());
            for p in &problems {
                ui.colored_label(ERROR, format!("• {p}"));
            }
            ui.horizontal(|ui| {
                let generate = ui.add_enabled(
                    problems.is_empty(),
                    egui::Button::new(RichText::new("Generate").strong().size(16.0))
                        .min_size(egui::vec2(120.0, 30.0)),
                );
                if generate.clicked() {
                    self.dialog = Some(Dialog::SavePdf {
                        file_name: self.default_file_name(),
                        folder: self.default_folder().display().to_string(),
                        errors: Vec::new(),
                    });
                }
                if let Ok(inv) = &computed {
                    ui.label(
                        RichText::new(format!("Suma na úhradu: {}", format_money(inv.total)))
                            .strong()
                            .color(Color32::from_rgb(0x00, 0x8a, 0x00)),
                    );
                }
                match &self.status {
                    Some(Status::Saved(path)) => {
                        ui.label(format!("Saved {}", path.display()));
                        if ui.button("Open").clicked() {
                            open_file(path);
                        }
                    }
                    Some(Status::Info(msg)) => {
                        ui.label(msg);
                    }
                    Some(Status::Failed(msg)) => {
                        ui.colored_label(ERROR, msg);
                    }
                    None => {}
                }
            });
            ui.add_space(6.0);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.weak("A field left empty is left out of the PDF.");
                ui.add_space(4.0);
                form_ui(ui, &mut self.form, computed.as_ref().ok());
            });
        });

        self.dialog_ui(ui.ctx());
    }

    fn on_exit(&mut self) {
        // Keep what was typed as a draft; the issued number only moves on Generate.
        self.state.draft = Some(self.form.clone());
        let _ = self.state.save();
    }
}

impl App {
    fn template_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Template:").strong());
            let mut chosen = None;
            egui::ComboBox::from_id_salt("template")
                .selected_text(&self.state.selected)
                .width(200.0)
                .show_ui(ui, |ui| {
                    for name in self.state.template_names() {
                        let label = if store::is_default(&name) {
                            format!("{name} (empty)")
                        } else {
                            name.clone()
                        };
                        if ui.selectable_label(self.state.selected == name, label).clicked() {
                            chosen = Some(name);
                        }
                    }
                });
            if let Some(name) = chosen {
                self.load_template(&name);
            }

            let is_default = store::is_default(&self.state.selected);
            if ui
                .button("Save as template…")
                .on_hover_text("Store the current form as a template, or update one")
                .clicked()
            {
                let name = if is_default { String::new() } else { self.state.selected.clone() };
                self.dialog = Some(Dialog::SaveTemplate { name, error: None });
            }
            if ui
                .add_enabled(!is_default, egui::Button::new("Delete template"))
                .on_disabled_hover_text(format!("\"{DEFAULT_TEMPLATE}\" is built in"))
                .clicked()
            {
                self.dialog = Some(Dialog::DeleteTemplate { name: self.state.selected.clone() });
            }
            if ui
                .button("Revert")
                .on_hover_text("Discard the edits and load the template again")
                .clicked()
            {
                let name = self.state.selected.clone();
                self.load_template(&name);
            }
        });
    }

    fn dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.dialog else {
            return;
        };
        enum Outcome {
            None,
            Close,
            SavePdf(PathBuf),
            SaveTemplate(String),
            DeleteTemplate(String),
        }
        let mut outcome = Outcome::None;
        let state = &self.state;

        let modal = egui::Modal::new(egui::Id::new("dialog")).show(ctx, |ui| {
            ui.set_width(520.0);
            match dialog {
                Dialog::SavePdf { file_name, folder, errors } => {
                    ui.heading("Save invoice as PDF");
                    ui.add_space(6.0);
                    egui::Grid::new("save_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                        ui.label("File name:");
                        let name = ui.add(egui::TextEdit::singleline(file_name).desired_width(380.0));
                        if ui.memory(|m| m.focused().is_none()) {
                            name.request_focus();
                        }
                        if name.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            if let Some(p) = target_path(folder, file_name) {
                                outcome = Outcome::SavePdf(p);
                            }
                        }
                        ui.end_row();
                        ui.label("Folder:");
                        ui.add(egui::TextEdit::singleline(folder).desired_width(380.0));
                        ui.end_row();
                    });
                    let path = target_path(folder, file_name);
                    if let Some(path) = &path {
                        ui.weak(path.display().to_string());
                        if path.exists() {
                            ui.colored_label(WARNING, "A file with this name exists and will be replaced.");
                        }
                    }
                    for e in errors.iter() {
                        ui.colored_label(ERROR, e);
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.add_enabled(path.is_some(), egui::Button::new("Save")).clicked() {
                            outcome = Outcome::SavePdf(path.clone().unwrap());
                        }
                        if ui.button("Cancel").clicked() {
                            outcome = Outcome::Close;
                        }
                    });
                }
                Dialog::SaveTemplate { name, error } => {
                    ui.heading("Save as template");
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        let edit = ui.add(egui::TextEdit::singleline(name).desired_width(300.0));
                        if ui.memory(|m| m.focused().is_none()) {
                            edit.request_focus();
                        }
                        if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            outcome = Outcome::SaveTemplate(name.clone());
                        }
                    });
                    if !store::is_default(name) && state.exists(name) {
                        ui.colored_label(WARNING, format!("Template \"{}\" will be updated.", name.trim()));
                    }
                    if let Some(e) = error {
                        ui.colored_label(ERROR, e.as_str());
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            outcome = Outcome::SaveTemplate(name.clone());
                        }
                        if ui.button("Cancel").clicked() {
                            outcome = Outcome::Close;
                        }
                    });
                }
                Dialog::DeleteTemplate { name } => {
                    ui.heading("Delete template");
                    ui.add_space(6.0);
                    ui.label(format!("Delete the template \"{name}\"? This cannot be undone."));
                    ui.label("The form keeps its current values.");
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button(RichText::new("Delete").color(ERROR)).clicked() {
                            outcome = Outcome::DeleteTemplate(name.clone());
                        }
                        if ui.button("Cancel").clicked() {
                            outcome = Outcome::Close;
                        }
                    });
                }
            }
        });
        if matches!(outcome, Outcome::None) && modal.should_close() {
            outcome = Outcome::Close;
        }

        match outcome {
            Outcome::None => {}
            Outcome::Close => self.dialog = None,
            Outcome::SavePdf(path) => match self.generate(&path) {
                Ok(()) => {
                    self.status = Some(Status::Saved(path));
                    self.dialog = None;
                }
                Err(errs) => {
                    if let Some(Dialog::SavePdf { errors, .. }) = &mut self.dialog {
                        *errors = errs;
                    }
                }
            },
            Outcome::SaveTemplate(name) => match self.state.save_template(&name, &self.form) {
                Ok(()) => {
                    self.persist();
                    self.status = Some(Status::Info(format!("Template \"{}\" saved.", name.trim())));
                    self.dialog = None;
                }
                Err(e) => {
                    if let Some(Dialog::SaveTemplate { error, .. }) = &mut self.dialog {
                        *error = Some(e);
                    }
                }
            },
            Outcome::DeleteTemplate(name) => {
                self.state.delete_template(&name);
                self.persist();
                self.status = Some(Status::Info(format!("Template \"{name}\" deleted.")));
                self.dialog = None;
            }
        }
    }
}

/// `folder/name.pdf`, adding the extension when it was left out.
fn target_path(folder: &str, file_name: &str) -> Option<PathBuf> {
    let name = file_name.trim();
    if name.is_empty() || name.contains('/') {
        return None;
    }
    let name = if name.to_ascii_lowercase().ends_with(".pdf") {
        name.to_string()
    } else {
        format!("{name}.pdf")
    };
    let folder = folder.trim();
    let folder = match folder.strip_prefix("~/") {
        Some(rest) => dirs::home_dir()?.join(rest),
        None if folder.is_empty() => PathBuf::from("."),
        None => PathBuf::from(folder),
    };
    Some(folder.join(name))
}

fn open_file(path: &Path) {
    #[cfg(target_os = "linux")]
    let opener = "xdg-open";
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(target_os = "windows")]
    let opener = "explorer";
    let _ = std::process::Command::new(opener).arg(path).spawn();
}

fn section(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(RichText::new(title).strong().size(15.0))
        .default_open(true)
        .show(ui, add);
    ui.add_space(4.0);
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String, width: f32) {
    ui.label(label);
    ui.add(egui::TextEdit::singleline(value).desired_width(width));
    ui.end_row();
}

/// A date typed as text, with a calendar button next to it that fills the
/// text in. The calendar opens on the typed date, or today if there is none.
fn date_edit(ui: &mut egui::Ui, id: &str, value: &mut String) {
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(value)
                .hint_text("d.m.yyyy")
                .desired_width(110.0),
        );
        let shown = parse_date(value).unwrap_or_else(today);
        let mut picked = to_jiff(shown);
        let response = ui.add(
            DatePickerButton::new(&mut picked)
                .id_salt(id)
                .format("")
                .calendar_week(true)
                .highlight_weekends(true),
        );
        if response.changed() {
            if let Some(date) = from_jiff(picked) {
                *value = format_date(date);
            }
        }
        if !value.trim().is_empty() && parse_date(value).is_none() {
            ui.colored_label(ERROR, "not a date");
        }
    });
}

fn date_row(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.label(label);
    date_edit(ui, label, value);
    ui.end_row();
}

fn to_jiff(date: chrono::NaiveDate) -> jiff::civil::Date {
    use chrono::Datelike;
    jiff::civil::Date::new(date.year() as i16, date.month() as i8, date.day() as i8)
        .unwrap_or_default()
}

fn from_jiff(date: jiff::civil::Date) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::from_ymd_opt(date.year().into(), date.month() as u32, date.day() as u32)
}

fn party_ui(ui: &mut egui::Ui, id: &str, party: &mut Party, iban: Option<&mut String>) {
    egui::Grid::new(id).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        field(ui, "Name", &mut party.name, 280.0);
        field(ui, "Street", &mut party.street, 280.0);
        field(ui, "City", &mut party.city, 280.0);
        field(ui, "IČO/IN", &mut party.ico, 280.0);
        field(ui, "DIČ/VAT", &mut party.dic, 280.0);
        field(ui, "IČ DPH", &mut party.ic_dph, 280.0);
        if let Some(iban) = iban {
            field(ui, "Účet/Account", iban, 280.0);
        }
    });
}

fn form_ui(ui: &mut egui::Ui, form: &mut Form, computed: Option<&Invoice>) {
    section(ui, "Invoice", |ui| {
        egui::Grid::new("invoice").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            field(ui, "Invoice number", &mut form.invoice_number, 200.0);
            field(ui, "Order number", &mut form.order_number, 200.0);
            date_row(ui, "Invoice date", &mut form.invoice_date);
            date_row(ui, "Due date", &mut form.due_date);
            date_row(ui, "Delivery date", &mut form.delivery_date);
        });
        ui.horizontal(|ui| {
            ui.weak("When this template is loaded, date the invoice");
            egui::ComboBox::from_id_salt("date_rule")
                .selected_text(form.date_rule.label())
                .show_ui(ui, |ui| {
                    for rule in DateRule::ALL {
                        ui.selectable_value(&mut form.date_rule, rule, rule.label());
                    }
                });
            ui.weak("and make it due after");
            ui.add(egui::DragValue::new(&mut form.due_days).range(0..=365));
            ui.weak("days.");
        });
    });

    section(ui, "Customer / Supplier", |ui| {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Zákazník/Customer").strong());
                party_ui(ui, "customer", &mut form.customer, None);
            });
            ui.add_space(24.0);
            ui.vertical(|ui| {
                ui.label(RichText::new("Dodávateľ/Supplier").strong());
                party_ui(ui, "supplier", &mut form.supplier, Some(&mut form.iban));
            });
        });
    });

    section(ui, "Items", |ui| {
        ui.horizontal(|ui| {
            ui.label("Number of lines:");
            let mut count = form.items.len();
            if ui
                .add(egui::DragValue::new(&mut count).range(1..=MAX_LINES).speed(0.1))
                .changed()
            {
                form.set_item_count(count);
            }
        });
        ui.weak(
            "Empty lines are skipped. Net amount is quantity × unit price when both are given. \
             Quantity, unit and unit price columns are printed only when some line uses them.",
        );
        ui.add_space(4.0);
        items_ui(ui, form, computed);
    });

    section(ui, "Payment QR code", |ui| {
        ui.checkbox(&mut form.include_qr, "Include PayMe QR code");
        if form.include_qr {
            ui.horizontal(|ui| {
                ui.label("Message:");
                ui.add(
                    egui::TextEdit::singleline(&mut form.qr_message)
                        .hint_text(format!("Faktura {}", form.invoice_number.trim()))
                        .desired_width(300.0),
                );
            });
            ui.weak("Pays the total to the supplier's account, with the invoice number as variable symbol (no date).");
        }
    });

    section(ui, "Agreement note", |ui| {
        ui.weak("\"Fakturujeme Vám služby na základe zmluvy zo dňa … za obdobie …\" — printed when the agreement date is filled in.");
        egui::Grid::new("note_grid").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            date_row(ui, "Agreement date", &mut form.agreement_date);
            date_row(ui, "Period from", &mut form.period_from);
            date_row(ui, "Period to", &mut form.period_to);
        });
    });

    section(ui, "Header / Footer", |ui| {
        egui::Grid::new("header_footer").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label("Header");
            ui.add(egui::TextEdit::multiline(&mut form.header_text).desired_rows(2).desired_width(500.0));
            ui.end_row();
            ui.label("Footer");
            ui.add(egui::TextEdit::multiline(&mut form.footer_text).desired_rows(2).desired_width(500.0));
            ui.end_row();
        });
    });
}

fn items_ui(ui: &mut egui::Ui, form: &mut Form, computed: Option<&Invoice>) {
    let mut remove = None;
    let can_remove = form.items.len() > 1;
    egui::Grid::new("items")
        .num_columns(10)
        .spacing([6.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            for h in [
                "#", "Popis/Description", "Počet/Quantity", "M.j./Unit", "Cena za m.j./Unit price",
                "Cena bez DPH/Net amount", "% DPH/% VAT", "DPH/VAT", "Suma/Subtotal", "",
            ] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();

            // Computed lines exist only for non-blank items, in order.
            let mut lines = computed.map(|c| c.lines.iter());
            for (i, item) in form.items.iter_mut().enumerate() {
                let line = if item.is_blank() { None } else { lines.as_mut().and_then(Iterator::next) };
                ui.label(format!("{}", i + 1));
                ui.add_sized([260.0, 22.0], egui::TextEdit::singleline(&mut item.description));
                ui.add_sized([70.0, 22.0], egui::TextEdit::singleline(&mut item.quantity));
                ui.add_sized([50.0, 22.0], egui::TextEdit::singleline(&mut item.unit));
                ui.add_sized([90.0, 22.0], egui::TextEdit::singleline(&mut item.unit_price));
                match item.computed_net() {
                    Some(net) => {
                        ui.add_sized([100.0, 22.0], egui::Label::new(RichText::new(format_money(net)).italics()))
                            .on_hover_text("quantity × unit price");
                    }
                    None => {
                        ui.add_sized([100.0, 22.0], egui::TextEdit::singleline(&mut item.net_amount));
                    }
                }
                ui.add_sized([50.0, 22.0], egui::TextEdit::singleline(&mut item.vat_rate));
                ui.label(line.map(|l| format_money(l.vat)).unwrap_or_default());
                ui.label(line.map(|l| format_money(l.subtotal)).unwrap_or_default());
                if ui
                    .add_enabled(can_remove, egui::Button::new("✖").small())
                    .on_hover_text("Remove this line")
                    .clicked()
                {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
    if let Some(i) = remove {
        form.items.remove(i);
    }

    if let Some(inv) = computed {
        ui.add_space(6.0);
        egui::Grid::new("totals").num_columns(2).spacing([24.0, 2.0]).show(ui, |ui| {
            for rate in &inv.rates {
                let r = format_percent(rate.rate);
                ui.label(format!("Základ DPH {r}/Net"));
                ui.label(format_money(rate.net));
                ui.end_row();
                ui.label(format!("DPH {r}/VAT"));
                ui.label(format_money(rate.vat));
                ui.end_row();
            }
            ui.label(RichText::new("Suma na úhradu (EUR)").strong());
            ui.label(RichText::new(format_money(inv.total)).strong());
            ui.end_row();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn calendar_fills_in_the_date_field() {
        let mut harness = Harness::new_ui_state(
            |ui, value: &mut String| date_edit(ui, "date", value),
            "12.10.2026".to_string(),
        );
        harness.run();
        harness.get_by_label_contains("📆").click();
        harness.run();
        harness.get_by_label("20").click();
        harness.run();
        harness.get_by_label("Save").click();
        harness.run();
        assert_eq!(harness.state(), "20.10.2026");
    }

    #[test]
    fn calendar_opens_on_today_for_an_empty_field() {
        let mut harness = Harness::new_ui_state(
            |ui, value: &mut String| date_edit(ui, "date", value),
            String::new(),
        );
        harness.run();
        harness.get_by_label_contains("📆").click();
        harness.run();
        harness.get_by_label("Save").click();
        harness.run();
        assert_eq!(harness.state(), &format_date(today()));
    }

    #[test]
    fn templates_save_and_delete_but_default_stays() {
        let mut app = App {
            state: State::default(),
            form: Form::default(),
            dialog: None,
            status: None,
        };
        app.load_template(DEFAULT_TEMPLATE);
        assert_eq!(app.form.customer.name, "");

        app.form.customer.name = "ACME".into();
        app.state.save_template("ACME monthly", &app.form).unwrap();
        app.load_template(DEFAULT_TEMPLATE);
        assert_eq!(app.form.customer.name, "");
        app.load_template("ACME monthly");
        assert_eq!(app.form.customer.name, "ACME");
        assert_eq!(app.default_file_name(), "Invoice_ACME_monthly_");

        app.state.delete_template("ACME monthly");
        assert_eq!(app.state.selected, DEFAULT_TEMPLATE);
        assert_eq!(app.state.template_names(), [DEFAULT_TEMPLATE]);
    }
}
