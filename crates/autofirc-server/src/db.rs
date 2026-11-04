use anyhow::Context;
use sqlx::PgPool;

pub async fn connect(database_url: &str) -> anyhow::Result<PgPool> {
    let pool = PgPool::connect(database_url)
        .await
        .context("postgres connect")?;
    Ok(pool)
}

pub async fn migrate(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .context("run migrations")?;
    Ok(())
}

/// Insert a webhook row; returns false when the id already exists (idempotency).
pub async fn insert_webhook_event(
    pool: &PgPool,
    id: &str,
    capture_id: &str,
    order_id: &str,
) -> anyhow::Result<bool> {
    let rows = sqlx::query(
        "INSERT INTO ledger (id, capture_id, order_id) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
    )
    .bind(id)
    .bind(capture_id)
    .bind(order_id)
    .execute(pool)
    .await?;
    Ok(rows.rows_affected() == 1)
}

pub async fn set_enrichment(
    pool: &PgPool,
    id: &str,
    enrichment: &serde_json::Value,
    status: &str,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE ledger SET enrichment = $2, status = $3, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(enrichment)
        .bind(status)
        .execute(pool)
        .await?;
    Ok(())
}

/// Classification result to persist, bundled to keep call sites readable.
pub struct ClassificationUpdate<'a> {
    pub purpose_code: &'a str,
    pub mode_of_export: i16,
    pub sac_code: &'a str,
    pub confidence: i16,
    pub backstop_note: Option<&'a str>,
    pub status: &'a str,
}

/// Store AI classification results and transition the row.
pub async fn set_classification(
    pool: &PgPool,
    id: &str,
    u: &ClassificationUpdate<'_>,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE ledger SET purpose_code = $2, mode_of_export = $3, sac_code = $4, \
         ai_confidence = $5, backstop_note = $6, status = $7, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(u.purpose_code)
    .bind(u.mode_of_export)
    .bind(u.sac_code)
    .bind(u.confidence)
    .bind(u.backstop_note)
    .bind(u.status)
    .execute(pool)
    .await?;
    Ok(())
}

/// Allocate the next GST invoice serial (`INV/<fy>/<seq:04>`).
pub async fn next_invoice_number(pool: &PgPool, financial_year: &str) -> anyhow::Result<String> {
    let seq: i64 = sqlx::query_scalar("SELECT nextval('invoice_serial')")
        .fetch_one(pool)
        .await?;
    Ok(format!("INV/{financial_year}/{seq:04}"))
}

pub async fn set_documents(
    pool: &PgPool,
    id: &str,
    invoice_number: &str,
    invoice_pdf_key: &str,
    lut_pdf_key: &str,
    status: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE ledger SET invoice_number = $2, invoice_pdf_key = $3, lut_pdf_key = $4, \
         status = $5, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(invoice_number)
    .bind(invoice_pdf_key)
    .bind(lut_pdf_key)
    .bind(status)
    .execute(pool)
    .await?;
    Ok(())
}

/// Fetch one row as JSON (None when missing).
pub async fn get_row(pool: &PgPool, id: &str) -> anyhow::Result<Option<serde_json::Value>> {
    let row = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT to_jsonb(l) FROM ledger l WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn set_status(pool: &PgPool, id: &str, status: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE ledger SET status = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(status)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_ebrc(pool: &PgPool, id: &str, ebrc_number: &str) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE ledger SET ebrc_number = $2, status = 'fully_reconciled', updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(ebrc_number)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_by_status(pool: &PgPool, status: &str) -> anyhow::Result<Vec<serde_json::Value>> {
    let rows = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT to_jsonb(l) FROM ledger l WHERE status = $1 ORDER BY created_at",
    )
    .bind(status)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_ledger(pool: &PgPool) -> anyhow::Result<Vec<serde_json::Value>> {
    let rows = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT to_jsonb(l) FROM ledger l ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Requires DATABASE_URL_DIRECT in env; ignored by default so CI without
    /// credentials stays green.
    #[tokio::test]
    #[ignore = "needs live postgres"]
    async fn duplicate_webhook_ids_are_idempotent() {
        dotenvy::dotenv().ok();
        let url = std::env::var("DATABASE_URL_DIRECT").expect("DATABASE_URL_DIRECT");
        let pool = connect(&url).await.unwrap();
        migrate(&pool).await.unwrap();
        let id = format!("TEST-{}", uuid_v4());
        assert!(
            insert_webhook_event(&pool, &id, "CAP-T", "ORD-T")
                .await
                .unwrap()
        );
        assert!(
            !insert_webhook_event(&pool, &id, "CAP-T", "ORD-T")
                .await
                .unwrap()
        );
        sqlx::query("DELETE FROM ledger WHERE id = $1")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();
    }

    fn uuid_v4() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{nanos:x}")
    }
}
