use std::collections::HashMap;
use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::error::{ContextKind, ContextValue, ErrorKind};
use clap::{Args, Parser, Subcommand};
use prettytable::{row, Table};

use futures_util::{stream, StreamExt};

use crate::api::{models, requests::Filters, Client};
use crate::install::{self, AptFormat, Distro, MsiOptions, MsiPlan, Plan, PlanOptions};

const FILES_EXAMPLES: &str = "\
Examples:
  mariadbcli files mariadb 11.4.13                        every published file
  mariadbcli files mariadb 11.4.13 --os windows           Windows only
  mariadbcli files mariadb 11.4.13 --os source            the source tarball
  mariadbcli files mariadb 11.4.13 --os windows --cpu amd64

Run `mariadbcli os` and `mariadbcli cpu` for the accepted ids: the architecture
id is amd64, while its display name is x86_64.

For MariaDB Server only --os source and --os windows match. Linux packages are
published through the apt and rpm repositories rather than as individual files
here, so --os deb_package and --os rpm_package find nothing; use `install` for
those. A filter that matches nothing is reported as not found.
";

const DOWNLOAD_EXAMPLES: &str = "\
Examples:
  mariadbcli download 16992                       into the current directory
  mariadbcli download 16992 --output ./dist       into a chosen directory
  mariadbcli download 16992 --mirror alwyzon      from a specific mirror

The file is not verified after download. Compare it against `mariadbcli
checksum 16992` yourself.
";

const INSTALL_EXAMPLES: &str = "\
Examples:
  mariadbcli install --dry-run                    show the plan, change nothing
  sudo mariadbcli install --version 11.4          Linux, with a confirmation prompt
  sudo mariadbcli install --yes                   Linux, unattended
  mariadbcli install --port 3307 --yes            Windows, from an elevated prompt
  sudo mariadbcli install --repo-base-url https://mirror.example.com/mariadb
";

const TOP_EXAMPLES: &str = "\
Commands chain: `list` gives a product id, `releases` a major release,
`point-releases` a point release, and `files` a file id that `checksum`,
`signature` and `download` all take.

Examples:
  mariadbcli list                                 products and their ids
  mariadbcli releases mariadb                     major releases of a product
  mariadbcli latest mariadb 11.4 --os windows     newest point release, filtered
  mariadbcli checksum 16992                       published digests for a file
  mariadbcli download 16992 --output ./dist       fetch it
  mariadbcli install --dry-run                    plan an install, change nothing
";

#[derive(Parser)]
#[command(
    name = "mariadbcli",
    version,
    about = "Find, verify, download and install MariaDB releases",
    long_about = "Find, verify, download and install MariaDB releases.\n\n\
                  Everything shown comes from the MariaDB Downloads REST API, so the \
                  products, releases and files are always whatever MariaDB currently \
                  publishes. No database connection is involved.",
    after_help = TOP_EXAMPLES
)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// Filters accepted by the release-listing endpoints.
#[derive(Args)]
struct FilterArgs {
    /// Mirror id, as reported by `mirrors`
    #[arg(long)]
    mirror: Option<String>,
    /// Operating system id, as reported by `os`
    #[arg(long)]
    os: Option<String>,
    /// Architecture id, as reported by `cpu`
    #[arg(long)]
    cpu: Option<String>,
}

impl From<FilterArgs> for Filters {
    fn from(args: FilterArgs) -> Self {
        Filters {
            mirror: args.mirror,
            os: args.os,
            cpu: args.cpu,
        }
    }
}

#[derive(Subcommand)]
enum Command {
    /// List available products
    List,
    /// List the major releases of a product, e.g. `releases mariadb`
    Releases {
        /// Product id as reported by `list`
        product_id: String,
    },
    /// List the point releases within a major release
    PointReleases {
        /// Product id as reported by `list`
        product_id: String,
        /// Major release id as reported by `releases`
        release_id: String,
        #[command(flatten)]
        filters: FilterArgs,
    },
    /// Show the files published for a point release
    #[command(after_help = FILES_EXAMPLES)]
    Files {
        /// Product id as reported by `list`
        product_id: String,
        /// Point release id as reported by `point-releases`
        release_id: String,
        #[command(flatten)]
        filters: FilterArgs,
    },
    /// Show the files of the newest point release in a major release
    ///
    /// The same output as `files`, for whichever point release is newest, so a
    /// point release does not have to be looked up first.
    #[command(after_help = FILES_EXAMPLES)]
    Latest {
        /// Product id as reported by `list`
        product_id: String,
        /// Major release id as reported by `releases`
        release_id: String,
        #[command(flatten)]
        filters: FilterArgs,
    },
    /// List download mirrors, grouped by country
    Mirrors {
        /// Also show where each mirror serves from
        ///
        /// The API publishes no URL for a mirror, only an id, so each one is
        /// resolved by following its download redirect: one request per mirror,
        /// eight at a time, a few seconds in total. Mirrors that do not carry the
        /// probe file are shown as (unavailable).
        #[arg(long)]
        urls: bool,
    },
    /// List operating system ids accepted by --os
    Os,
    /// List architecture ids accepted by --cpu
    Cpu,
    /// Show the published checksums for a file
    Checksum {
        /// File id as reported by `files`
        file_id: i64,
    },
    /// Print the detached PGP signature for a file
    Signature {
        /// File id as reported by `files`
        file_id: i64,
    },
    /// Install MariaDB Server on this machine
    ///
    /// On Linux the distribution package repository is configured and the server
    /// installed with the system package manager. On Windows the published MSI is
    /// downloaded, checked against its sha256 and handed to msiexec.
    ///
    /// This is the only command that writes outside the working directory. It
    /// prints the whole plan first, asks before applying it, and needs root or
    /// Administrator. Start with --dry-run.
    #[command(after_help = INSTALL_EXAMPLES)]
    Install {
        /// Release to install (default: newest stable)
        ///
        /// A major release such as 11.4, or on Windows a point release such as
        /// 11.4.13. A major release is resolved to its newest point release.
        #[arg(long)]
        version: Option<String>,
        /// Print the plan and exit without touching the system
        #[arg(long)]
        dry_run: bool,
        /// Apply the plan without the confirmation prompt
        #[arg(short, long)]
        yes: bool,

        /// Write the one-line mariadb.list instead of deb822 mariadb.sources
        ///
        /// Debian and Ubuntu only, ignored elsewhere. Either way the other file is
        /// removed if present, so apt does not read the repository twice.
        #[arg(long, help_heading = "Linux options")]
        list_format: bool,
        /// Full base URL of a repository mirror
        ///
        /// Replaces https://deb.mariadb.org or https://rpm.mariadb.org wholesale.
        /// Everything below it is unchanged, so the mirror must reproduce the
        /// upstream layout: <URL>/<version>/<flavour> for apt, and
        /// <URL>/<version>/<flavour>/<release>/<arch> for rpm. Must be http or
        /// https, and does not move the signing key.
        #[arg(long, value_name = "URL", help_heading = "Linux options")]
        repo_base_url: Option<String>,
        /// Read distribution info from this file instead of /etc/os-release
        #[arg(long, value_name = "PATH", help_heading = "Linux options")]
        os_release: Option<PathBuf>,

        /// Name of the service to create (MSI SERVICENAME)
        ///
        /// The installer creates no service at all when this is empty.
        #[arg(long, default_value = "MariaDB", help_heading = "Windows options")]
        service_name: String,
        /// Port the server listens on (MSI PORT)
        #[arg(long, default_value_t = 3306, help_heading = "Windows options")]
        port: u16,
        /// Root password (MSI PASSWORD)
        ///
        /// Redacted from the printed plan, but visible to other processes while the
        /// installer runs. Prefer setting the password after installation if that
        /// matters to you.
        #[arg(long, help_heading = "Windows options")]
        password: Option<String>,
        /// Installation root (MSI INSTALLDIR)
        #[arg(long, value_name = "PATH", help_heading = "Windows options")]
        install_dir: Option<String>,
        /// Data directory (MSI DATADIR)
        #[arg(long, value_name = "PATH", help_heading = "Windows options")]
        data_dir: Option<String>,
        /// Any other MSI property, repeatable
        ///
        /// For example --msi-property UTF8=1 --msi-property STDCONFIG=1
        #[arg(long, value_name = "KEY=VALUE", help_heading = "Windows options")]
        msi_property: Vec<String>,
    },
    /// Download a file by id
    ///
    /// Streamed to disk rather than buffered, so a large file costs no memory.
    /// The name comes from the mirror the download is redirected to.
    #[command(after_help = DOWNLOAD_EXAMPLES)]
    Download {
        /// File id as reported by `files`
        file_id: i64,
        /// Directory to write into
        #[arg(short, long, default_value = ".")]
        output: PathBuf,
        /// Mirror id, as reported by `mirrors`
        #[arg(long)]
        mirror: Option<String>,
    },
}

/// Parse the command line and run, turning a product id typed as a command into
/// a pointer at the command that takes one.
///
/// `list` prints product ids, so reaching for `mariadbcli connector-odbc` next is
/// the obvious move; clap can only say the subcommand is unknown.
pub async fn run() -> Result<()> {
    let parsed = match Cli::try_parse() {
        Ok(cli) => return cli.run().await,
        Err(error) => error,
    };

    if parsed.kind() == ErrorKind::InvalidSubcommand {
        if let Some(token) = invalid_subcommand(&parsed) {
            // Best effort: an unreachable API just means the usual clap error.
            if let Ok(client) = Client::new() {
                if let Ok(products) = client.products().await {
                    if let Some(hint) = product_hint(&token, &products) {
                        eprint!("{hint}");
                        std::process::exit(2);
                    }
                }
            }
        }
    }

    parsed.exit()
}

/// The offending word from a clap `InvalidSubcommand` error.
fn invalid_subcommand(error: &clap::Error) -> Option<String> {
    error
        .context()
        .find_map(|(kind, value)| match (kind, value) {
            (ContextKind::InvalidSubcommand, ContextValue::String(token)) => Some(token.clone()),
            _ => None,
        })
}

/// Say what a filtered listing actually failed at.
///
/// The API answers a filter that matches nothing with the same 404 it uses for
/// an unknown release, so the raw error blames the release rather than the
/// filter that is really at fault.
fn explain_filtered<T>(result: Result<T>, filters: &Filters) -> Result<T> {
    match result {
        Err(error) if filters.is_set() => Err(error.context(format!(
            "nothing matched `{}`; for MariaDB Server only --os source and --os windows \
             match, because Linux packages are published through the apt and rpm \
             repositories rather than as individual files (see `mariadbcli install`)",
            filters.describe()
        ))),
        other => other,
    }
}

/// The message to print when the unknown subcommand is really a product id.
fn product_hint(token: &str, products: &models::ProductListRoot) -> Option<String> {
    let product = products
        .products_list
        .iter()
        .find(|product| product.product_id.eq_ignore_ascii_case(token))?;

    Some(format!(
        concat!(
            "error: unrecognized subcommand '{token}'\n",
            "\n",
            "'{token}' is a product id ({name}), not a command.\n",
            "\n",
            "To see its releases:\n",
            "    mariadbcli releases {token}\n",
            "\n",
            "For all commands, try '--help'.\n",
        ),
        token = token,
        name = product.name
    ))
}

impl Cli {
    pub async fn run(self) -> Result<()> {
        let client = Client::new()?;

        match self.command {
            Command::List => {
                print_products(&client.products().await?);
            }
            Command::Releases { product_id } => {
                print_releases(&client.product_releases(&product_id).await?);
            }
            Command::PointReleases {
                product_id,
                release_id,
                filters,
            } => {
                let filters: Filters = filters.into();
                let releases = explain_filtered(
                    client
                        .point_releases(&product_id, &release_id, &filters)
                        .await,
                    &filters,
                )?;
                print_point_releases(&releases);
            }
            Command::Files {
                product_id,
                release_id,
                filters,
            } => {
                let filters: Filters = filters.into();
                let files = explain_filtered(
                    client
                        .release_files(&product_id, &release_id, &filters)
                        .await,
                    &filters,
                )?;
                print_release_files(&files);
            }
            Command::Latest {
                product_id,
                release_id,
                filters,
            } => {
                let filters: Filters = filters.into();
                let latest = explain_filtered(
                    client
                        .latest_release(&product_id, &release_id, &filters)
                        .await,
                    &filters,
                )?;
                print_latest(&latest);
            }
            Command::Mirrors { urls } => {
                let mirrors = client.mirrors().await?;

                if urls {
                    print_mirrors_with_urls(&client, &mirrors).await?;
                } else {
                    print_mirrors(&mirrors);
                }
            }
            Command::Os => {
                print_operating_systems(&client.operating_systems().await?);
            }
            Command::Cpu => {
                print_architectures(&client.architectures().await?);
            }
            Command::Checksum { file_id } => {
                print_checksum(&client.checksum(file_id).await?);
            }
            Command::Signature { file_id } => {
                print!("{}", client.signature(file_id).await?);
            }
            Command::Install {
                version,
                dry_run,
                yes,
                list_format,
                service_name,
                port,
                password,
                install_dir,
                data_dir,
                msi_property,
                repo_base_url,
                os_release,
            } => {
                // The requested release, or the newest stable one.
                let version = match version {
                    Some(version) => version,
                    None => {
                        let releases = client.product_releases("mariadb").await?;
                        install::latest_stable(&releases)
                            .context("the API reported no stable MariaDB release")?
                            .to_string()
                    }
                };

                if cfg!(target_os = "windows") {
                    // Windows has no repository: the plan names one installer,
                    // so a major release has to be resolved to its newest point
                    // release first.
                    let release = if install::is_point_release(&version) {
                        client
                            .release_files("mariadb", &version, &Default::default())
                            .await?
                            .release_data
                    } else {
                        client
                            .latest_release("mariadb", &version, &Default::default())
                            .await?
                            .releases
                    };

                    let release = release.into_values().next().with_context(|| {
                        format!("the API reported no files for MariaDB {version}")
                    })?;

                    let mut extra = Vec::new();
                    for property in &msi_property {
                        extra.push(MsiOptions::parse_property(property)?);
                    }

                    let plan = MsiPlan::build(
                        &release,
                        install::current_arch(),
                        std::env::temp_dir().join("mariadbcli"),
                        &MsiOptions {
                            service_name,
                            port,
                            password,
                            install_dir,
                            data_dir,
                            extra,
                        },
                    )?;

                    print!("{}", plan.describe());

                    if dry_run {
                        return Ok(());
                    }

                    install::require_privileges()?;

                    if !yes && !confirm("\nApply this plan?")? {
                        println!("Aborted; nothing was changed.");
                        return Ok(());
                    }

                    println!();
                    let mut last_percent = u64::MAX;
                    plan.execute(&client, |written, total| {
                        report_progress(written, total, &mut last_percent)
                    })
                    .await?;
                    println!("\nMariaDB {} installed.", plan.release_id);

                    return Ok(());
                }

                let distro = match &os_release {
                    Some(path) => Distro::read_from(path)?,
                    None => Distro::detect()?,
                };

                let options = PlanOptions {
                    apt_format: if list_format {
                        AptFormat::OneLine
                    } else {
                        AptFormat::Deb822
                    },
                    repo_base_url,
                };

                let plan = Plan::build(&distro, &version, install::current_arch(), &options)?;
                print!("{}", plan.describe());

                // Read-only, so it runs on --dry-run too: an unsupported distro
                // release should surface here, not after a root install begins.
                plan.verify_repository(client.http()).await?;
                println!("\nRepository verified.");

                if dry_run {
                    return Ok(());
                }

                install::require_privileges()?;

                if !yes && !confirm("\nApply this plan?")? {
                    println!("Aborted; nothing was changed.");
                    return Ok(());
                }

                println!();
                plan.execute(client.http()).await?;
                println!("\nMariaDB {version} installed.");
            }
            Command::Download {
                file_id,
                output,
                mirror,
            } => {
                let mut last_percent = u64::MAX;
                let dest = client
                    .download(file_id, &output, mirror.as_deref(), |written, total| {
                        report_progress(written, total, &mut last_percent)
                    })
                    .await?;
                println!("\nSaved to {}", dest.display());
            }
        }

        Ok(())
    }
}

fn confirm(prompt: &str) -> Result<bool> {
    print!("{prompt} [y/N] ");
    io::stdout().flush()?;

    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .context("could not read confirmation")?;

    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// Overwrite a single progress line, only when the rounded percentage moves.
fn report_progress(written: u64, total: Option<u64>, last_percent: &mut u64) {
    match total {
        Some(total) if total > 0 => {
            let percent = written * 100 / total;
            if percent != *last_percent {
                *last_percent = percent;
                print!("\rDownloading... {percent}% ({written}/{total} bytes)");
            }
        }
        _ => print!("\rDownloading... {written} bytes"),
    }
}

fn print_products(products: &models::ProductListRoot) {
    let mut table = Table::new();
    table.add_row(row!["Product id", "Product name", "License"]);

    for product in &products.products_list {
        table.add_row(row![
            product.product_id,
            product.name,
            product.license.as_deref().unwrap_or_default()
        ]);
    }

    println!("Products:");
    table.printstd();
}

fn print_releases(releases: &models::MajorReleasesRoot) {
    let mut table = Table::new();
    table.add_row(row!["Id", "Name", "Status"]);

    for release in &releases.major_releases {
        table.add_row(row![
            release.release_id,
            release.release_name,
            release.release_status
        ]);
    }

    println!("Product releases:");
    table.printstd();
}

/// The API returns release maps keyed by version, so sort for stable output.
fn sorted_releases(releases: &HashMap<String, models::Release>) -> Vec<&models::Release> {
    let mut releases: Vec<_> = releases.values().collect();
    releases.sort_by(|a, b| a.release_id.cmp(&b.release_id));
    releases
}

fn print_point_releases(point_releases: &models::PointReleasesRoot) {
    let mut table = Table::new();
    table.add_row(row!["Id", "Name", "Release date", "Release notes", "Files"]);

    for release in sorted_releases(&point_releases.releases) {
        table.add_row(row![
            release.release_id,
            release.release_name,
            release.date_of_release,
            release.release_notes_url,
            release.files.len()
        ]);
    }

    println!("Point releases:");
    table.printstd();
}

fn print_latest(latest: &models::PointReleasesRoot) {
    for release in sorted_releases(&latest.releases) {
        print_release(release);
    }
}

fn print_release_files(release_files: &models::ReleaseFilesRoot) {
    for release in sorted_releases(&release_files.release_data) {
        print_release(release);
    }
}

fn print_release(release: &models::Release) {
    let mut summary = Table::new();
    summary.add_row(row!["Id", "Name", "Release date", "Release notes"]);
    summary.add_row(row![
        release.release_id,
        release.release_name,
        release.date_of_release,
        release.release_notes_url
    ]);
    summary.printstd();

    let mut files = Table::new();
    files.add_row(row!["Id", "Name", "Type", "OS", "CPU", "Url"]);

    for file in &release.files {
        files.add_row(row![
            file.file_id,
            file.file_name,
            file.package_type.as_deref().unwrap_or_default(),
            file.os.as_deref().unwrap_or_default(),
            file.cpu.as_deref().unwrap_or_default(),
            file.file_download_url
        ]);
    }

    println!("\nFiles:");
    files.printstd();
}

fn print_mirrors(mirrors: &models::MirrorListRoot) {
    let mut countries: Vec<_> = mirrors.mirror_list.iter().collect();
    countries.sort_by_key(|(country, _)| *country);

    let mut table = Table::new();
    table.add_row(row!["Country", "Mirror id", "Mirror name"]);

    for (country, mirrors) in countries {
        for mirror in mirrors {
            table.add_row(row![country, mirror.mirror_id, mirror.mirror_name]);
        }
    }

    println!("Mirrors:");
    table.printstd();
}

/// Resolve every mirror concurrently and print where each serves from.
///
/// A probe file from the newest stable release is needed because mirrors carry
/// only current releases; asking for an archived file returns 404 everywhere.
async fn print_mirrors_with_urls(client: &Client, mirrors: &models::MirrorListRoot) -> Result<()> {
    let releases = client.product_releases("mariadb").await?;
    let newest = install::latest_stable(&releases)
        .context("the API reported no stable MariaDB release to probe mirrors with")?;

    let probe = client
        .latest_release("mariadb", newest, &Filters::default())
        .await?
        .releases
        .into_values()
        .next()
        .and_then(|release| release.files.into_iter().next())
        .with_context(|| format!("MariaDB {newest} lists no files to probe mirrors with"))?;

    let mut entries: Vec<(&str, &models::Mirror)> = mirrors
        .mirror_list
        .iter()
        .flat_map(|(country, mirrors)| mirrors.iter().map(move |m| (country.as_str(), m)))
        .collect();
    entries.sort_by_key(|(country, mirror)| (*country, mirror.mirror_id.as_str()));

    eprintln!(
        "Resolving {} mirrors against {}...",
        entries.len(),
        probe.file_name
    );

    let probe_id = probe.file_id;
    let probe_name = probe.file_name.as_str();

    // Bounded so a listing does not open 94 connections at once.
    let resolved: Vec<Option<String>> = stream::iter(entries.iter().map(|(_, mirror)| {
        let mirror_id = mirror.mirror_id.as_str();
        async move {
            client
                .resolve_mirror(probe_id, mirror_id)
                .await
                .map(|url| mirror_base(&url, probe_name))
        }
    }))
    .buffered(8)
    .collect()
    .await;

    let mut table = Table::new();
    table.add_row(row!["Country", "Mirror id", "Mirror name", "Serves from"]);

    for ((country, mirror), url) in entries.iter().zip(resolved) {
        table.add_row(row![
            country,
            mirror.mirror_id,
            mirror.mirror_name,
            url.as_deref().unwrap_or("(unavailable)")
        ]);
    }

    println!("Mirrors:");
    table.printstd();
    Ok(())
}

/// Trim the file-specific tail off a resolved download URL, leaving the root the
/// mirror serves MariaDB from.
fn mirror_base(resolved: &str, file_name: &str) -> String {
    // The redirect ends in <root>/<release-dir>/<package-dir>/<file-name>.
    let trimmed = resolved
        .rfind(file_name)
        .map(|end| &resolved[..end])
        .unwrap_or(resolved);

    let root = trimmed
        .trim_end_matches('/')
        .rsplit_once('/')
        .map(|(head, _)| head)
        .and_then(|head| head.trim_end_matches('/').rsplit_once('/'))
        .map(|(head, _)| head)
        .unwrap_or(trimmed);

    format!("{}/", root.trim_end_matches('/'))
}

fn print_operating_systems(systems: &models::OsListRoot) {
    let mut table = Table::new();
    table.add_row(row!["Os id", "Name"]);

    for os in &systems.os_list {
        table.add_row(row![os.os_id, os.os_name]);
    }

    println!("Operating systems:");
    table.printstd();
}

fn print_architectures(architectures: &models::ArchitectureListRoot) {
    let mut table = Table::new();
    table.add_row(row!["Architecture id", "Name"]);

    for architecture in &architectures.architecture_list {
        table.add_row(row![
            architecture.architecture_id,
            architecture.architecture_name
        ]);
    }

    println!("Architectures:");
    table.printstd();
}

fn print_checksum(checksum: &models::ChecksumRoot) {
    let mut algorithms: Vec<_> = checksum.response.checksum.iter().collect();
    algorithms.sort_by_key(|(algorithm, _)| *algorithm);

    let mut table = Table::new();
    table.add_row(row!["Algorithm", "Checksum"]);

    for (algorithm, value) in algorithms {
        table.add_row(row![algorithm, value.as_deref().unwrap_or_default()]);
    }

    table.printstd();
}

#[cfg(test)]
mod tests {
    use super::{mirror_base, product_hint};
    use crate::api::models::{ProductListRoot, ProductsList};

    fn products() -> ProductListRoot {
        ProductListRoot {
            products_list: vec![
                ProductsList {
                    product_id: "mariadb".to_string(),
                    name: "MariaDB Server".to_string(),
                    description: String::new(),
                    license: Some("GPL".to_string()),
                },
                ProductsList {
                    product_id: "connector-odbc".to_string(),
                    name: "MariaDB Connector/ODBC".to_string(),
                    description: String::new(),
                    license: Some("LGPL".to_string()),
                },
            ],
        }
    }

    #[test]
    fn points_a_product_id_at_the_command_that_takes_one() {
        let hint = product_hint("connector-odbc", &products()).expect("recognised as a product");

        assert!(hint.contains("MariaDB Connector/ODBC"), "{hint}");
        assert!(
            hint.contains("mariadbcli releases connector-odbc"),
            "{hint}"
        );
    }

    #[test]
    fn leaves_a_genuine_typo_to_clap() {
        // `lst` is a misspelt command, not a product; clap's own suggestion is
        // better than pointing it at `releases`.
        assert!(product_hint("lst", &products()).is_none());
        assert!(product_hint("mariadb-server", &products()).is_none());
    }

    #[test]
    fn trims_the_release_and_package_directories_off_a_resolved_url() {
        // Mirrors are reached through a redirect ending in
        // <root>/<release>/<package-dir>/<file>, and the doubled slashes are
        // what the service actually emits.
        assert_eq!(
            mirror_base(
                "https://mirror.alwyzon.net/mariadb//mariadb-12.3.3/winx64-packages/mariadb-12.3.3-winx64.msi",
                "mariadb-12.3.3-winx64.msi"
            ),
            "https://mirror.alwyzon.net/mariadb/"
        );
        assert_eq!(
            mirror_base(
                "https://www.nic.funet.fi/pub/mirrors/mariadb.com/mariadb///mariadb-12.3.3/winx64-packages/mariadb-12.3.3-winx64.msi",
                "mariadb-12.3.3-winx64.msi"
            ),
            "https://www.nic.funet.fi/pub/mirrors/mariadb.com/mariadb/"
        );
        assert_eq!(
            mirror_base(
                "http://mirror.aarnet.edu.au/pub/MariaDB///mariadb-12.3.3/yum/mariadb-12.3.3.rpm",
                "mariadb-12.3.3.rpm"
            ),
            "http://mirror.aarnet.edu.au/pub/MariaDB/"
        );
    }
}
