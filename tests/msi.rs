//! Windows installer planning tests.
//!
//! `install` on Windows runs a downloaded MSI as an administrator, so the two
//! decisions worth pinning are which file it picks out of a release and what
//! command line it builds. Both are pure, so they run on any platform.

use std::path::{Path, PathBuf};

use mariadbcli::api::models::{Release, ReleaseFilesRoot};
use mariadbcli::install::{msi::select_installer, MsiOptions, MsiPlan};

fn release() -> Release {
    let root: ReleaseFilesRoot =
        serde_json::from_str(include_str!("fixtures/release_files.json")).unwrap();
    root.release_data.into_values().next().unwrap()
}

fn plan(options: &MsiOptions) -> MsiPlan {
    MsiPlan::build(&release(), "x86_64", PathBuf::from("C:\\tmp"), options).unwrap()
}

#[test]
fn picks_the_msi_over_the_zip_and_the_debug_symbols() {
    let release = release();
    let installer = select_installer(&release, "x86_64").unwrap();

    // The same release ships a winx64 zip, a debugsymbols msi and a source
    // tarball; only one of the four is the installer.
    assert_eq!(installer.file_name, "mariadb-10.11.6-winx64.msi");
    assert_eq!(installer.file_id, 16070);
}

#[test]
fn reports_what_exists_when_there_is_no_installer_for_the_architecture() {
    let release = release();
    let error = select_installer(&release, "aarch64")
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("no Windows aarch64 .msi installer"),
        "{error}"
    );
    // The message lists the files it did find, so the mismatch is obvious.
    assert!(error.contains("mariadb-10.11.6-winx64.msi"), "{error}");
}

#[test]
fn carries_the_published_sha256_into_the_plan() {
    let plan = plan(&MsiOptions::default());

    assert_eq!(plan.release_id, "10.11.6");
    assert_eq!(plan.file_id, 16070);
    assert_eq!(
        plan.expected_sha256.as_deref(),
        Some("1111222233334444555566667777888899990000aaaabbbbccccddddeeeeffff")
    );
}

#[test]
fn defaults_create_a_named_service_on_the_standard_port() {
    // With SERVICENAME empty the MSI installs no service at all, so the default
    // has to be a real name.
    let command = plan(&MsiOptions::default()).command(Path::new("C:\\tmp\\mariadb.msi"));

    assert_eq!(command[0], "msiexec");
    assert_eq!(command[1], "/i");
    assert_eq!(command[2], "C:\\tmp\\mariadb.msi");
    assert!(
        command.contains(&"SERVICENAME=MariaDB".to_string()),
        "{command:?}"
    );
    assert!(command.contains(&"PORT=3306".to_string()), "{command:?}");
    assert!(command.contains(&"/qn".to_string()));
    // Never reboot a machine out from under the caller.
    assert!(command.contains(&"/norestart".to_string()));
}

#[test]
fn passes_through_the_documented_properties() {
    let options = MsiOptions {
        service_name: "MariaDB114".to_string(),
        port: 3307,
        password: Some("s3cret".to_string()),
        install_dir: Some("D:\\MariaDB".to_string()),
        data_dir: Some("E:\\data".to_string()),
        extra: vec![("UTF8".to_string(), "1".to_string())],
    };
    let command = plan(&options).command(Path::new("C:\\tmp\\mariadb.msi"));

    for expected in [
        "SERVICENAME=MariaDB114",
        "PORT=3307",
        "PASSWORD=s3cret",
        "INSTALLDIR=D:\\MariaDB",
        "DATADIR=E:\\data",
        "UTF8=1",
    ] {
        assert!(
            command.contains(&expected.to_string()),
            "{expected} missing from {command:?}"
        );
    }
}

#[test]
fn describe_redacts_the_password() {
    let options = MsiOptions {
        password: Some("s3cret".to_string()),
        ..Default::default()
    };
    let described = plan(&options).describe();

    // The plan is printed to the terminal and may be pasted into a ticket.
    assert!(!described.contains("s3cret"), "{described}");
    assert!(described.contains("PASSWORD=***"), "{described}");
    // The rest of the command is still shown.
    assert!(described.contains("SERVICENAME=MariaDB"));
    assert!(described.contains("mariadb-10.11.6-winx64.msi"));
}

#[test]
fn extra_properties_are_parsed_and_upper_cased() {
    assert_eq!(
        MsiOptions::parse_property("utf8=1").unwrap(),
        ("UTF8".to_string(), "1".to_string())
    );
    // Values keep their case and may contain '='.
    assert_eq!(
        MsiOptions::parse_property("DATADIR=D:\\a=b").unwrap(),
        ("DATADIR".to_string(), "D:\\a=b".to_string())
    );

    assert!(MsiOptions::parse_property("UTF8").is_err());
    assert!(MsiOptions::parse_property("=1").is_err());
}
