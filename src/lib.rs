//! Invoice generator: a form for the invoice data and a PDF laid out like the
//! original spreadsheet templates, optionally with a PayMe payment QR code.

pub mod app;
#[cfg(test)]
mod fixtures;
pub mod model;
pub mod pdf;
pub mod qr;
pub mod store;
