CREATE SEQUENCE IF NOT EXISTS invoice_serial START 1;

ALTER TABLE ledger
    ADD COLUMN IF NOT EXISTS invoice_number  TEXT,
    ADD COLUMN IF NOT EXISTS invoice_pdf_key TEXT,
    ADD COLUMN IF NOT EXISTS lut_pdf_key     TEXT,
    ADD COLUMN IF NOT EXISTS backstop_note   TEXT;
