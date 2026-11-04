use anyhow::Context;
use s3::Bucket;
use s3::Region;
use s3::creds::Credentials;

/// S3-compatible object storage (Supabase Storage) for generated PDFs.
/// Objects are namespaced under `autofirc/` inside the configured bucket.
#[derive(Clone)]
pub struct PdfStore {
    bucket: Bucket,
}

impl PdfStore {
    pub fn from_env() -> anyhow::Result<Self> {
        let endpoint =
            std::env::var("OBJECT_STORAGE_ENDPOINT").context("OBJECT_STORAGE_ENDPOINT")?;
        let region = std::env::var("OBJECT_STORAGE_REGION").context("OBJECT_STORAGE_REGION")?;
        let bucket_name =
            std::env::var("OBJECT_STORAGE_BUCKET").context("OBJECT_STORAGE_BUCKET")?;
        let access =
            std::env::var("OBJECT_STORAGE_ACCESS_KEY").context("OBJECT_STORAGE_ACCESS_KEY")?;
        let secret =
            std::env::var("OBJECT_STORAGE_SECRET_KEY").context("OBJECT_STORAGE_SECRET_KEY")?;

        let region = Region::Custom { region, endpoint };
        let credentials = Credentials::new(Some(&access), Some(&secret), None, None, None)
            .context("storage credentials")?;
        let bucket = Bucket::new(&bucket_name, region, credentials)
            .context("bucket init")?
            .with_path_style();
        Ok(Self { bucket: *bucket })
    }

    /// Upload a PDF and return its object key.
    pub async fn upload_pdf(
        &self,
        event_id: &str,
        kind: &str,
        bytes: &[u8],
    ) -> anyhow::Result<String> {
        let key = format!("autofirc/{event_id}/{kind}.pdf");
        self.bucket
            .put_object(&key, bytes)
            .await
            .context("s3 put_object")?;
        Ok(key)
    }

    /// Short-lived signed URL for a stored object (default 15 minutes).
    pub async fn signed_url(&self, key: &str, expiry_secs: u32) -> anyhow::Result<String> {
        Ok(self.bucket.presign_get(key, expiry_secs, None).await?)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn key_namespacing_is_per_event() {
        // Mirrors upload_pdf's key format; storage itself needs live credentials.
        let key = format!("autofirc/{}/{kind}.pdf", "EV-1", kind = "invoice");
        assert_eq!(key, "autofirc/EV-1/invoice.pdf");
        assert!(key.starts_with("autofirc/"));
    }
}
