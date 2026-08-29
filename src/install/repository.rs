//! Installing MariaDB from the Linux distribution package repositories.
//!
//! The work is split so that everything decidable without touching the system
//! is a pure function: [`Distro::from_os_release`] parses the detected release
//! file and [`Plan::build`] turns it into the exact files and commands that
//! would be applied. Only [`Plan::execute`] writes anything.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};
use sha2::{Digest, Sha256};

const DEB_BASE_URL: &str = "https://deb.mariadb.org";
const RPM_BASE_URL: &str = "https://rpm.mariadb.org";

/// apt needs a keyring file on disk. MariaDB publishes a **binary** keyring with
/// a companion digest, which is what `Signed-By` expects; the armored key below
/// is the wrong encoding for that purpose.
const KEYRING_URL: &str = "https://supplychain.mariadb.com/mariadb-keyring-2025.gpg";
const KEYRING_SHA256_URL: &str = "https://supplychain.mariadb.com/mariadb-keyring-2025.gpg.sha256";
const KEYRING_PATH: &str = "/etc/apt/keyrings/mariadb-keyring.gpg";

/// rpm-based package managers import an ASCII-armored key straight from the URL
/// named by `gpgkey`, so they use the armored form instead.
const ARMORED_KEY_URL: &str = "https://mariadb.org/mariadb_release_signing_key.pgp";

const APT_SOURCES_PATH: &str = "/etc/apt/sources.list.d/mariadb.sources";
const APT_LIST_PATH: &str = "/etc/apt/sources.list.d/mariadb.list";
const YUM_REPO_PATH: &str = "/etc/yum.repos.d/mariadb.repo";
const ZYPP_REPO_PATH: &str = "/etc/zypp/repos.d/mariadb.repo";

/// Mode for files apt must read as the unprivileged `_apt` user.
#[cfg(unix)]
const READABLE: u32 = 0o644;

/// Which apt source syntax to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AptFormat {
    /// `mariadb.sources`, the deb822 format apt has supported since 1.1.
    #[default]
    Deb822,
    /// `mariadb.list`, the one-line format, for tooling that parses it.
    OneLine,
}

/// Caller-supplied knobs for [`Plan::build`].
#[derive(Debug, Clone, Default)]
pub struct PlanOptions {
    pub apt_format: AptFormat,
    /// Replaces `deb.mariadb.org` / `rpm.mariadb.org` wholesale, for an internal
    /// mirror or caching proxy. The layout beneath it is unchanged, so the
    /// mirror must reproduce the upstream paths.
    ///
    /// This does **not** move the signing key: it still comes from MariaDB and
    /// is checksum-verified, so a mirror cannot substitute its own.
    pub repo_base_url: Option<String>,
}

/// The subset of `/etc/os-release` that decides which repository applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Distro {
    pub id: String,
    pub version_id: String,
    /// `UBUNTU_CODENAME` when present, else `VERSION_CODENAME`. Derivatives such
    /// as Linux Mint carry their own codename in `VERSION_CODENAME`, which has
    /// no matching suite upstream, but state the Ubuntu base separately.
    pub codename: Option<String>,
    pub id_like: Vec<String>,
    pub pretty_name: Option<String>,
}

impl Distro {
    pub fn from_os_release(contents: &str) -> Result<Self> {
        let mut fields: HashMap<&str, String> = HashMap::new();

        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim().trim_matches(['"', '\'']).to_string();
            fields.insert(key.trim(), value);
        }

        let id = fields
            .get("ID")
            .cloned()
            .ok_or_else(|| anyhow!("os-release has no ID field"))?;

        Ok(Distro {
            version_id: fields.get("VERSION_ID").cloned().unwrap_or_default(),
            codename: fields
                .get("UBUNTU_CODENAME")
                .or_else(|| fields.get("VERSION_CODENAME"))
                .cloned(),
            id_like: fields
                .get("ID_LIKE")
                .map(|value| value.split_whitespace().map(str::to_string).collect())
                .unwrap_or_default(),
            pretty_name: fields.get("PRETTY_NAME").cloned(),
            id,
        })
    }

    /// Read `/etc/os-release`, falling back to `/usr/lib/os-release`.
    pub fn detect() -> Result<Self> {
        for path in ["/etc/os-release", "/usr/lib/os-release"] {
            if let Ok(contents) = std::fs::read_to_string(path) {
                return Self::from_os_release(&contents)
                    .with_context(|| format!("could not parse {path}"));
            }
        }
        bail!(
            "no /etc/os-release found; `install` configures Linux package repositories \
             and is only supported on Linux"
        )
    }

    pub fn read_from(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("could not read {}", path.display()))?;
        Self::from_os_release(&contents)
            .with_context(|| format!("could not parse {}", path.display()))
    }

    fn is_like(&self, family: &str) -> bool {
        self.id == family || self.id_like.iter().any(|like| like == family)
    }

    pub fn display_name(&self) -> String {
        self.pretty_name
            .clone()
            .unwrap_or_else(|| format!("{} {}", self.id, self.version_id))
    }
}

/// The package manager driving the install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    Apt,
    Dnf,
    Yum,
    Zypper,
}

impl PackageManager {
    fn binary(self) -> &'static str {
        match self {
            PackageManager::Apt => "apt-get",
            PackageManager::Dnf => "dnf",
            PackageManager::Yum => "yum",
            PackageManager::Zypper => "zypper",
        }
    }

    /// MariaDB's own rpm repositories capitalise the package name; the deb
    /// repositories do not.
    fn server_package(self) -> &'static str {
        match self {
            PackageManager::Apt => "mariadb-server",
            _ => "MariaDB-server",
        }
    }

    fn commands(self) -> Vec<Vec<String>> {
        let owned = |args: &[&str]| args.iter().map(|arg| arg.to_string()).collect();
        match self {
            PackageManager::Apt => vec![
                owned(&["apt-get", "update"]),
                owned(&["apt-get", "install", "-y", self.server_package()]),
            ],
            PackageManager::Dnf => vec![
                owned(&["dnf", "makecache"]),
                owned(&["dnf", "install", "-y", self.server_package()]),
            ],
            PackageManager::Yum => vec![
                owned(&["yum", "makecache"]),
                owned(&["yum", "install", "-y", self.server_package()]),
            ],
            PackageManager::Zypper => vec![
                owned(&["zypper", "--gpg-auto-import-keys", "refresh"]),
                owned(&["zypper", "install", "-y", self.server_package()]),
            ],
        }
    }
}

/// A signing keyring to place on disk before the package manager runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keyring {
    pub path: PathBuf,
    pub url: String,
    /// Digest published alongside the keyring, checked before it is installed.
    pub sha256_url: String,
}

/// Everything `install` would do, resolved before anything is touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub distro: String,
    pub mariadb_version: String,
    pub manager: PackageManager,
    pub keyring: Option<Keyring>,
    /// Host the repository was built against, upstream unless overridden.
    pub repo_base_url: String,
    pub repo_file: PathBuf,
    pub repo_contents: String,
    /// The other apt source format, removed if present. apt reads every file in
    /// `sources.list.d`, so leaving both behind means the repository is
    /// configured twice and every update warns about it.
    pub remove_files: Vec<PathBuf>,
    /// Fetched before writing anything, to catch an unsupported distro release
    /// rather than leaving a broken repository behind.
    pub verify_url: String,
}

impl Plan {
    /// Resolve the repository layout for `distro`, MariaDB `version` and `arch`.
    ///
    /// `arch` is an rpm-style architecture such as `x86_64` or `aarch64`;
    /// `options.apt_format` is ignored for rpm-based distributions.
    pub fn build(
        distro: &Distro,
        version: &str,
        arch: &str,
        options: &PlanOptions,
    ) -> Result<Self> {
        let overridden = options
            .repo_base_url
            .as_deref()
            .map(normalize_base_url)
            .transpose()?;

        if distro.is_like("debian") || distro.is_like("ubuntu") {
            let base = overridden.unwrap_or_else(|| DEB_BASE_URL.to_string());
            Self::build_deb(distro, version, arch, options.apt_format, &base)
        } else if distro.is_like("suse") || distro.is_like("opensuse") {
            let base = overridden.unwrap_or_else(|| RPM_BASE_URL.to_string());
            Self::build_rpm(distro, version, arch, PackageManager::Zypper, &base)
        } else if distro.is_like("rhel") || distro.is_like("fedora") {
            let manager = if distro.major_version() == Some(7) {
                PackageManager::Yum
            } else {
                PackageManager::Dnf
            };
            let base = overridden.unwrap_or_else(|| RPM_BASE_URL.to_string());
            Self::build_rpm(distro, version, arch, manager, &base)
        } else {
            bail!(
                "unsupported distribution `{}`; MariaDB publishes .deb repositories for \
                 Debian and Ubuntu, and .rpm repositories for RHEL, CentOS, AlmaLinux, \
                 Rocky, Fedora, openSUSE and SLES",
                distro.id
            )
        }
    }

    fn build_deb(
        distro: &Distro,
        version: &str,
        arch: &str,
        apt_format: AptFormat,
        base_url: &str,
    ) -> Result<Self> {
        // MariaDB publishes under the base distribution, not the derivative.
        let flavour = if distro.is_like("ubuntu") {
            "ubuntu"
        } else {
            "debian"
        };

        let suite = distro.codename.as_deref().ok_or_else(|| {
            anyhow!(
                "os-release for `{}` has no VERSION_CODENAME or UBUNTU_CODENAME, \
                 so the apt suite cannot be determined",
                distro.id
            )
        })?;

        let uri = format!("{base_url}/{version}/{flavour}");
        let deb_arch = deb_arch(arch);

        let (repo_file, other_file, repo_contents) = match apt_format {
            AptFormat::Deb822 => (
                PathBuf::from(APT_SOURCES_PATH),
                PathBuf::from(APT_LIST_PATH),
                format!(
                    "# Written by mariadbcli\n\
                     Types: deb\n\
                     URIs: {uri}\n\
                     Suites: {suite}\n\
                     Components: main\n\
                     Architectures: {deb_arch}\n\
                     Signed-By: {KEYRING_PATH}\n"
                ),
            ),
            AptFormat::OneLine => (
                PathBuf::from(APT_LIST_PATH),
                PathBuf::from(APT_SOURCES_PATH),
                format!(
                    "# Written by mariadbcli\n\
                     deb [arch={deb_arch} signed-by={KEYRING_PATH}] {uri} {suite} main\n"
                ),
            ),
        };

        Ok(Plan {
            distro: distro.display_name(),
            mariadb_version: version.to_string(),
            manager: PackageManager::Apt,
            repo_base_url: base_url.to_string(),
            keyring: Some(Keyring {
                path: PathBuf::from(KEYRING_PATH),
                url: KEYRING_URL.to_string(),
                sha256_url: KEYRING_SHA256_URL.to_string(),
            }),
            repo_file,
            repo_contents,
            remove_files: vec![other_file],
            verify_url: format!("{uri}/dists/{suite}/Release"),
        })
    }

    fn build_rpm(
        distro: &Distro,
        version: &str,
        arch: &str,
        manager: PackageManager,
        base_url: &str,
    ) -> Result<Self> {
        let flavour = distro.rpm_flavour();
        let releasever = distro.rpm_releasever()?;

        let baseurl = format!("{base_url}/{version}/{flavour}/{releasever}/{arch}");
        let repo_contents = format!(
            "# Written by mariadbcli\n\
             [mariadb]\n\
             name = MariaDB {version}\n\
             baseurl = {baseurl}\n\
             gpgkey = {ARMORED_KEY_URL}\n\
             gpgcheck = 1\n\
             enabled = 1\n"
        );

        let repo_file = if manager == PackageManager::Zypper {
            PathBuf::from(ZYPP_REPO_PATH)
        } else {
            PathBuf::from(YUM_REPO_PATH)
        };

        Ok(Plan {
            distro: distro.display_name(),
            mariadb_version: version.to_string(),
            manager,
            repo_base_url: base_url.to_string(),
            keyring: None,
            repo_file,
            repo_contents,
            remove_files: Vec::new(),
            verify_url: format!("{baseurl}/repodata/repomd.xml"),
        })
    }

    pub fn commands(&self) -> Vec<Vec<String>> {
        self.manager.commands()
    }

    /// Human-readable rendering of every change, used for `--dry-run` and for
    /// the confirmation prompt.
    pub fn describe(&self) -> String {
        let mut out = String::new();

        let _ = writeln!(out, "Distribution:    {}", self.distro);
        let _ = writeln!(out, "MariaDB version: {}", self.mariadb_version);
        let _ = writeln!(out, "Package manager: {}", self.manager.binary());
        let _ = writeln!(out, "Repository base: {}", self.repo_base_url);
        let _ = writeln!(out, "\nWould verify:\n  {}", self.verify_url);

        if let Some(keyring) = &self.keyring {
            let _ = writeln!(
                out,
                "\nWould install the signing keyring:\n  {} -> {}\n  verified against {}",
                keyring.url,
                keyring.path.display(),
                keyring.sha256_url
            );
        }

        let _ = writeln!(out, "\nWould write {}:", self.repo_file.display());
        for line in self.repo_contents.lines() {
            let _ = writeln!(out, "  | {line}");
        }

        if !self.remove_files.is_empty() {
            let _ = writeln!(
                out,
                "\nWould remove, if present (apt would otherwise read the repository twice):"
            );
            for path in &self.remove_files {
                let _ = writeln!(out, "  {}", path.display());
            }
        }

        let _ = writeln!(out, "\nWould run:");
        for command in self.commands() {
            let _ = writeln!(out, "  {}", command.join(" "));
        }

        out
    }

    /// Read-only check that the resolved repository exists. Safe to run before
    /// the user has committed to anything, so `--dry-run` performs it too.
    pub async fn verify_repository(&self, http: &reqwest::Client) -> Result<()> {
        let resp = http
            .head(&self.verify_url)
            .send()
            .await
            .with_context(|| format!("could not reach {}", self.verify_url))?;

        if !resp.status().is_success() {
            bail!(
                "no MariaDB {} repository for this system: {} returned {}",
                self.mariadb_version,
                self.verify_url,
                resp.status()
            );
        }

        Ok(())
    }

    /// Apply the plan: place the keyring and repo file, drop the other apt
    /// source format, then hand over to the package manager.
    ///
    /// Callers verify first via [`Plan::verify_repository`].
    pub async fn execute(&self, http: &reqwest::Client) -> Result<()> {
        if let Some(keyring) = &self.keyring {
            self.install_keyring(http, keyring).await?;
        }

        for path in &self.remove_files {
            if path.exists() {
                std::fs::remove_file(path)
                    .with_context(|| format!("could not remove {}", path.display()))?;
                println!("Removed {}", path.display());
            }
        }

        write_readable(&self.repo_file, self.repo_contents.as_bytes())?;
        println!("Wrote {}", self.repo_file.display());

        for command in self.commands() {
            run(&command)?;
        }

        Ok(())
    }

    async fn install_keyring(&self, http: &reqwest::Client, keyring: &Keyring) -> Result<()> {
        let key = fetch(http, &keyring.url).await?;
        let digest_file = fetch(http, &keyring.sha256_url).await?;

        // The digest file is `sha256sum` output: "<hex>  <filename>".
        let expected = String::from_utf8(digest_file)
            .context("keyring digest is not valid UTF-8")?
            .split_whitespace()
            .next()
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| anyhow!("keyring digest at {} is empty", keyring.sha256_url))?;

        let actual = Sha256::digest(&key)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();

        if actual != expected {
            bail!(
                "signing keyring from {} failed its checksum: expected {expected}, got {actual}",
                keyring.url
            );
        }

        write_readable(&keyring.path, &key)?;
        println!("Wrote {} (sha256 verified)", keyring.path.display());
        Ok(())
    }
}

async fn fetch(http: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let resp = http
        .get(url)
        .send()
        .await
        .with_context(|| format!("could not fetch {url}"))?;

    let status = resp.status();
    if !status.is_success() {
        bail!("{url} returned {status}");
    }

    Ok(resp
        .bytes()
        .await
        .with_context(|| format!("could not read {url}"))?
        .to_vec())
}

impl Distro {
    /// Directory name used by <https://rpm.mariadb.org>, which differs from the
    /// os-release id for Rocky and the SUSE family.
    fn rpm_flavour(&self) -> &str {
        match self.id.as_str() {
            "rocky" => "rockylinux",
            "almalinux" | "centos" | "fedora" | "rhel" | "sles" => &self.id,
            id if id.starts_with("opensuse") => "opensuse",
            _ if self.is_like("fedora") => "fedora",
            _ => "rhel",
        }
    }

    /// RHEL-likes publish under the major version only; Fedora and the SUSE
    /// family publish under the full version.
    fn rpm_releasever(&self) -> Result<String> {
        if self.version_id.is_empty() {
            bail!("os-release for `{}` has no VERSION_ID", self.id);
        }

        let full_version = self.id == "fedora"
            || self.id == "sles"
            || self.id.starts_with("opensuse")
            || self.is_like("suse")
            || self.is_like("opensuse");

        if full_version {
            Ok(self.version_id.clone())
        } else {
            Ok(self
                .version_id
                .split('.')
                .next()
                .unwrap_or(&self.version_id)
                .to_string())
        }
    }

    fn major_version(&self) -> Option<u32> {
        self.version_id.split('.').next()?.parse().ok()
    }
}

/// Trim a caller-supplied repository base URL and reject what cannot be checked.
///
/// The plan is verified with an HTTP request before anything is written, so a
/// scheme that request cannot make would silently skip that safety net.
fn normalize_base_url(url: &str) -> Result<String> {
    let trimmed = url.trim().trim_end_matches('/');

    if trimmed.is_empty() {
        bail!("repository base URL is empty");
    }

    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        bail!(
            "repository base URL must be http or https, got `{url}`; the repository \
             is verified over HTTP before anything is written"
        );
    }

    Ok(trimmed.to_string())
}

/// Write a file apt or the rpm tooling must be able to read.
///
/// apt fetches as the unprivileged `_apt` user, so a root umask of 077 would
/// otherwise leave the keyring unreadable and `apt-get update` would fail.
fn write_readable(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }

    std::fs::write(path, contents)
        .with_context(|| format!("could not write {}", path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(READABLE))
            .with_context(|| format!("could not set permissions on {}", path.display()))?;
    }

    Ok(())
}

fn run(command: &[String]) -> Result<()> {
    println!("+ {}", command.join(" "));

    let status = Command::new(&command[0])
        .args(&command[1..])
        .status()
        .with_context(|| format!("could not run `{}`", command[0]))?;

    if !status.success() {
        bail!("`{}` failed with {status}", command.join(" "));
    }

    Ok(())
}

/// dpkg spells several architectures differently from rpm.
pub fn deb_arch(rpm_arch: &str) -> &str {
    match rpm_arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "ppc64le" => "ppc64el",
        "i686" | "x86" => "i386",
        other => other,
    }
}
