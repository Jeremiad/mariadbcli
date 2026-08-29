use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::de::DeserializeOwned;
use tokio::io::AsyncWriteExt;

use crate::api::models;

pub const DEFAULT_BASE_URL: &str = "https://downloads.mariadb.org/rest-api";

/// The part of an error body worth repeating.
///
/// The API explains some failures in a plain sentence, which is the most useful
/// thing an error can carry, and answers others with a full HTML page, which is
/// only noise in a terminal.
fn describe_body(body: &str) -> String {
    let body = body.trim();

    if body.is_empty() || body.starts_with('<') {
        return String::new();
    }

    let short: String = body.chars().take(200).collect();
    if short.len() < body.len() {
        format!(": {short}...")
    } else {
        format!(": {short}")
    }
}

/// Optional query filters accepted by the release-listing endpoints.
///
/// Valid `os` and `cpu` ids come from [`Client::operating_systems`] and
/// [`Client::architectures`]; mirror ids from [`Client::mirrors`].
#[derive(Debug, Default, Clone)]
pub struct Filters {
    pub mirror: Option<String>,
    pub os: Option<String>,
    pub cpu: Option<String>,
}

impl Filters {
    /// Whether any filter is actually set.
    pub fn is_set(&self) -> bool {
        self.mirror.is_some() || self.os.is_some() || self.cpu.is_some()
    }

    /// The filters as the flags that produced them, for error messages.
    pub fn describe(&self) -> String {
        self.as_query()
            .iter()
            .map(|(key, value)| format!("--{key} {value}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn as_query(&self) -> Vec<(&'static str, &str)> {
        let mut query = Vec::new();
        if let Some(mirror) = &self.mirror {
            query.push(("mirror", mirror.as_str()));
        }
        if let Some(os) = &self.os {
            query.push(("os", os.as_str()));
        }
        if let Some(cpu) = &self.cpu {
            query.push(("cpu", cpu.as_str()));
        }
        query
    }
}

/// Handle to the downloads API. Holds one `reqwest::Client` so that repeated
/// calls reuse the same connection pool.
pub struct Client {
    http: reqwest::Client,
    base_url: String,
}

impl Client {
    pub fn new() -> Result<Self> {
        Self::with_base_url(DEFAULT_BASE_URL)
    }

    pub fn with_base_url(base_url: impl Into<String>) -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .build()
                .context("failed to build HTTP client")?,
            base_url: base_url.into(),
        })
    }

    /// The underlying HTTP client, so callers reaching hosts outside the REST
    /// API (repository metadata, signing keys) reuse the same connection pool.
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// GET `path` relative to the base URL, returning the raw body.
    ///
    /// A non-success status carries the response body into the error, since the
    /// API reports unknown product, release and file ids that way.
    async fn get_text(&self, path: &str, query: &[(&str, &str)]) -> Result<String> {
        let url = format!("{}/{}", self.base_url, path);

        let resp = self
            .http
            .get(&url)
            .query(query)
            .send()
            .await
            .with_context(|| format!("request to {url} failed"))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .with_context(|| format!("could not read response body from {url}"))?;

        if !status.is_success() {
            bail!("{url} returned {status}{}", describe_body(&body));
        }

        Ok(body)
    }

    /// GET `path` and deserialize the body as `T`.
    async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, &str)]) -> Result<T> {
        let body = self.get_text(path, query).await?;
        serde_json::from_str(&body)
            .with_context(|| format!("unexpected response shape from {}/{}", self.base_url, path))
    }

    pub async fn products(&self) -> Result<models::ProductListRoot> {
        self.get("products/", &[]).await
    }

    pub async fn product_releases(&self, product_id: &str) -> Result<models::MajorReleasesRoot> {
        self.get(product_id, &[]).await
    }

    pub async fn point_releases(
        &self,
        product_id: &str,
        release_id: &str,
        filters: &Filters,
    ) -> Result<models::PointReleasesRoot> {
        self.get(&format!("{product_id}/{release_id}"), &filters.as_query())
            .await
    }

    pub async fn release_files(
        &self,
        product_id: &str,
        point_release_id: &str,
        filters: &Filters,
    ) -> Result<models::ReleaseFilesRoot> {
        self.get(
            &format!("{product_id}/{point_release_id}"),
            &filters.as_query(),
        )
        .await
    }

    /// Files of the newest point release within a major release.
    pub async fn latest_release(
        &self,
        product_id: &str,
        release_id: &str,
        filters: &Filters,
    ) -> Result<models::PointReleasesRoot> {
        self.get(
            &format!("{product_id}/{release_id}/latest/"),
            &filters.as_query(),
        )
        .await
    }

    pub async fn mirrors(&self) -> Result<models::MirrorListRoot> {
        self.get("mirrors", &[]).await
    }

    /// Where a mirror actually serves from.
    ///
    /// The API publishes no URL for a mirror, only an id, so the only way to
    /// find one is to ask for a file through that mirror and see where the
    /// download redirect lands. `file_id` must belong to a current release:
    /// mirrors do not carry archived ones, and asking for one returns 404.
    ///
    /// Returns `None` when the mirror cannot be resolved — it is unreachable,
    /// too slow, or no longer carries the file — since one bad mirror should
    /// not fail a listing of all of them.
    pub async fn resolve_mirror(&self, file_id: i64, mirror_id: &str) -> Option<String> {
        let url = format!("{}/{}", self.base_url, file_id);

        let resp = self
            .http
            .head(&url)
            .query(&[("mirror", mirror_id)])
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
            .ok()?;

        resp.status().is_success().then(|| resp.url().to_string())
    }

    pub async fn operating_systems(&self) -> Result<models::OsListRoot> {
        self.get("os", &[]).await
    }

    pub async fn architectures(&self) -> Result<models::ArchitectureListRoot> {
        self.get("cpu", &[]).await
    }

    pub async fn checksum(&self, file_id: i64) -> Result<models::ChecksumRoot> {
        self.get(&format!("{file_id}/checksum"), &[]).await
    }

    /// The detached PGP signature, returned as text rather than JSON.
    pub async fn signature(&self, file_id: i64) -> Result<String> {
        self.get_text(&format!("{file_id}/signature"), &[]).await
    }

    /// Stream a file into `dest_dir`, returning the path written.
    ///
    /// The endpoint redirects to a mirror; the name is taken from the final URL
    /// and reduced to a bare file name so a mirror cannot steer the write with
    /// a path of its own. `on_progress` receives (bytes so far, total if known).
    pub async fn download(
        &self,
        file_id: i64,
        dest_dir: &Path,
        mirror: Option<&str>,
        mut on_progress: impl FnMut(u64, Option<u64>),
    ) -> Result<PathBuf> {
        let url = format!("{}/{}", self.base_url, file_id);
        let query: Vec<(&str, &str)> = mirror.map(|m| vec![("mirror", m)]).unwrap_or_default();

        let mut resp = self
            .http
            .get(&url)
            .query(&query)
            .send()
            .await
            .with_context(|| format!("request to {url} failed"))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!("{url} returned {status}{}", describe_body(&body));
        }

        let file_name = resp
            .url()
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .map(|name| Path::new(name).file_name().unwrap_or_default())
            .filter(|name| !name.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(format!("file-{file_id}")));

        let total = resp.content_length();
        let dest = dest_dir.join(file_name);

        let mut file = tokio::fs::File::create(&dest)
            .await
            .with_context(|| format!("could not create {}", dest.display()))?;

        let mut written = 0u64;
        on_progress(written, total);

        while let Some(chunk) = resp
            .chunk()
            .await
            .with_context(|| format!("download of {url} was interrupted"))?
        {
            file.write_all(&chunk)
                .await
                .with_context(|| format!("could not write to {}", dest.display()))?;
            written += chunk.len() as u64;
            on_progress(written, total);
        }

        file.flush()
            .await
            .with_context(|| format!("could not flush {}", dest.display()))?;

        Ok(dest)
    }
}
