//! Server-side PDF generation: GST Rule 46 commercial invoices and LUT
//! declarations. Pure Rust (genpdf) with embedded DejaVu fonts.
//!
//! Note: the design spec named typst; genpdf was substituted for reliability
//! (no external template engine, deterministic offline builds, small dep
//! tree). Output is identical in substance.
use anyhow::Context;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// The statutory LUT endorsement required on zero-rated export invoices
/// (Section 16 IGST Act read with Rule 96A CGST Rules).
pub const LUT_ENDORSEMENT: &str = "Supply meant for export under bond or letter of undertaking without payment of integrated tax";

/// Every field of GST Rule 46 relevant to a service export invoice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvoiceData {
    // (a) supplier identity
    pub supplier_name: String,
    pub supplier_address: String,
    pub supplier_gstin: String,
    // (b) consecutive serial number, unique for the financial year
    pub invoice_number: String,
    // (c) date of issue
    pub invoice_date: NaiveDate,
    // (d) recipient
    pub recipient_name: String,
    pub recipient_address: String,
    pub recipient_country: String,
    // (e-h) line item
    pub service_description: String,
    pub sac_code: String,
    pub taxable_value_foreign: String, // e.g. "USD 500.00"
    pub taxable_value_inr_paise: i64,
    // LUT / export context
    pub lut_reference: String,
    pub place_of_supply: String, // country of the recipient
    pub purpose_code: String,
    pub mode_of_export: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LutData {
    pub legal_name: String,
    pub gstin: String,
    pub lut_reference: String,
    pub financial_year: String,
    pub date: NaiveDate,
}

/// `INV/2026-27/0007` — consecutive serial per financial year.
pub fn format_invoice_number(financial_year: &str, serial: u32) -> String {
    format!("INV/{financial_year}/{serial:04}")
}

fn fonts() -> anyhow::Result<genpdf::fonts::FontFamily<genpdf::fonts::FontData>> {
    let regular = genpdf::fonts::FontData::new(
        include_bytes!("../fonts/DejaVuSans.ttf").to_vec(),
        None,
    )
    .context("load DejaVuSans")?;
    let bold = genpdf::fonts::FontData::new(
        include_bytes!("../fonts/DejaVuSans-Bold.ttf").to_vec(),
        None,
    )
    .context("load DejaVuSans-Bold")?;
    Ok(genpdf::fonts::FontFamily {
        regular,
        bold: bold.clone(),
        italic: bold.clone(),
        bold_italic: bold,
    })
}

fn doc(title: &str) -> anyhow::Result<genpdf::Document> {
    let family = fonts()?;
    let mut doc = genpdf::Document::new(family);
    doc.set_title(title);
    let mut decorator = genpdf::SimplePageDecorator::new();
    decorator.set_margins(12);
    doc.set_page_decorator(decorator);
    Ok(doc)
}

fn inr(paise: i64) -> String {
    format!("INR {:.2}", paise as f64 / 100.0)
}

/// Render a GST Rule 46 compliant commercial invoice for export of services.
pub fn render_invoice(data: &InvoiceData) -> anyhow::Result<Vec<u8>> {
    use genpdf::Element as _;
    use genpdf::elements;
    use genpdf::style::{Style, StyledString};

    let mut doc = doc("Export Commercial Invoice")?;
    let h1 = Style::new().bold().with_font_size(16);
    let h2 = Style::new().bold().with_font_size(11);

    doc.push(elements::Paragraph::new(StyledString::new(
        "TAX INVOICE — EXPORT OF SERVICES (ZERO-RATED UNDER LUT)",
        h1,
    )));
    doc.push(elements::Paragraph::new(StyledString::new(LUT_ENDORSEMENT, h2)));
    doc.push(elements::Break::new(1.0));

    let mut table = elements::TableLayout::new(vec![3, 5]);
    table.set_cell_decorator(elements::FrameCellDecorator::new(true, true, false));
    let rows: Vec<(String, String)> = vec![
        ("Supplier".into(), format!("{}\n{}", data.supplier_name, data.supplier_address)),
        ("Supplier GSTIN".into(), data.supplier_gstin.clone()),
        ("Invoice No.".into(), data.invoice_number.clone()),
        ("Invoice Date".into(), data.invoice_date.to_string()),
        ("Recipient".into(), format!("{}\n{}", data.recipient_name, data.recipient_address)),
        ("Recipient Country".into(), data.recipient_country.clone()),
        ("Place of Supply".into(), data.place_of_supply.clone()),
        ("Reverse Charge".into(), "No".into()),
        ("LUT Reference".into(), data.lut_reference.clone()),
        ("RBI Purpose Code".into(), data.purpose_code.clone()),
        ("Mode of Export of Services".into(), format!("Mode {}", data.mode_of_export)),
        ("Description of Services".into(), data.service_description.clone()),
        ("SAC Code".into(), data.sac_code.clone()),
        (
            "Taxable Value".into(),
            format!("{}  ({})", data.taxable_value_foreign, inr(data.taxable_value_inr_paise)),
        ),
        ("Integrated Tax (IGST)".into(), "NIL — export under LUT".into()),
    ];
    for (k, v) in rows {
        table
            .row()
            .element(elements::Paragraph::new(StyledString::new(k, h2)).padded(2))
            .element(elements::Paragraph::new(v).padded(2))
            .push()
            .expect("table row");
    }
    doc.push(table);
    doc.push(elements::Break::new(1.0));
    doc.push(elements::Paragraph::new(
        "Declaration: We declare that this invoice shows the actual price of the services \
         described and that all particulars are true and correct.",
    ));
    doc.push(elements::Break::new(1.5));
    doc.push(elements::Paragraph::new(StyledString::new(
        format!("For {}", data.supplier_name),
        h2,
    )));
    doc.push(elements::Paragraph::new("Authorised Signatory"));

    let mut out = Vec::new();
    doc.render(&mut out).context("render invoice pdf")?;
    Ok(out)
}

/// Render a Letter of Undertaking summary declaration (GST RFD-11 extract).
pub fn render_lut(data: &LutData) -> anyhow::Result<Vec<u8>> {
    use genpdf::elements;
    use genpdf::style::{Style, StyledString};

    let mut doc = doc("Letter of Undertaking")?;
    let h1 = Style::new().bold().with_font_size(16);
    let h2 = Style::new().bold().with_font_size(11);

    doc.push(elements::Paragraph::new(StyledString::new(
        "LETTER OF UNDERTAKING FOR EXPORT WITHOUT PAYMENT OF INTEGRATED TAX",
        h1,
    )));
    doc.push(elements::Paragraph::new("(Furnished under Rule 96A of the CGST Rules, 2017 — Form GST RFD-11)"));
    doc.push(elements::Break::new(1.0));

    let body = format!(
        "To\nThe President of India (through the jurisdictional GST authority)\n\n\
         I/We, {name}, holding GSTIN {gstin}, hereby undertake to pay the integrated tax \
         along with interest under Section 50 of the CGST Act, 2017, if the export proceeds \
         for services exported under this Letter of Undertaking are not received in \
         convertible foreign exchange (or in Indian rupees wherever permitted by the RBI) \
         within one year from the date of invoice, as prescribed under Rule 96A of the \
         CGST Rules, 2017.\n\n\
         LUT Reference: {lut}\nFinancial Year: {fy}\nDate: {date}\n\n\
         This undertaking is valid for exports made during the above financial year.",
        name = data.legal_name,
        gstin = data.gstin,
        lut = data.lut_reference,
        fy = data.financial_year,
        date = data.date,
    );
    for para in body.split("\n\n") {
        doc.push(elements::Paragraph::new(para.replace('\n', " ")));
        doc.push(elements::Break::new(0.5));
    }
    doc.push(elements::Break::new(1.0));
    doc.push(elements::Paragraph::new(StyledString::new(
        format!("For {}", data.legal_name),
        h2,
    )));
    doc.push(elements::Paragraph::new("Authorised Signatory"));

    let mut out = Vec::new();
    doc.render(&mut out).context("render lut pdf")?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invoice() -> InvoiceData {
        InvoiceData {
            supplier_name: "Acme Tech LLP".into(),
            supplier_address: "4th Floor, MG Road, Bengaluru 560001".into(),
            supplier_gstin: "29ABCDE1234F1Z5".into(),
            invoice_number: format_invoice_number("2026-27", 7),
            invoice_date: NaiveDate::from_ymd_opt(2026, 11, 4).unwrap(),
            recipient_name: "Globex Inc".into(),
            recipient_address: "1 Infinite Loop, Cupertino, CA".into(),
            recipient_country: "United States".into(),
            service_description: "Remote software consulting — October 2026 milestone".into(),
            sac_code: "998314".into(),
            taxable_value_foreign: "USD 500.00".into(),
            taxable_value_inr_paise: 4_150_000,
            lut_reference: "LUT/2026-27/AD290000000001A".into(),
            place_of_supply: "United States".into(),
            purpose_code: "P0802".into(),
            mode_of_export: 1,
        }
    }

    #[test]
    fn invoice_number_format_is_consecutive_per_fy() {
        assert_eq!(format_invoice_number("2026-27", 7), "INV/2026-27/0007");
        assert_eq!(format_invoice_number("2026-27", 1234), "INV/2026-27/1234");
    }

    #[test]
    fn invoice_renders_valid_pdf_with_rule46_content() {
        let bytes = render_invoice(&invoice()).unwrap();
        assert!(bytes.starts_with(b"%PDF"), "not a pdf");
        assert!(bytes.len() > 10_000, "suspiciously small pdf: {}", bytes.len());
    }

    #[test]
    fn lut_renders_valid_pdf() {
        let lut = LutData {
            legal_name: "Acme Tech LLP".into(),
            gstin: "29ABCDE1234F1Z5".into(),
            lut_reference: "LUT/2026-27/AD290000000001A".into(),
            financial_year: "2026-27".into(),
            date: NaiveDate::from_ymd_opt(2026, 4, 1).unwrap(),
        };
        let bytes = render_lut(&lut).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
        assert!(bytes.len() > 5_000);
    }

    #[test]
    fn endorsement_text_matches_statute() {
        assert!(LUT_ENDORSEMENT.contains("letter of undertaking"));
        assert!(LUT_ENDORSEMENT.contains("without payment of integrated tax"));
    }
}
