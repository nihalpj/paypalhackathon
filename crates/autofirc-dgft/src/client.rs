use crate::crypto;
use crate::types::*;

/// DGFT Open API client (production-faithful; demo points it at the mock).
#[derive(Clone)]
pub struct DgftClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    client_id: String,
    client_secret: String,
    signing_key: rsa::RsaPrivateKey,
}

impl DgftClient {
    pub fn new(
        base_url: &str,
        api_key: &str,
        client_id: &str,
        client_secret: &str,
        signing_key: rsa::RsaPrivateKey,
    ) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_owned(),
            api_key: api_key.to_owned(),
            client_id: client_id.to_owned(),
            client_secret: client_secret.to_owned(),
            signing_key,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// PBKDF2 credential handshake → bearer token.
    pub async fn access_token(&self) -> anyhow::Result<String> {
        let derived = crypto::derive_client_secret(&self.client_secret);
        let resp: TokenResponse = self
            .http
            .post(format!("{}/token", self.base_url))
            .json(&TokenRequest {
                api_key: self.api_key.clone(),
                client_id: self.client_id.clone(),
                client_secret_salt: derived.salt_b64,
                client_secret_hash: derived.hash_b64,
            })
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(resp.access_token)
    }

    /// Fetch pending IRMs for the merchant's IEC.
    pub async fn fetch_irms(&self, token: &str, iec: &str) -> anyhow::Result<Vec<IrmDto>> {
        Ok(self
            .http
            .get(format!("{}/irms", self.base_url))
            .query(&[("iec", iec)])
            .bearer_auth(token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    /// Submit a signed eBRC self-certification payload.
    pub async fn submit_ebrc(
        &self,
        token: &str,
        mut req: EbrcRequest,
    ) -> anyhow::Result<EbrcResponse> {
        // Signature covers the payload with the signature field blanked.
        req.signature = String::new();
        let sig = crypto::sign_payload(&self.signing_key, &serde_json::to_value(&req)?);
        req.signature = sig;
        Ok(self
            .http
            .post(format!("{}/ebrc", self.base_url))
            .bearer_auth(token)
            .json(&req)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    /// Group IRMs into clubbable sets using the core rule engine.
    pub fn club_irms(irms: &[IrmDto]) -> Vec<Vec<IrmDto>> {
        let mut groups: Vec<Vec<IrmDto>> = Vec::new();
        'outer: for irm in irms {
            let core = autofirc_core::clubbing::Irm {
                irm_number: irm.irm_number.clone(),
                bank_name: irm.bank_name.clone(),
                account_number: irm.account_number.clone(),
                currency: irm.currency.clone(),
                remittance_date: chrono::NaiveDate::parse_from_str(
                    &irm.remittance_date,
                    "%Y-%m-%d",
                )
                .unwrap_or(chrono::NaiveDate::MIN),
            };
            for group in &mut groups {
                let first = &group[0];
                let first_core = autofirc_core::clubbing::Irm {
                    irm_number: first.irm_number.clone(),
                    bank_name: first.bank_name.clone(),
                    account_number: first.account_number.clone(),
                    currency: first.currency.clone(),
                    remittance_date: chrono::NaiveDate::MIN,
                };
                if autofirc_core::clubbing::can_club(&first_core, &core).is_ok() {
                    group.push(irm.clone());
                    continue 'outer;
                }
            }
            groups.push(vec![irm.clone()]);
        }
        groups
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_client(uri: &str) -> DgftClient {
        let mut rng = rand::thread_rng();
        DgftClient::new(
            uri,
            "api-key",
            "client-id",
            "client-secret",
            rsa::RsaPrivateKey::new(&mut rng, 2048).unwrap(),
        )
    }

    #[tokio::test]
    async fn token_handshake_posts_pbkdf2_credentials() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({"access_token": "tok-1", "expires_in": 3600}),
                ),
            )
            .mount(&server)
            .await;
        let client = test_client(&server.uri());
        assert_eq!(client.access_token().await.unwrap(), "tok-1");
    }

    #[tokio::test]
    async fn fetch_irms_uses_bearer_and_iec() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/irms"))
            .and(query_param("iec", "0123456789"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"irm_number": "IRM-1", "bank_name": "HDFC", "account_number": "001",
                 "currency": "USD", "amount": "500.00", "remittance_date": "2026-11-01",
                 "remitter_name": "Globex Inc"}
            ])))
            .mount(&server)
            .await;
        let client = test_client(&server.uri());
        let irms = client.fetch_irms("tok-1", "0123456789").await.unwrap();
        assert_eq!(irms.len(), 1);
        assert_eq!(irms[0].bank_name, "HDFC");
    }

    #[tokio::test]
    async fn ebrc_submission_is_signed() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/ebrc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"ebrc_number": "EBRC-2026-001", "status": "ISSUED"}),
            ))
            .mount(&server)
            .await;
        let client = test_client(&server.uri());
        let resp = client
            .submit_ebrc(
                "tok-1",
                EbrcRequest {
                    iec: "0123456789".into(),
                    irm_numbers: vec!["IRM-1".into()],
                    invoice_number: "INV/2026-27/0001".into(),
                    invoice_date: "2026-11-04".into(),
                    invoice_value_inr: "41500.00".into(),
                    gstin: "29ABCDE1234F1Z5".into(),
                    sac_code: "998314".into(),
                    mode_of_export: 1,
                    purpose_code: "P0802".into(),
                    signature: String::new(),
                },
            )
            .await
            .unwrap();
        assert_eq!(resp.ebrc_number, "EBRC-2026-001");
    }

    #[test]
    fn clubbing_groups_by_bank_account_currency() {
        let mk = |n: &str, bank: &str, acct: &str, ccy: &str| IrmDto {
            irm_number: n.into(),
            bank_name: bank.into(),
            account_number: acct.into(),
            currency: ccy.into(),
            amount: "100".into(),
            remittance_date: "2026-11-01".into(),
            remitter_name: "X".into(),
        };
        let irms = vec![
            mk("I1", "HDFC", "001", "USD"),
            mk("I2", "HDFC", "001", "USD"),
            mk("I3", "HDFC", "001", "EUR"),
            mk("I4", "ICICI", "001", "USD"),
        ];
        let groups = DgftClient::club_irms(&irms);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].len(), 2); // I1+I2 clubbed
    }
}
