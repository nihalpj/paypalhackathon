//! autofirc-cli: machine-friendly control surface for the AutoFIRC server.
//!
//! Every command emits JSON on stdout so AI agents (Claude Code, Kimi,
//! OpenClaw, Hermes) can drive a merchant's cross-border compliance
//! programmatically. Human-readable output is behind `--pretty`.
use anyhow::{Context, bail};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "autofirc",
    about = "AutoFIRC compliance agent CLI — PayPal → FEMA/eBRC reconciliation",
    version
)]
struct Cli {
    /// Base URL of the autofirc-server API.
    #[arg(long, env = "AUTOFIRC_URL", default_value = "http://localhost:8080")]
    base_url: String,
    /// Human-readable tables instead of JSON.
    #[arg(long, global = true)]
    pretty: bool,
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List ledger rows, optionally filtered by pipeline status.
    List {
        /// Filter by status, e.g. compliance_packet_ready, fully_reconciled.
        #[arg(long)]
        status: Option<String>,
    },
    /// Show one ledger row plus its generated document URLs.
    Get {
        /// Ledger row id (PayPal webhook event id).
        event_id: String,
    },
    /// Natural-language ledger search ("unreconciled rows in the red zone").
    Query {
        /// The question to filter by.
        #[arg(required = true)]
        text: Vec<String>,
    },
    /// Authorize bank petitions for compliance-packet-ready rows.
    Petition {
        /// Ledger row ids to petition.
        #[arg(required = true)]
        event_ids: Vec<String>,
    },
    /// Fetch presigned download URLs for a row's invoice/LUT PDFs.
    Documents {
        /// Ledger row id.
        event_id: String,
    },
    /// Stream ledger changes as JSON lines (Ctrl-C to stop).
    Watch {
        /// Polling interval in seconds.
        #[arg(long, default_value_t = 5)]
        interval: u64,
    },
    /// Check connectivity and print the server status summary.
    Setup,
}

fn emit(value: &serde_json::Value, pretty: bool) -> anyhow::Result<()> {
    if pretty {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        println!("{}", serde_json::to_string(value)?);
    }
    Ok(())
}

fn pretty_table(rows: &[serde_json::Value]) {
    println!(
        "{:<28} {:<20} {:>12} {:<14} {:<8} {:<6} EBRC",
        "EVENT", "CLIENT", "AMOUNT INR", "STATUS", "PURPOSE", "CONF"
    );
    for r in rows {
        let s = |k: &str| r.get(k).and_then(|v| v.as_str()).unwrap_or("").to_owned();
        let paise = r
            .get("amount_inr_paise")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let conf = r
            .get("ai_confidence")
            .and_then(|v| v.as_i64())
            .map(|c| c.to_string())
            .unwrap_or_default();
        println!(
            "{:<28} {:<20} {:>12.2} {:<14} {:<8} {:<6} {}",
            truncate(&s("id"), 28),
            truncate(&s("client_name"), 20),
            paise as f64 / 100.0,
            s("status"),
            s("purpose_code"),
            conf,
            s("ebrc_number"),
        );
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_owned()
    } else {
        format!("{}…", &s[..max - 1])
    }
}

struct Api {
    http: reqwest::Client,
    base: String,
}

impl Api {
    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.http.get(format!("{}{path}", self.base))
    }
    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.http.post(format!("{}{path}", self.base))
    }
}

async fn fetch_ledger(api: &Api, status: Option<&str>) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut rows: Vec<serde_json::Value> = api
        .get("/api/ledger")
        .send()
        .await
        .context("GET /api/ledger")?
        .error_for_status()?
        .json()
        .await?;
    if let Some(want) = status {
        rows.retain(|r| r.get("status").and_then(|v| v.as_str()) == Some(want));
    }
    Ok(rows)
}

/// Client-side mirror of the server's FEMA alert computation, used to apply
/// the `alert` leg of an NL filter locally.
fn alert_level(row: &serde_json::Value, today: chrono::NaiveDate) -> &'static str {
    if row.get("status").and_then(|v| v.as_str()) == Some("fully_reconciled") {
        return "green";
    }
    let Some(date_str) = row.get("invoice_date").and_then(|v| v.as_str()) else {
        return "green";
    };
    let Ok(invoice) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") else {
        return "green";
    };
    let months = if row.get("inr_vostro").and_then(|v| v.as_bool()) == Some(true) {
        12
    } else {
        9
    };
    let Some(deadline) = invoice.checked_add_months(chrono::Months::new(months)) else {
        return "green";
    };
    let remaining = (deadline - today).num_days();
    if remaining <= 31 {
        "red"
    } else if remaining <= 62 {
        "amber"
    } else {
        "green"
    }
}

fn apply_filter(rows: &mut Vec<serde_json::Value>, filter: &serde_json::Value) {
    let today = chrono::Utc::now().date_naive();
    let g = |k: &str| filter.get(k).filter(|v| !v.is_null());
    let status = g("status").and_then(|v| v.as_str()).map(str::to_owned);
    let purpose = g("purpose_code")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let min_conf = g("min_confidence").and_then(|v| v.as_i64());
    let alert = g("alert").and_then(|v| v.as_str()).map(str::to_owned);
    let text = g("text").and_then(|v| v.as_str()).map(|s| s.to_lowercase());
    rows.retain(|r| {
        let rs = |k: &str| r.get(k).and_then(|v| v.as_str()).unwrap_or("");
        if let Some(want) = &status
            && rs("status") != want
        {
            return false;
        }
        if let Some(want) = &purpose
            && rs("purpose_code") != want
        {
            return false;
        }
        if let Some(min) = min_conf
            && r.get("ai_confidence").and_then(|v| v.as_i64()).unwrap_or(0) < min
        {
            return false;
        }
        if let Some(want) = &alert
            && alert_level(r, today) != want
        {
            return false;
        }
        if let Some(needle) = &text {
            let hay = [
                rs("client_name"),
                rs("invoice_number"),
                rs("ebrc_number"),
                rs("capture_id"),
                rs("order_id"),
                rs("id"),
            ]
            .join(" ")
            .to_lowercase();
            if !hay.contains(needle) {
                return false;
            }
        }
        true
    });
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let api = Api {
        http: reqwest::Client::new(),
        base: cli.base_url.trim_end_matches('/').to_owned(),
    };

    match cli.cmd {
        Command::List { status } => {
            let rows = fetch_ledger(&api, status.as_deref()).await?;
            if cli.pretty {
                pretty_table(&rows);
            } else {
                emit(&serde_json::json!({ "rows": rows }), false)?;
            }
        }
        Command::Get { event_id } => {
            let rows = fetch_ledger(&api, None).await?;
            let Some(row) = rows
                .into_iter()
                .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(event_id.as_str()))
            else {
                bail!("row {event_id} not found");
            };
            let docs: serde_json::Value = match api
                .get(&format!("/api/ledger/{event_id}/documents"))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => resp.json().await?,
                _ => {
                    serde_json::json!({ "note": "documents unavailable (object storage not configured)" })
                }
            };
            emit(
                &serde_json::json!({ "row": row, "documents": docs }),
                cli.pretty,
            )?;
        }
        Command::Query { text } => {
            let question = text.join(" ");
            let resp = api
                .post("/api/query")
                .json(&serde_json::json!({ "query": question }))
                .send()
                .await?;
            let filter: serde_json::Value = if resp.status().is_success() {
                resp.json().await?
            } else {
                eprintln!("note: NL parser unavailable, falling back to text search");
                serde_json::json!({ "text": question })
            };
            let mut rows = fetch_ledger(&api, None).await?;
            apply_filter(&mut rows, &filter);
            emit(
                &serde_json::json!({ "query": question, "filter": filter, "rows": rows }),
                cli.pretty,
            )?;
        }
        Command::Petition { event_ids } => {
            let out: serde_json::Value = api
                .post("/api/petition")
                .json(&serde_json::json!({ "event_ids": event_ids }))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            emit(&out, cli.pretty)?;
        }
        Command::Documents { event_id } => {
            let out: serde_json::Value = api
                .get(&format!("/api/ledger/{event_id}/documents"))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            emit(&out, cli.pretty)?;
        }
        Command::Watch { interval } => {
            let mut seen: std::collections::HashMap<String, String> = Default::default();
            loop {
                match fetch_ledger(&api, None).await {
                    Ok(rows) => {
                        for row in rows {
                            let id = row
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_owned();
                            let status = row
                                .get("status")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_owned();
                            if seen.get(&id) != Some(&status) {
                                seen.insert(id, status);
                                emit(&row, false)?;
                            }
                        }
                    }
                    Err(e) => eprintln!("watch: {e:#}"),
                }
                tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
            }
        }
        Command::Setup => {
            let rows = fetch_ledger(&api, None).await?;
            let mut by_status = serde_json::Map::new();
            for row in &rows {
                let status = row
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_owned();
                let entry = by_status
                    .entry(status)
                    .or_insert_with(|| serde_json::Value::from(0));
                *entry = serde_json::Value::from(entry.as_i64().unwrap_or(0) + 1);
            }
            emit(
                &serde_json::json!({
                    "server": api.base,
                    "reachable": true,
                    "rows": rows.len(),
                    "by_status": by_status,
                }),
                cli.pretty,
            )?;
        }
    }
    Ok(())
}
