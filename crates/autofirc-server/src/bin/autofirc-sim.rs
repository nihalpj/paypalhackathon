//! autofirc-sim: demo traffic generator.
//!
//! Signs synthetic PAYMENT.CAPTURE.COMPLETED webhook events with the dev key
//! (DEV_WEBHOOK_KEY_PEM) and posts them to the server's webhook endpoint.
//! Paired with mock-paypal + mock-dgft this drives the entire pipeline —
//! ingestion, AI classification, GST documents, bank petition, DGFT
//! reconciliation — with zero external dependencies.
//!
//! Usage:
//!   autofirc-sim --keygen                 print a fresh PKCS#8 dev key PEM
//!   autofirc-sim [--count 5] [--base-url http://localhost:8080]
use anyhow::{Context, bail};
use autofirc_paypal::webhook::{self, TransmissionMeta};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey};
use rsa::signature::{RandomizedSigner, SignatureEncoding};
use sha2::Sha256;

struct Args {
    count: usize,
    base_url: String,
    keygen: bool,
}

fn parse_args() -> anyhow::Result<Args> {
    let mut args = Args {
        count: 5,
        base_url: std::env::var("AUTOFIRC_URL").unwrap_or_else(|_| "http://localhost:8080".into()),
        keygen: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--keygen" => args.keygen = true,
            "--count" => {
                args.count = it.next().context("--count needs a value")?.parse()?;
            }
            "--base-url" => {
                args.base_url = it.next().context("--base-url needs a value")?;
            }
            other => bail!("unknown argument {other}"),
        }
    }
    Ok(args)
}

fn load_private_key() -> anyhow::Result<rsa::RsaPrivateKey> {
    let v = std::env::var("DEV_WEBHOOK_KEY_PEM")
        .context("DEV_WEBHOOK_KEY_PEM not set (PEM string or path; generate with --keygen)")?;
    let pem = if v.contains("BEGIN") {
        v
    } else {
        std::fs::read_to_string(&v).context("read DEV_WEBHOOK_KEY_PEM file")?
    };
    Ok(rsa::RsaPrivateKey::from_pkcs8_pem(&pem)?)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let args = parse_args()?;

    if args.keygen {
        let mut rng = rand::thread_rng();
        let key = rsa::RsaPrivateKey::new(&mut rng, 2048)?;
        let pem = key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)?;
        print!("{}", pem.as_str());
        return Ok(());
    }

    let key = load_private_key()?;
    let webhook_id = std::env::var("PAYPAL_WEBHOOK_ID").unwrap_or_else(|_| "WH-DEMO".into());
    let http = reqwest::Client::new();
    let run = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        % 100_000;

    println!(
        "posting {} demo webhook event(s) to {args_url} ...",
        args.count,
        args_url = args.base_url
    );
    for i in 1..=args.count {
        let suffix = format!("demo-{run}-{i}");
        let event = serde_json::json!({
            "id": format!("EV-{suffix}"),
            "event_type": "PAYMENT.CAPTURE.COMPLETED",
            "resource": {
                "id": format!("CAP-{suffix}"),
                "supplementary_data": {"related_ids": {"order_id": format!("ORDER-{suffix}")}},
            },
        });
        let body = serde_json::to_vec(&event)?;
        let mut rng = rand::thread_rng();
        let meta = TransmissionMeta {
            transmission_id: format!("sim-{run}-{i}"),
            transmission_time: chrono::Utc::now().to_rfc3339(),
            cert_url: "https://api.paypal.com/dev-cert.pem".into(),
            transmission_sig: String::new(),
        };
        let msg = webhook::signed_message(&meta, &webhook_id, &body);
        let sig = SigningKey::<Sha256>::new(key.clone()).sign_with_rng(&mut rng, msg.as_bytes());
        let resp = http
            .post(format!("{}/api/webhooks/paypal", args.base_url))
            .header("paypal-transmission-id", &meta.transmission_id)
            .header("paypal-transmission-time", &meta.transmission_time)
            .header("paypal-cert-url", &meta.cert_url)
            .header("paypal-transmission-sig", B64.encode(sig.to_bytes()))
            .body(body)
            .send()
            .await?;
        println!(
            "  {} CAP-{suffix} → HTTP {}",
            event["id"].as_str().unwrap_or("?"),
            resp.status()
        );
        if !resp.status().is_success() {
            bail!("webhook rejected: HTTP {}", resp.status());
        }
    }
    println!("done. Watch the pipeline: autofirc watch");
    Ok(())
}
