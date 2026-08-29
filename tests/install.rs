//! Repository-setup planning tests.
//!
//! `install` writes to /etc and runs a package manager, so everything decidable
//! beforehand is a pure function and is pinned here against real `/etc/os-release`
//! contents. Nothing in this file touches the system or the network.

use mariadbcli::api::models::{MajorRelease, MajorReleasesRoot};
use mariadbcli::install::{latest_stable, AptFormat, Distro, PackageManager, Plan, PlanOptions};

fn deb822() -> PlanOptions {
    PlanOptions::default()
}

fn one_line() -> PlanOptions {
    PlanOptions {
        apt_format: AptFormat::OneLine,
        ..Default::default()
    }
}

fn distro(fixture: &str) -> Distro {
    let contents = match fixture {
        "ubuntu-24.04" => include_str!("fixtures/os-release/ubuntu-24.04"),
        "debian-12" => include_str!("fixtures/os-release/debian-12"),
        "linuxmint-21.3" => include_str!("fixtures/os-release/linuxmint-21.3"),
        "rocky-9" => include_str!("fixtures/os-release/rocky-9"),
        "fedora-40" => include_str!("fixtures/os-release/fedora-40"),
        "opensuse-leap-15.6" => include_str!("fixtures/os-release/opensuse-leap-15.6"),
        "centos-7" => include_str!("fixtures/os-release/centos-7"),
        "arch" => include_str!("fixtures/os-release/arch"),
        other => panic!("unknown fixture {other}"),
    };
    Distro::from_os_release(contents).expect("fixture parses")
}

#[test]
fn parses_quoted_and_unquoted_values() {
    let ubuntu = distro("ubuntu-24.04");

    assert_eq!(ubuntu.id, "ubuntu");
    assert_eq!(ubuntu.version_id, "24.04");
    assert_eq!(ubuntu.codename.as_deref(), Some("noble"));
    assert_eq!(ubuntu.id_like, vec!["debian"]);
    assert_eq!(ubuntu.display_name(), "Ubuntu 24.04.1 LTS");
}

#[test]
fn prefers_ubuntu_codename_on_derivatives() {
    // Mint's own codename (virginia) has no suite upstream; the Ubuntu base does.
    let mint = distro("linuxmint-21.3");
    assert_eq!(mint.codename.as_deref(), Some("jammy"));

    let plan = Plan::build(&mint, "11.4", "x86_64", &deb822()).unwrap();
    assert!(
        plan.repo_contents.contains("Suites: jammy"),
        "{}",
        plan.repo_contents
    );
    assert!(plan
        .repo_contents
        .contains("URIs: https://deb.mariadb.org/11.4/ubuntu"));
}

#[test]
fn builds_ubuntu_apt_repository() {
    let plan = Plan::build(&distro("ubuntu-24.04"), "11.4", "x86_64", &deb822()).unwrap();

    assert_eq!(plan.manager, PackageManager::Apt);
    assert_eq!(
        plan.repo_file.to_str().unwrap(),
        "/etc/apt/sources.list.d/mariadb.sources"
    );
    assert!(plan
        .repo_contents
        .contains("URIs: https://deb.mariadb.org/11.4/ubuntu"));
    assert!(plan.repo_contents.contains("Suites: noble"));
    assert!(plan
        .repo_contents
        .contains("Signed-By: /etc/apt/keyrings/mariadb-keyring.gpg"));
    assert_eq!(
        plan.verify_url,
        "https://deb.mariadb.org/11.4/ubuntu/dists/noble/Release"
    );

    // apt needs the key on disk before it can verify the suite.
    let keyring = plan.keyring.as_ref().expect("apt plan installs a keyring");
    assert_eq!(
        keyring.path.to_str().unwrap(),
        "/etc/apt/keyrings/mariadb-keyring.gpg"
    );

    let commands = plan.commands();
    assert_eq!(commands[0], vec!["apt-get", "update"]);
    assert_eq!(
        commands[1],
        vec!["apt-get", "install", "-y", "mariadb-server"]
    );
}

#[test]
fn builds_debian_apt_repository_under_debian_flavour() {
    let plan = Plan::build(&distro("debian-12"), "11.4", "x86_64", &deb822()).unwrap();

    assert!(plan
        .repo_contents
        .contains("URIs: https://deb.mariadb.org/11.4/debian"));
    assert!(plan.repo_contents.contains("Suites: bookworm"));
}

#[test]
fn builds_rocky_rpm_repository_with_major_version_only() {
    let plan = Plan::build(&distro("rocky-9"), "11.4", "x86_64", &deb822()).unwrap();

    assert_eq!(plan.manager, PackageManager::Dnf);
    assert_eq!(
        plan.repo_file.to_str().unwrap(),
        "/etc/yum.repos.d/mariadb.repo"
    );
    // ID is `rocky` but the repository directory is `rockylinux`, and RHEL-likes
    // publish under the major version, not 9.4.
    assert!(
        plan.repo_contents
            .contains("baseurl = https://rpm.mariadb.org/11.4/rockylinux/9/x86_64"),
        "{}",
        plan.repo_contents
    );
    assert!(plan.repo_contents.contains("gpgcheck = 1"));
    // rpm fetches the key from the URL in the repo file; nothing to place first.
    assert!(plan.keyring.is_none());

    assert_eq!(
        plan.commands()[1],
        vec!["dnf", "install", "-y", "MariaDB-server"]
    );
}

#[test]
fn builds_fedora_rpm_repository_with_full_version() {
    let plan = Plan::build(&distro("fedora-40"), "11.4", "aarch64", &deb822()).unwrap();

    assert!(
        plan.repo_contents
            .contains("baseurl = https://rpm.mariadb.org/11.4/fedora/40/aarch64"),
        "{}",
        plan.repo_contents
    );
}

#[test]
fn builds_opensuse_repository_with_zypper() {
    let plan = Plan::build(&distro("opensuse-leap-15.6"), "11.4", "x86_64", &deb822()).unwrap();

    assert_eq!(plan.manager, PackageManager::Zypper);
    assert_eq!(
        plan.repo_file.to_str().unwrap(),
        "/etc/zypp/repos.d/mariadb.repo"
    );
    // The SUSE family publishes under the full version, unlike RHEL-likes.
    assert!(
        plan.repo_contents
            .contains("baseurl = https://rpm.mariadb.org/11.4/opensuse/15.6/x86_64"),
        "{}",
        plan.repo_contents
    );
    assert_eq!(
        plan.commands()[1],
        vec!["zypper", "install", "-y", "MariaDB-server"]
    );
}

#[test]
fn uses_yum_on_el7() {
    let plan = Plan::build(&distro("centos-7"), "10.11", "x86_64", &deb822()).unwrap();

    assert_eq!(plan.manager, PackageManager::Yum);
    assert_eq!(plan.commands()[0], vec!["yum", "makecache"]);
}

#[test]
fn rejects_distributions_without_a_mariadb_repository() {
    let error = Plan::build(&distro("arch"), "11.4", "x86_64", &deb822()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unsupported distribution `arch`"),
        "{error}"
    );
}

#[test]
fn rejects_deb_distro_without_a_codename() {
    let distro = Distro::from_os_release("ID=debian\nVERSION_ID=\"12\"\n").unwrap();
    let error = Plan::build(&distro, "11.4", "x86_64", &deb822()).unwrap_err();
    assert!(error.to_string().contains("no VERSION_CODENAME"), "{error}");
}

#[test]
fn describe_names_every_change() {
    let plan = Plan::build(&distro("ubuntu-24.04"), "11.4", "x86_64", &deb822()).unwrap();
    let described = plan.describe();

    // --dry-run output must name every file written and command run.
    assert!(described.contains("/etc/apt/keyrings/mariadb-keyring.gpg"));
    assert!(described.contains("/etc/apt/sources.list.d/mariadb.sources"));
    assert!(described.contains("apt-get install -y mariadb-server"));
    assert!(described.contains("https://deb.mariadb.org/11.4/ubuntu/dists/noble/Release"));
}

#[test]
fn deb822_plan_installs_the_binary_keyring_with_a_digest() {
    let plan = Plan::build(&distro("ubuntu-24.04"), "11.4", "x86_64", &deb822()).unwrap();
    let keyring = plan.keyring.as_ref().expect("apt plan installs a keyring");

    // Signed-By needs a binary keyring; the armored key at mariadb.org is the
    // wrong encoding for it, so the plan must use the .gpg keyring.
    assert_eq!(
        keyring.path.to_str().unwrap(),
        "/etc/apt/keyrings/mariadb-keyring.gpg"
    );
    assert_eq!(
        keyring.url,
        "https://supplychain.mariadb.com/mariadb-keyring-2025.gpg"
    );
    assert_eq!(keyring.sha256_url, format!("{}.sha256", keyring.url));
    assert!(plan
        .repo_contents
        .contains("Signed-By: /etc/apt/keyrings/mariadb-keyring.gpg"));
}

#[test]
fn rpm_plan_uses_the_armored_key_url() {
    let plan = Plan::build(&distro("rocky-9"), "11.4", "x86_64", &deb822()).unwrap();

    // dnf imports an armored key straight from gpgkey=; it needs no keyring file.
    assert!(plan.keyring.is_none());
    assert!(plan
        .repo_contents
        .contains("gpgkey = https://mariadb.org/mariadb_release_signing_key.pgp"));
}

#[test]
fn one_line_format_writes_a_list_file() {
    let plan = Plan::build(&distro("ubuntu-24.04"), "11.4", "x86_64", &one_line()).unwrap();

    assert_eq!(
        plan.repo_file.to_str().unwrap(),
        "/etc/apt/sources.list.d/mariadb.list"
    );
    assert!(
        plan.repo_contents.contains(
            "deb [arch=amd64 signed-by=/etc/apt/keyrings/mariadb-keyring.gpg] https://deb.mariadb.org/11.4/ubuntu noble main"
        ),
        "{}",
        plan.repo_contents
    );
    // deb822 keys must not leak into the one-line form.
    assert!(!plan.repo_contents.contains("Types:"));
}

#[test]
fn each_apt_format_removes_the_other() {
    let sources = Plan::build(&distro("debian-12"), "11.4", "x86_64", &deb822()).unwrap();
    assert_eq!(
        sources.remove_files,
        vec![std::path::PathBuf::from(
            "/etc/apt/sources.list.d/mariadb.list"
        )]
    );

    let list = Plan::build(&distro("debian-12"), "11.4", "x86_64", &one_line()).unwrap();
    assert_eq!(
        list.remove_files,
        vec![std::path::PathBuf::from(
            "/etc/apt/sources.list.d/mariadb.sources"
        )]
    );

    // rpm distributions have only one repo file, so nothing to clean up.
    let rpm = Plan::build(&distro("fedora-40"), "11.4", "x86_64", &deb822()).unwrap();
    assert!(rpm.remove_files.is_empty());
}

#[test]
fn deb_architecture_uses_dpkg_names() {
    let arm = Plan::build(&distro("ubuntu-24.04"), "11.4", "aarch64", &deb822()).unwrap();
    // dpkg calls it arm64, not aarch64.
    assert!(
        arm.repo_contents.contains("Architectures: arm64"),
        "{}",
        arm.repo_contents
    );

    let intel = Plan::build(&distro("ubuntu-24.04"), "11.4", "x86_64", &deb822()).unwrap();
    assert!(intel.repo_contents.contains("Architectures: amd64"));
}

fn mirrored(url: &str) -> PlanOptions {
    PlanOptions {
        repo_base_url: Some(url.to_string()),
        ..Default::default()
    }
}

#[test]
fn base_url_override_replaces_the_apt_host_only() {
    let plan = Plan::build(
        &distro("ubuntu-24.04"),
        "11.4",
        "x86_64",
        &mirrored("https://mirror.example.com/mariadb"),
    )
    .unwrap();

    // Everything below the base is unchanged, so a mirror must reproduce the
    // upstream layout.
    assert!(
        plan.repo_contents
            .contains("URIs: https://mirror.example.com/mariadb/11.4/ubuntu"),
        "{}",
        plan.repo_contents
    );
    assert_eq!(
        plan.verify_url,
        "https://mirror.example.com/mariadb/11.4/ubuntu/dists/noble/Release"
    );
    assert_eq!(plan.repo_base_url, "https://mirror.example.com/mariadb");

    // The signing keyring is not mirrored: it stays on MariaDB's host and keeps
    // its checksum, so a mirror cannot substitute a key of its own.
    let keyring = plan.keyring.as_ref().unwrap();
    assert_eq!(
        keyring.url,
        "https://supplychain.mariadb.com/mariadb-keyring-2025.gpg"
    );
}

#[test]
fn base_url_override_replaces_the_rpm_host() {
    let plan = Plan::build(
        &distro("rocky-9"),
        "11.4",
        "x86_64",
        &mirrored("https://mirror.example.com/mariadb"),
    )
    .unwrap();

    assert!(
        plan.repo_contents
            .contains("baseurl = https://mirror.example.com/mariadb/11.4/rockylinux/9/x86_64"),
        "{}",
        plan.repo_contents
    );
    // gpgkey is likewise left pointing at MariaDB.
    assert!(plan
        .repo_contents
        .contains("gpgkey = https://mariadb.org/mariadb_release_signing_key.pgp"));
}

#[test]
fn base_url_override_ignores_a_trailing_slash() {
    let with = Plan::build(
        &distro("debian-12"),
        "11.4",
        "x86_64",
        &mirrored("https://mirror.example.com/mariadb/"),
    )
    .unwrap();
    let without = Plan::build(
        &distro("debian-12"),
        "11.4",
        "x86_64",
        &mirrored("https://mirror.example.com/mariadb"),
    )
    .unwrap();

    assert_eq!(with, without);
}

#[test]
fn base_url_override_requires_http() {
    // Verification is an HTTP request, so a scheme it cannot make would quietly
    // skip the check that the repository exists.
    for url in ["file:///srv/mirror", "mirror.example.com", "ftp://host/x"] {
        let error =
            Plan::build(&distro("debian-12"), "11.4", "x86_64", &mirrored(url)).unwrap_err();
        assert!(
            error.to_string().contains("must be http or https"),
            "{url}: {error}"
        );
    }

    let error = Plan::build(&distro("debian-12"), "11.4", "x86_64", &mirrored("   ")).unwrap_err();
    assert!(error.to_string().contains("is empty"), "{error}");
}

#[test]
fn without_an_override_the_upstream_hosts_are_used() {
    let deb = Plan::build(&distro("ubuntu-24.04"), "11.4", "x86_64", &deb822()).unwrap();
    assert_eq!(deb.repo_base_url, "https://deb.mariadb.org");

    let rpm = Plan::build(&distro("fedora-40"), "11.4", "x86_64", &deb822()).unwrap();
    assert_eq!(rpm.repo_base_url, "https://rpm.mariadb.org");
}

fn releases(entries: &[(&str, &str)]) -> MajorReleasesRoot {
    MajorReleasesRoot {
        major_releases: entries
            .iter()
            .map(|(id, status)| MajorRelease {
                release_id: id.to_string(),
                release_name: id.to_string(),
                release_status: status.to_string(),
            })
            .collect(),
    }
}

#[test]
fn latest_stable_compares_versions_numerically() {
    // Lexicographically "10.5" sorts above "10.11"; numerically it must not.
    let root = releases(&[("10.5", "Stable"), ("10.11", "Stable"), ("10.6", "Stable")]);
    assert_eq!(latest_stable(&root), Some("10.11"));
}

#[test]
fn latest_stable_skips_unstable_releases() {
    let root = releases(&[
        ("11.4", "Stable"),
        ("11.8", "RC"),
        ("12.0", "Alpha"),
        ("10.11", "Stable"),
    ]);
    assert_eq!(latest_stable(&root), Some("11.4"));
}

#[test]
fn latest_stable_is_none_without_a_stable_release() {
    assert_eq!(latest_stable(&releases(&[("12.0", "Alpha")])), None);
}
