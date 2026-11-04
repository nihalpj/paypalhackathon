CREATE TABLE IF NOT EXISTS ledger (
    id               TEXT PRIMARY KEY,            -- paypal webhook event id
    capture_id       TEXT NOT NULL,
    order_id         TEXT NOT NULL,
    client_name      TEXT NOT NULL DEFAULT '',
    amount_inr_paise BIGINT NOT NULL DEFAULT 0,
    invoice_date     DATE NOT NULL DEFAULT CURRENT_DATE,
    inr_vostro       BOOLEAN NOT NULL DEFAULT FALSE,
    purpose_code     TEXT,
    mode_of_export   SMALLINT,
    sac_code         TEXT,
    ai_confidence    SMALLINT,
    ebrc_number      TEXT,
    status           TEXT NOT NULL DEFAULT 'pending_analysis',
    enrichment       JSONB,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
