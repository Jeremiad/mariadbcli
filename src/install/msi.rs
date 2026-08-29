//! Installing MariaDB on Windows from the published MSI package.
//!
//! Windows has no repository to configure, so the plan is a different shape:
//! pick the installer out of a release, fetch it through the downloads API,
//! check it against the checksum that API publishes, and hand it to `msiexec`.
//!
//! Selecting the installer and assembling the command line are pure functions,
//! so the interesting decisions are testable without Windows or a network.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};
use sha2::{Digest, Sha256};

use crate::api::{models, Client};

/// Properties passed to the installer. The names are MariaDB's documented MSI
/// properties; anything not covered by a field goes through `extra`.
#[derive(Debug, Clone)]
pub struct MsiOptions {
    pub service_name: String,
    pub port: u16,
    pub password: Option<String>,
    pub install_dir: Option<String>,
    pub data_dir: Option<String>,
    pub extra: Vec<(String, String)>,
}

impl Default for MsiOptions {
    fn default() -> Self {
        Self {
            // The MSI creates no service when SERVICENAME is empty, which is not
            // what installing a server is expected to mean.
            service_name: "MariaDB".to_string(),
            port: 3306,
            password: None,
            install_dir: None,
            data_dir: None,
            extra: Vec::new(),
        }
    }
}

impl MsiOptions {
    /// Parse a repeatable `KEY=VALUE` argument into an extra MSI property.
    pub fn parse_property(raw: &str) -> Result<(String, String)> {
        let (key, value) = raw
            .split_once('=')
            .ok_or_else(|| anyhow!("MSI property must be KEY=VALUE, got `{raw}`"))?;

        let key = key.trim();
        if key.is_empty() {
            bail!("MSI property has an empty name: `{raw}`");
        }

        Ok((key.to_ascii_uppercase(), value.to_string()))
    }

    fn properties(&self) -> Vec<(String, String)> {
        let mut properties = vec![
            ("SERVICENAME".to_string(), self.service_name.clone()),
            ("PORT".to_string(), self.port.to_string()),
        ];

        if let Some(password) = &self.password {
            properties.push(("PASSWORD".to_string(), password.clone()));
        }
        if let Some(install_dir) = &self.install_dir {
            properties.push(("INSTALLDIR".to_string(), install_dir.clone()));
        }
        if let Some(data_dir) = &self.data_dir {
            properties.push(("DATADIR".to_string(), data_dir.clone()));
        }

        properties.extend(self.extra.iter().cloned());
        properties
    }
}

/// Everything `install` would do on Windows, resolved before anything is run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MsiPlan {
    pub release_id: String,
    pub file_id: i64,
    pub file_name: String,
    pub download_dir: PathBuf,
    /// Published alongside the file by the downloads API. Absent only if the
    /// API stops publishing a sha256 for it, which is treated as a hard error
    /// at install time rather than silently skipping the check.
    pub expected_sha256: Option<String>,
    pub properties: Vec<(String, String)>,
}

impl MsiPlan {
    pub fn build(
        release: &models::Release,
        arch: &str,
        download_dir: PathBuf,
        options: &MsiOptions,
    ) -> Result<Self> {
        let installer = select_installer(release, arch)?;

        Ok(MsiPlan {
            release_id: release.release_id.clone(),
            file_id: installer.file_id,
            file_name: installer.file_name.clone(),
            download_dir,
            expected_sha256: installer
                .checksum
                .get("sha256sum")
                .cloned()
                .flatten()
                .map(|digest| digest.trim().to_ascii_lowercase()),
            properties: options.properties(),
        })
    }

    /// The `msiexec` invocation, given where the package was downloaded.
    ///
    /// `/qn` is silent, `/norestart` keeps the installer from rebooting a
    /// machine out from under whoever ran this.
    pub fn command(&self, msi_path: &Path) -> Vec<String> {
        let mut command = vec![
            "msiexec".to_string(),
            "/i".to_string(),
            msi_path.display().to_string(),
        ];

        for (key, value) in &self.properties {
            command.push(format!("{key}={value}"));
        }

        command.push("/qn".to_string());
        command.push("/norestart".to_string());
        command
    }

    /// Human-readable rendering, with any password redacted so a plan can be
    /// shown, logged or pasted without leaking it.
    pub fn describe(&self) -> String {
        let mut out = String::new();

        let _ = writeln!(out, "Target:          Windows ({})", super::current_arch());
        let _ = writeln!(out, "MariaDB version: {}", self.release_id);
        let _ = writeln!(
            out,
            "Package:         {} (file {})",
            self.file_name, self.file_id
        );

        match &self.expected_sha256 {
            Some(digest) => {
                let _ = writeln!(out, "\nWould verify sha256:\n  {digest}");
            }
            None => {
                let _ = writeln!(
                    out,
                    "\nThe downloads API publishes no sha256 for this file; install would stop."
                );
            }
        }

        let _ = writeln!(
            out,
            "\nWould download into:\n  {}",
            self.download_dir.display()
        );

        let _ = writeln!(out, "\nWould run:");
        let placeholder = self.download_dir.join(&self.file_name);
        let _ = writeln!(out, "  {}", redact(&self.command(&placeholder)).join(" "));

        out
    }

    /// Download the installer, check it against the published digest, and run it.
    pub async fn execute(
        &self,
        client: &Client,
        on_progress: impl FnMut(u64, Option<u64>),
    ) -> Result<()> {
        let Some(expected) = &self.expected_sha256 else {
            bail!(
                "the downloads API publishes no sha256 for {}, so the installer cannot be \
                 verified before it is run",
                self.file_name
            );
        };

        std::fs::create_dir_all(&self.download_dir)
            .with_context(|| format!("could not create {}", self.download_dir.display()))?;

        let path = client
            .download(self.file_id, &self.download_dir, None, on_progress)
            .await?;
        println!();

        let bytes =
            std::fs::read(&path).with_context(|| format!("could not read {}", path.display()))?;

        let actual = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();

        if &actual != expected {
            bail!(
                "{} failed its checksum: expected {expected}, got {actual}",
                path.display()
            );
        }
        println!("Verified sha256 of {}", path.display());

        let command = self.command(&path);
        println!("+ {}", redact(&command).join(" "));

        let status = Command::new(&command[0])
            .args(&command[1..])
            .status()
            .context("could not run msiexec")?;

        if !status.success() {
            bail!(
                "msiexec failed with {status}; run the installer manually from {} to see \
                 its own error",
                path.display()
            );
        }

        Ok(())
    }
}

/// Replace the value of any password property so a command can be printed.
fn redact(command: &[String]) -> Vec<String> {
    command
        .iter()
        .map(|argument| match argument.split_once('=') {
            Some((key, _)) if key.eq_ignore_ascii_case("PASSWORD") => format!("{key}=***"),
            _ => argument.clone(),
        })
        .collect()
}

/// Pick the Windows installer for `arch` out of a release's published files.
///
/// The same release also carries a zip and a debug-symbols zip, so matching on
/// the os and cpu alone is not enough.
pub fn select_installer<'a>(release: &'a models::Release, arch: &str) -> Result<&'a models::File> {
    let candidates: Vec<&models::File> = release
        .files
        .iter()
        .filter(|file| {
            file.os
                .as_deref()
                .is_some_and(|os| os.eq_ignore_ascii_case("windows"))
                && file
                    .cpu
                    .as_deref()
                    .is_some_and(|cpu| cpu.eq_ignore_ascii_case(arch))
                && file.file_name.to_ascii_lowercase().ends_with(".msi")
                && !file.file_name.to_ascii_lowercase().contains("debugsymbols")
        })
        .collect();

    match candidates.as_slice() {
        [installer] => Ok(installer),
        [] => bail!(
            "MariaDB {} publishes no Windows {arch} .msi installer; it has {}",
            release.release_id,
            summarise(release)
        ),
        // Never seen in practice, but picking arbitrarily from a map-ordered
        // list would make which installer runs depend on iteration order.
        many => bail!(
            "MariaDB {} publishes {} Windows {arch} installers ({}); cannot choose",
            release.release_id,
            many.len(),
            many.iter()
                .map(|file| file.file_name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn summarise(release: &models::Release) -> String {
    if release.files.is_empty() {
        return "no files at all".to_string();
    }

    release
        .files
        .iter()
        .map(|file| file.file_name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}
