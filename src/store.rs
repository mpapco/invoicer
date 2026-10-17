//! What is remembered between runs: the saved templates, the form being
//! edited, the last invoice number issued and the folder invoices went to.
//!
//! Kept in `$XDG_CONFIG_HOME/invoicer/state.json`.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::model::Form;

/// The built-in empty template. It is not stored and cannot be deleted.
pub const DEFAULT_TEMPLATE: &str = "Default";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Template {
    pub name: String,
    pub form: Form,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// The user's templates; "Default" is not among them.
    pub templates: Vec<Template>,
    /// Name of the template the current form was loaded from.
    pub selected: String,
    /// The form as it was left when the app was closed.
    pub draft: Option<Form>,
    /// Shared by all templates: they draw from one numbering sequence.
    pub last_invoice_number: String,
    pub output_dir: Option<PathBuf>,
}

pub fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("invoicer").join("state.json"))
}

impl State {
    /// A missing or unreadable file just means starting with only "Default".
    pub fn load() -> State {
        let state: State = path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        state.with_valid_selection()
    }

    /// Fall back to "Default" when the selected template no longer exists.
    fn with_valid_selection(mut self) -> State {
        if self.selected.is_empty() || self.template(&self.selected).is_none() {
            self.selected = DEFAULT_TEMPLATE.into();
        }
        self
    }

    pub fn save(&self) -> Result<()> {
        let path = path().context("no configuration directory")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json).with_context(|| format!("writing {}", path.display()))
    }

    /// "Default" first, then the saved templates in alphabetical order.
    pub fn template_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.templates.iter().map(|t| t.name.clone()).collect();
        names.sort_by_key(|n| n.to_lowercase());
        names.insert(0, DEFAULT_TEMPLATE.into());
        names
    }

    pub fn template(&self, name: &str) -> Option<Form> {
        if is_default(name) {
            return Some(Form::default());
        }
        self.templates.iter().find(|t| t.name == name).map(|t| t.form.clone())
    }

    pub fn exists(&self, name: &str) -> bool {
        self.template(name.trim()).is_some()
    }

    /// Store `form` under `name`, replacing a template of that name.
    pub fn save_template(&mut self, name: &str, form: &Form) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Enter a name.".into());
        }
        if is_default(name) {
            return Err(format!("\"{DEFAULT_TEMPLATE}\" is the built-in empty template; choose another name."));
        }
        match self.templates.iter_mut().find(|t| t.name == name) {
            Some(t) => t.form = form.clone(),
            None => self.templates.push(Template { name: name.into(), form: form.clone() }),
        }
        self.selected = name.into();
        Ok(())
    }

    pub fn delete_template(&mut self, name: &str) {
        self.templates.retain(|t| t.name != name);
        if self.selected == name {
            self.selected = DEFAULT_TEMPLATE.into();
        }
    }
}

pub fn is_default(name: &str) -> bool {
    name.trim().eq_ignore_ascii_case(DEFAULT_TEMPLATE)
}

/// The number after `last`: its trailing digits incremented, width kept
/// (`2026016` → `2026017`, `FQ-009` → `FQ-010`).
pub fn next_number(last: &str) -> String {
    let last = last.trim();
    let digits = last.len() - last.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 {
        return last.to_string();
    }
    let (prefix, number) = last.split_at(last.len() - digits);
    match number.parse::<u128>() {
        Ok(n) => format!("{prefix}{:0width$}", n + 1, width = digits),
        Err(_) => last.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    #[test]
    fn increments_trailing_digits() {
        assert_eq!(next_number("2026016"), "2026017");
        assert_eq!(next_number("FQ-009"), "FQ-010");
        assert_eq!(next_number("99"), "100");
        assert_eq!(next_number("draft"), "draft");
        assert_eq!(next_number(""), "");
    }

    #[test]
    fn first_run_has_only_the_empty_default() {
        let state = State::default().with_valid_selection();
        assert_eq!(state.template_names(), [DEFAULT_TEMPLATE]);
        assert_eq!(state.selected, DEFAULT_TEMPLATE);
        assert_eq!(state.template(DEFAULT_TEMPLATE), Some(Form::default()));
    }

    #[test]
    fn deleted_templates_stay_deleted() {
        let mut state = State::default().with_valid_selection();
        state.save_template("Monthly", &fixtures::hourly()).unwrap();
        state.delete_template("Monthly");
        let json = serde_json::to_string(&state).unwrap();
        let state: State = serde_json::from_str::<State>(&json).unwrap().with_valid_selection();
        assert_eq!(state.template_names(), [DEFAULT_TEMPLATE]);
    }

    #[test]
    fn default_cannot_be_overwritten() {
        let mut state = State::default().with_valid_selection();
        assert!(state.save_template("default", &fixtures::hourly()).is_err());
        assert!(state.save_template("  ", &fixtures::hourly()).is_err());
        state.save_template("Monthly", &fixtures::hourly()).unwrap();
        assert_eq!(state.selected, "Monthly");
        assert_eq!(state.template("Monthly"), Some(fixtures::hourly()));
    }
}
