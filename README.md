# invoicer

Fills in an invoice in a form and writes it as a PDF laid out like the original spreadsheet
invoice templates: same columns, bilingual Slovak/English labels, font sizes, colours and borders.
No customer or supplier data is built in. Fill it in once and save it as a template.

```sh
cargo run --release
```

- **Templates**: pick one to start from. *Default* is the built-in empty template and cannot
  be deleted; on a fresh install it is the only one. *Save as template…* stores the
  current form under a new or existing name. *Delete template* removes the selected one, and
  *Revert* reloads it. Loading a template fills in today's dates using the template's rule (dated
  today, or on the last day of the previous month, due N days later). The invoice number
  continues from the last invoice generated, across all templates.
- **Empty means left out**: order number, delivery date, IČ DPH, header, footer and the
  agreement paragraph (when the agreement date is empty) are printed only when filled in.
- **Items**: *Number of lines* sets the size of the grid. All columns are always editable, and
  empty lines are skipped. Net amount is quantity × unit price when both are given, otherwise
  the typed amount is used. *Quantity / Unit / Unit price* are printed only if some line uses
  them. Totals are shown per VAT rate.
- **Dates**: type them as `d.m.yyyy` or pick them from the 📆 calendar.
- **QR code**: PayMe payment code made with the `payme-qr` crate: the total and the invoice
  number as VS. It carries no date.
- **Generate** asks for a file name and folder, then writes the PDF.

State lives in `~/.config/invoicer/state.json`. The PDF uses the bundled Noto Sans, which is also
what LibreOffice puts in place of the templates' Aptos Narrow. To use other fonts, put `Regular.ttf`
and `Bold.ttf` into `~/.config/invoicer/fonts/`.

`cargo run --example sample -- DIR` renders every saved template without the GUI.

## The QR library

`payme-qr` comes from the `qr-code-generator` repository on GitHub, pinned to the tag `0.1`
in `Cargo.toml` (the exact commit is recorded in `Cargo.lock`). Cargo fetches it into its
own cache, so a local checkout of that project is not needed and can be moved or deleted.
To use a newer version, push a new tag there and change `tag = "…"`. The repository is
reached over SSH, so `cargo build` needs your GitHub key in ssh-agent.
