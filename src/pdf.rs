//! Lays the invoice out like the spreadsheet templates and writes it as PDF.
//!
//! Geometry comes straight from the `.ods` files: A4, left/right margins of
//! 8.78 mm, top/bottom 19.05 mm, the same nine column widths, and each section
//! starting no higher than the row it occupies in the sheet. Layout produces a
//! list of drawing operations per page first, so that a long item list can
//! simply continue on the next page with the table header repeated.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use krilla::Document;
use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Rect};
use krilla::metadata::Metadata;
use krilla::page::PageSettings;
use krilla::paint::{Fill, Stroke};
use krilla::surface::Surface;
use krilla::text::{Font, TextDirection};
use payme_qr::qr::Matrix;

use crate::model::{
    Invoice, format_date, format_money, format_percent, format_quantity, format_rate,
};

const PAGE_W: f32 = 210.0;
const PAGE_H: f32 = 297.0;
const MARGIN_TOP: f32 = 19.05;
const MARGIN_BOTTOM: f32 = 19.05;

/// Left edges of columns A..I and the right edge of I, in mm.
const COL: [f32; 10] = [
    8.78, 27.88, 46.98, 66.08, 83.08, 112.18, 140.30, 157.74, 177.99, 198.24,
];
const A: usize = 0;
const C: usize = 2;
const D: usize = 3;
const E: usize = 4;
const F: usize = 5;
const G: usize = 6;
const H: usize = 7;
const I: usize = 8;
const END: usize = 9;

/// Top of the template rows each section starts at (sheet rows 1, 5, 15, 22, 32).
const ROW_TITLE: f32 = MARGIN_TOP;
const ROW_PARTIES: f32 = 41.44;
const ROW_DETAILS: f32 = 92.64;
const ROW_TABLE: f32 = 126.73;
const ROW_TOTALS: f32 = 181.60;

/// Row heights: the sheet's 4.87 mm rows are grown by LibreOffice to fit the
/// font, so these are the heights the current PDFs actually show.
const ROW: f32 = 4.9;
const ROW_12PT: f32 = 5.6;
const TABLE_HEADER_H: f32 = 11.04;
const QR_SIZE: f32 = 34.0;
/// The QR picture in the External template sits 12.11 mm into column F.
const QR_X: f32 = COL[F] + 12.11;
const CELL_PAD: f32 = 0.6;
const BORDER_PT: f32 = 0.74;

const BLACK: Color = Color(0x00, 0x00, 0x00);
const GREY: Color = Color(0x7f, 0x7f, 0x7f);
const HEADER_BLUE: Color = Color(0xa6, 0xca, 0xec);
const TOTAL_GREEN_BG: Color = Color(0xc6, 0xef, 0xce);
const TOTAL_GREEN_FG: Color = Color(0x00, 0x61, 0x00);

#[derive(Debug, Clone, Copy, PartialEq)]
struct Color(u8, u8, u8);

impl Color {
    fn fill(self) -> Fill {
        Fill {
            paint: rgb::Color::new(self.0, self.1, self.2).into(),
            ..Fill::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Weight {
    Regular,
    Bold,
}

#[derive(Debug, Clone, Copy)]
enum Align {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy)]
struct Style {
    size: f32,
    weight: Weight,
    color: Color,
}

const BODY: Style = Style { size: 11.0, weight: Weight::Regular, color: BLACK };
const BODY_BOLD: Style = Style { size: 11.0, weight: Weight::Bold, color: BLACK };
const TITLE: Style = Style { size: 18.0, weight: Weight::Regular, color: GREY };
const HEADING: Style = Style { size: 16.0, weight: Weight::Bold, color: GREY };
const PARTY_NAME: Style = Style { size: 12.0, weight: Weight::Bold, color: BLACK };
const NOTE: Style = Style { size: 12.0, weight: Weight::Regular, color: BLACK };
const TOTAL: Style = Style { size: 11.0, weight: Weight::Bold, color: TOTAL_GREEN_FG };
const HEADER_FOOTER: Style = Style { size: 9.0, weight: Weight::Regular, color: GREY };

/// One drawing operation, in mm with the origin at the top left of the page.
#[derive(Debug, Clone)]
enum Op {
    Text { x: f32, baseline: f32, style: Style, text: String },
    Rect { x: f32, y: f32, w: f32, h: f32, color: Color },
    Line { x1: f32, y1: f32, x2: f32, y2: f32 },
    Qr { x: f32, y: f32, size: f32, matrix: Matrix },
}

/// A font for drawing plus its parsed tables for measuring text width.
struct FontFace {
    font: Font,
    data: Arc<Vec<u8>>,
}

impl FontFace {
    fn load(data: Vec<u8>) -> Result<Self> {
        let data = Arc::new(data);
        ttf_parser::Face::parse(&data, 0).context("unreadable font file")?;
        let font = Font::new(data.clone().into(), 0).ok_or_else(|| anyhow!("unsupported font"))?;
        Ok(FontFace { font, data })
    }

    /// Advance width of `text` in mm.
    fn width(&self, text: &str, size_pt: f32) -> f32 {
        let face = ttf_parser::Face::parse(&self.data, 0).expect("checked on load");
        let units: u32 = text
            .chars()
            .map(|c| {
                face.glyph_index(c)
                    .and_then(|g| face.glyph_hor_advance(g))
                    .unwrap_or(0) as u32
            })
            .sum();
        units as f32 / face.units_per_em() as f32 * pt_to_mm(size_pt)
    }
}

struct Fonts {
    regular: FontFace,
    bold: FontFace,
}

impl Fonts {
    /// The bundled Noto Sans, which is also what LibreOffice substitutes for
    /// the templates' Aptos Narrow; `Regular.ttf` / `Bold.ttf` in the config
    /// font directory override it.
    fn load() -> Result<Self> {
        let pick = |file: &str, bundled: &[u8]| -> Result<FontFace> {
            match font_override_dir().map(|d| d.join(file)).filter(|p| p.exists()) {
                Some(path) => FontFace::load(
                    std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?,
                )
                .with_context(|| format!("loading {}", path.display())),
                None => FontFace::load(bundled.to_vec()),
            }
        };
        Ok(Fonts {
            regular: pick("Regular.ttf", include_bytes!("../assets/fonts/NotoSans-Regular.ttf"))?,
            bold: pick("Bold.ttf", include_bytes!("../assets/fonts/NotoSans-Bold.ttf"))?,
        })
    }

    fn face(&self, weight: Weight) -> &FontFace {
        match weight {
            Weight::Regular => &self.regular,
            Weight::Bold => &self.bold,
        }
    }

    fn width(&self, text: &str, style: Style) -> f32 {
        self.face(style.weight).width(text, style.size)
    }
}

pub fn font_override_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("invoicer").join("fonts"))
}

fn pt_to_mm(pt: f32) -> f32 {
    pt * 25.4 / 72.0
}

fn mm_to_pt(mm: f32) -> f32 {
    mm * 72.0 / 25.4
}

/// A table column: its span of sheet columns, the bilingual header, and how
/// its cells are aligned.
struct Column {
    from: usize,
    to: usize,
    header: [&'static str; 2],
    align: Align,
}

fn columns(show_quantity: bool) -> Vec<Column> {
    let col = |from, to, header, align| Column { from, to, header, align };
    let mut cols = vec![if show_quantity {
        col(A, C, ["Popis/", "Description"], Align::Center)
    } else {
        col(A, F, ["Popis/", "Description"], Align::Center)
    }];
    if show_quantity {
        cols.push(col(C, D, ["Počet/", "Quantity"], Align::Right));
        cols.push(col(D, E, ["M.j./", "Unit"], Align::Right));
        cols.push(col(E, F, ["Cena za m.j.", "/Unit price"], Align::Right));
    }
    cols.push(col(F, G, ["Cena bez DPH/", "Net amount"], Align::Right));
    cols.push(col(G, H, ["% DPH/", "% VAT"], Align::Right));
    cols.push(col(H, I, ["DPH/", "VAT"], Align::Right));
    cols.push(col(I, END, ["Suma/", "Subtotal"], Align::Right));
    cols
}

struct Layout<'a> {
    fonts: &'a Fonts,
    pages: Vec<Vec<Op>>,
    y: f32,
    header: Vec<String>,
    footer: Vec<String>,
}

impl<'a> Layout<'a> {
    fn new(fonts: &'a Fonts, header: Vec<String>, footer: Vec<String>) -> Self {
        let mut layout = Layout { fonts, pages: Vec::new(), y: MARGIN_TOP, header, footer };
        layout.new_page();
        layout
    }

    fn ops(&mut self) -> &mut Vec<Op> {
        self.pages.last_mut().unwrap()
    }

    fn new_page(&mut self) {
        self.pages.push(Vec::new());
        self.y = MARGIN_TOP;
        // Header lines sit in the top margin, ending just above the title row.
        let lines = self.header.clone();
        let first = MARGIN_TOP - 3.0 - (lines.len().saturating_sub(1)) as f32 * 4.0;
        for (i, line) in lines.iter().enumerate() {
            self.text_in(line, HEADER_FOOTER, COL[A], COL[END], Align::Center, first + i as f32 * 4.0);
        }
    }

    /// Bottom of the area content may use on the current page.
    fn limit(&self) -> f32 {
        PAGE_H - MARGIN_BOTTOM
    }

    /// Continue on a new page unless `height` mm still fit on this one.
    fn ensure(&mut self, height: f32) {
        if self.y + height > self.limit() {
            self.new_page();
        }
    }

    /// Move down to the template row `row_top`, but never back up.
    fn at_least(&mut self, row_top: f32) {
        if self.y < row_top {
            self.y = row_top;
        }
    }

    fn text(&mut self, text: &str, style: Style, x: f32, baseline: f32) {
        if !text.is_empty() {
            self.ops().push(Op::Text { x, baseline, style, text: text.to_string() });
        }
    }

    /// Text aligned inside the cell spanning `left`..`right`.
    fn text_in(&mut self, text: &str, style: Style, left: f32, right: f32, align: Align, baseline: f32) {
        let w = self.fonts.width(text, style);
        let x = match align {
            Align::Left => left + CELL_PAD,
            Align::Right => right - CELL_PAD - w,
            Align::Center => (left + right - w) / 2.0,
        };
        self.text(text, style, x, baseline);
    }

    /// Baseline for text in a row of `height` mm starting at `self.y`,
    /// bottom-aligned as in the sheet.
    fn baseline(&self, height: f32, style: Style) -> f32 {
        self.y + height - pt_to_mm(style.size) * 0.27 - 0.3
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        self.ops().push(Op::Rect { x, y, w, h, color });
    }

    fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32) {
        self.ops().push(Op::Line { x1, y1, x2, y2 });
    }

    /// Break `text` into lines no wider than `width` mm.
    fn wrap(&self, text: &str, style: Style, width: f32) -> Vec<String> {
        let mut lines = Vec::new();
        for paragraph in text.lines() {
            let mut line = String::new();
            for word in paragraph.split_whitespace() {
                let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
                if !line.is_empty() && self.fonts.width(&candidate, style) > width {
                    lines.push(std::mem::take(&mut line));
                    line = word.to_string();
                } else {
                    line = candidate;
                }
            }
            lines.push(line);
        }
        if lines.is_empty() {
            lines.push(String::new());
        }
        lines
    }

    fn finish(mut self) -> Vec<Vec<Op>> {
        let count = self.pages.len();
        let footer = std::mem::take(&mut self.footer);
        for page in 0..count {
            let mut lines = footer.clone();
            if count > 1 {
                lines.push(format!("{}/{}", page + 1, count));
            }
            let first = PAGE_H - MARGIN_BOTTOM + 6.0;
            for (i, line) in lines.iter().enumerate() {
                let style = HEADER_FOOTER;
                let w = self.fonts.width(line, style);
                self.pages[page].push(Op::Text {
                    x: (COL[A] + COL[END] - w) / 2.0,
                    baseline: first + i as f32 * 4.0,
                    style,
                    text: line.clone(),
                });
            }
        }
        self.pages
    }
}

fn non_empty_lines(text: &str) -> Vec<String> {
    let lines: Vec<String> = text.lines().map(|l| l.trim_end().to_string()).collect();
    let end = lines.iter().rposition(|l| !l.is_empty()).map_or(0, |i| i + 1);
    let start = lines.iter().position(|l| !l.is_empty()).unwrap_or(end);
    lines[start..end].to_vec()
}

fn layout(invoice: &Invoice, qr: Option<&Matrix>, fonts: &Fonts) -> Vec<Vec<Op>> {
    let form = &invoice.form;
    let mut l = Layout::new(
        fonts,
        non_empty_lines(&form.header_text),
        non_empty_lines(&form.footer_text),
    );

    // Row 1: the title in column F.
    l.y = ROW_TITLE;
    let b = l.baseline(7.78, TITLE);
    l.text(&format!("Faktúra/Invoice  {}", form.invoice_number.trim()), TITLE, COL[F], b);

    // Rows 5-12: customer in column A, supplier in column F.
    l.at_least(ROW_PARTIES);
    let b = l.baseline(6.95, HEADING);
    l.text("Zákazník/Customer", HEADING, COL[A], b);
    l.text("Dodávateľ/Supplier", HEADING, COL[F], b);
    l.y += 6.95;

    let party_lines = |p: &crate::model::Party| {
        let mut lines = vec![p.street.clone(), p.city.clone()];
        let labelled = [("IČO/IN: ", &p.ico), ("DIČ/VAT: ", &p.dic), ("IČ DPH: ", &p.ic_dph)];
        for (label, value) in labelled {
            if !value.trim().is_empty() {
                lines.push(format!("{label}{}", value.trim()));
            }
        }
        lines.retain(|s| !s.trim().is_empty());
        lines
    };
    let customer = party_lines(&form.customer);
    let mut supplier = party_lines(&form.supplier);
    if !form.iban.trim().is_empty() {
        supplier.push("Účet/Account Number:".into());
        supplier.push(payme_qr::iban::format_pretty(&payme_qr::iban::normalize(&form.iban)));
    }

    let b = l.baseline(ROW_12PT, PARTY_NAME);
    l.text(form.customer.name.trim(), PARTY_NAME, COL[A], b);
    l.text(form.supplier.name.trim(), PARTY_NAME, COL[F], b);
    l.y += ROW_12PT;
    let width = COL[F] - COL[A] - 2.0;
    let customer: Vec<String> = customer.iter().flat_map(|s| l.wrap(s, BODY, width)).collect();
    let supplier: Vec<String> = supplier.iter().flat_map(|s| l.wrap(s, BODY, COL[END] - COL[F])).collect();
    for i in 0..customer.len().max(supplier.len()) {
        let b = l.baseline(ROW, BODY);
        if let Some(s) = customer.get(i) {
            l.text(s, BODY, COL[A], b);
        }
        if let Some(s) = supplier.get(i) {
            l.text(s, BODY, COL[F], b);
        }
        l.y += ROW;
    }

    // Rows 15-18: order number and dates, labels in A and values in F.
    l.y += 2.0 * ROW;
    l.at_least(ROW_DETAILS);
    let mut details = Vec::new();
    if !form.order_number.trim().is_empty() {
        details.push(("Číslo objednávky/Order Number:", form.order_number.trim().to_string(), BODY_BOLD));
    }
    details.push(("Dátum vystavenia/Invoice date:", format_date(invoice.invoice_date), BODY));
    details.push(("Dátum splatnosti/Due date:", format_date(invoice.due_date), BODY));
    if let Some(d) = invoice.delivery_date {
        details.push(("Dátum dodania/Delivery date:", format_date(d), BODY));
    }
    for (label, value, style) in details {
        let b = l.baseline(ROW, BODY);
        l.text(label, BODY, COL[A], b);
        l.text(&value, style, COL[F], b);
        l.y += ROW;
    }

    // Row 22 on: the item table.
    l.y += 3.0 * ROW;
    l.at_least(ROW_TABLE);
    let cols = columns(invoice.show_quantity);
    let table_header = |l: &mut Layout| {
        let top = l.y;
        l.rect(COL[A], top, COL[END] - COL[A], TABLE_HEADER_H, HEADER_BLUE);
        for c in &cols {
            for (i, label) in c.header.iter().enumerate() {
                let baseline = top + 4.6 + i as f32 * 4.9;
                l.text_in(label, BODY, COL[c.from], COL[c.to], Align::Center, baseline);
            }
            let (x1, x2, y2) = (COL[c.from], COL[c.to], top + TABLE_HEADER_H);
            l.line(x1, top, x2, top);
            l.line(x1, y2, x2, y2);
            l.line(x1, top, x1, y2);
            l.line(x2, top, x2, y2);
        }
        l.y += TABLE_HEADER_H;
    };
    l.ensure(TABLE_HEADER_H + ROW);
    table_header(&mut l);

    for line in &invoice.lines {
        let desc_col = &cols[0];
        let desc = l.wrap(&line.description, BODY, COL[desc_col.to] - COL[desc_col.from] - 2.0 * CELL_PAD);
        let height = desc.len() as f32 * ROW;
        if l.y + height > l.limit() {
            l.new_page();
            table_header(&mut l);
        }
        for (i, text) in desc.iter().enumerate() {
            let b = l.baseline(ROW, BODY) + i as f32 * ROW;
            l.text_in(text, BODY, COL[desc_col.from], COL[desc_col.to], desc_col.align, b);
        }
        let mut values = Vec::new();
        if invoice.show_quantity {
            values.push(line.quantity.map(format_quantity).unwrap_or_default());
            values.push(line.unit.clone());
            values.push(line.unit_price.map(format_money).unwrap_or_default());
        }
        values.push(format_money(line.net));
        values.push(format_percent(line.vat_rate));
        values.push(format_money(line.vat));
        values.push(format_money(line.subtotal));
        // Numbers sit on the last line of a wrapped description, as a
        // bottom-aligned cell would put them.
        let b = l.baseline(height, BODY);
        for (c, value) in cols[1..].iter().zip(&values) {
            l.text_in(value, BODY, COL[c.from], COL[c.to], c.align, b);
        }
        l.y += height;
    }

    // Row 32 on: totals per VAT rate, then the amount due on green.
    l.y += 2.0 * ROW;
    l.at_least(ROW_TOTALS);
    let totals_h = (invoice.rates.len() * 2 + 1) as f32 * ROW;
    l.ensure(totals_h);
    for rate in &invoice.rates {
        let r = format_rate(rate.rate);
        for (label, value) in [
            (format!("Základ DPH {r}%/Net (EUR)"), rate.net),
            (format!("DPH {r}%/VAT {r}%"), rate.vat),
        ] {
            let b = l.baseline(ROW, BODY);
            l.text_in(&label, BODY, COL[E], COL[END], Align::Left, b);
            l.text_in(&format_money(value), BODY, COL[E], COL[END], Align::Right, b);
            l.y += ROW;
        }
    }
    let top = l.y;
    l.rect(COL[E], top, COL[END] - COL[E], ROW, TOTAL_GREEN_BG);
    l.line(COL[E], top, COL[END], top);
    let b = l.baseline(ROW, TOTAL);
    l.text_in("Suma na úhradu (EUR)", TOTAL, COL[E], COL[END], Align::Left, b);
    l.text_in(&format_money(invoice.total), TOTAL, COL[E], COL[END], Align::Right, b);
    l.y += ROW;

    // Row 37 on: the payment QR code.
    if let Some(matrix) = qr {
        l.y += 2.0 * ROW;
        l.ensure(QR_SIZE);
        let y = l.y;
        l.ops().push(Op::Qr { x: QR_X, y, size: QR_SIZE, matrix: matrix.clone() });
        l.y += QR_SIZE;
    }

    // Rows 41-45: the agreement paragraph of the FQ template, when an
    // agreement date is given.
    if !form.agreement_date.trim().is_empty() {
        l.y += if qr.is_some() { 2.0 * ROW } else { 6.0 * ROW };
        l.ensure(5.0 * ROW_12PT);
        let period = invoice.period();
        let period = period.as_str();
        let date = form.agreement_date.trim();
        let rows: [(String, Option<&str>); 5] = [
            ("Fakturujeme Vám služby na základe zmluvy".into(), None),
            (format!("zo dňa {date}  v znení neskorších dodatkov za obdobie:"), Some(period)),
            (String::new(), None),
            ("For my services I kindly request the following payment based on agreement".into(), None),
            (format!("from {date} for the period:"), Some(period)),
        ];
        for (text, value) in rows {
            let b = l.baseline(ROW_12PT, NOTE);
            l.text(&text, NOTE, COL[A], b);
            if let Some(value) = value {
                // Pushed right if the sentence before it runs long.
                let x = COL[G].max(COL[A] + l.fonts.width(&text, NOTE) + 3.0);
                l.text(value, BODY, x, b);
            }
            l.y += ROW_12PT;
        }
    }

    l.finish()
}

fn draw(surface: &mut Surface, op: &Op, fonts: &Fonts) {
    match op {
        Op::Text { x, baseline, style, text } => {
            surface.set_fill(Some(style.color.fill()));
            surface.set_stroke(None);
            surface.draw_text(
                Point::from_xy(mm_to_pt(*x), mm_to_pt(*baseline)),
                fonts.face(style.weight).font.clone(),
                style.size,
                text,
                false,
                TextDirection::Auto,
            );
        }
        Op::Rect { x, y, w, h, color } => {
            let mut pb = PathBuilder::new();
            if let Some(rect) = Rect::from_xywh(mm_to_pt(*x), mm_to_pt(*y), mm_to_pt(*w), mm_to_pt(*h)) {
                pb.push_rect(rect);
            }
            if let Some(path) = pb.finish() {
                surface.set_stroke(None);
                surface.set_fill(Some(color.fill()));
                surface.draw_path(&path);
            }
        }
        Op::Line { x1, y1, x2, y2 } => {
            let mut pb = PathBuilder::new();
            pb.move_to(mm_to_pt(*x1), mm_to_pt(*y1));
            pb.line_to(mm_to_pt(*x2), mm_to_pt(*y2));
            if let Some(path) = pb.finish() {
                surface.set_fill(None);
                surface.set_stroke(Some(Stroke { width: BORDER_PT, ..Stroke::default() }));
                surface.draw_path(&path);
            }
        }
        Op::Qr { x, y, size, matrix } => {
            let module = size / matrix.size as f32;
            let mut pb = PathBuilder::new();
            for row in 0..matrix.size {
                // One rectangle per horizontal run of dark modules.
                let mut col = 0;
                while col < matrix.size {
                    if !matrix.is_dark(col, row) {
                        col += 1;
                        continue;
                    }
                    let start = col;
                    while col < matrix.size && matrix.is_dark(col, row) {
                        col += 1;
                    }
                    if let Some(rect) = Rect::from_xywh(
                        mm_to_pt(x + start as f32 * module),
                        mm_to_pt(y + row as f32 * module),
                        mm_to_pt((col - start) as f32 * module),
                        mm_to_pt(module),
                    ) {
                        pb.push_rect(rect);
                    }
                }
            }
            if let Some(path) = pb.finish() {
                surface.set_stroke(None);
                surface.set_fill(Some(BLACK.fill()));
                surface.draw_path(&path);
            }
        }
    }
}

/// Render the invoice to PDF bytes.
pub fn render(invoice: &Invoice, qr: Option<&Matrix>) -> Result<Vec<u8>> {
    let fonts = Fonts::load()?;
    let pages = layout(invoice, qr, &fonts);

    let mut document = Document::new();
    document.set_metadata(
        Metadata::new()
            .title(format!("Faktúra/Invoice {}", invoice.form.invoice_number.trim()))
            .authors(vec![invoice.form.supplier.name.trim().to_string()])
            .creator("invoicer".to_string()),
    );
    for ops in &pages {
        let settings = PageSettings::from_wh(mm_to_pt(PAGE_W), mm_to_pt(PAGE_H))
            .ok_or_else(|| anyhow!("invalid page size"))?;
        let mut page = document.start_page_with(settings);
        let mut surface = page.surface();
        for op in ops {
            draw(&mut surface, op, &fonts);
        }
        surface.finish();
        page.finish();
    }
    document
        .finish()
        .map_err(|e| anyhow!("PDF could not be written: {e:?}"))
}

/// Render and write to `path`.
pub fn write(invoice: &Invoice, qr: Option<&Matrix>, path: &Path) -> Result<()> {
    let bytes = render(invoice, qr)?;
    std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    #[test]
    fn long_item_lists_continue_on_the_next_page() {
        let mut form = fixtures::hourly();
        form.footer_text = "Example Supplier s.r.o., Hlavná 1, 040 01 Košice".into();
        form.set_item_count(60);
        for (i, item) in form.items.iter_mut().enumerate() {
            item.description = format!("Item {i} with a rather long description that has to wrap");
            item.quantity = "2".into();
            item.unit_price = "10".into();
        }
        let invoice = Invoice::compute(&form).unwrap();
        let fonts = Fonts::load().unwrap();
        let pages = layout(&invoice, None, &fonts);
        assert!(pages.len() > 1);
        for ops in &pages {
            for op in ops {
                if let Op::Text { baseline, .. } = op {
                    assert!(*baseline < PAGE_H);
                }
            }
        }
        assert!(render(&invoice, None).unwrap().starts_with(b"%PDF"));
    }
}
