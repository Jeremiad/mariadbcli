//! Deserialization tests for the API response shapes.
//!
//! The models are hand-written against a third-party API, so these fixtures are
//! the guard against the published shape drifting away from them. No network.

use mariadbcli::api::models::{
    ArchitectureListRoot, ChecksumRoot, MajorReleasesRoot, MirrorListRoot, OsListRoot,
    PointReleasesRoot, ProductListRoot, ReleaseFilesRoot,
};

#[test]
fn parses_product_list() {
    let root: ProductListRoot =
        serde_json::from_str(include_str!("fixtures/products.json")).unwrap();

    assert_eq!(root.products_list.len(), 2);
    assert_eq!(root.products_list[1].product_id, "mariadb");
    assert_eq!(root.products_list[1].license.as_deref(), Some("GPL"));
    // Not every product declares a license.
    assert_eq!(root.products_list[0].license, None);
}

#[test]
fn parses_major_releases() {
    let root: MajorReleasesRoot =
        serde_json::from_str(include_str!("fixtures/major_releases.json")).unwrap();

    assert_eq!(root.major_releases.len(), 2);
    assert_eq!(root.major_releases[0].release_id, "10.11");
    assert_eq!(root.major_releases[0].release_status, "Stable");
}

#[test]
fn parses_point_releases_keyed_by_version() {
    let root: PointReleasesRoot =
        serde_json::from_str(include_str!("fixtures/point_releases.json")).unwrap();

    assert_eq!(root.releases.len(), 2);
    let release = root.releases.get("10.11.6").expect("10.11.6 present");
    assert_eq!(release.release_name, "MariaDB Server 10.11.6");
    assert_eq!(release.date_of_release, "2023-11-13");
}

#[test]
fn parses_release_files() {
    let root: ReleaseFilesRoot =
        serde_json::from_str(include_str!("fixtures/release_files.json")).unwrap();

    // `release_data` is keyed by version even for a single point release.
    assert_eq!(root.release_data.len(), 1);
    let release = root.release_data.get("10.11.6").expect("10.11.6 present");
    assert_eq!(release.release_id, "10.11.6");
    assert_eq!(release.files.len(), 4);

    let find = |name: &str| {
        release
            .files
            .iter()
            .find(|file| file.file_name == name)
            .unwrap_or_else(|| panic!("{name} present"))
    };

    let binary = find("mariadb-10.11.6-winx64.zip");
    assert_eq!(binary.file_id, 16068);
    assert_eq!(binary.os.as_deref(), Some("Windows"));
    assert_eq!(binary.cpu.as_deref(), Some("x86_64"));
    // Checksum entries are present-but-null for algorithms not published.
    assert_eq!(binary.checksum.get("sha1sum"), Some(&None));
    assert!(binary.checksum["md5sum"].is_some());

    // Source tarballs carry no os/cpu.
    let source = find("mariadb-10.11.6.tar.gz");
    assert_eq!(source.os, None);
    assert_eq!(source.cpu, None);
}

#[test]
fn ignores_unknown_fields() {
    // The API is free to add fields; that must not break existing commands.
    let json = r#"{
        "products_list": [
            {
                "product_id": "mariadb",
                "name": "MariaDB Server",
                "description": "d",
                "license": "GPL",
                "some_future_field": {"nested": true}
            }
        ],
        "another_future_field": 1
    }"#;

    let root: ProductListRoot = serde_json::from_str(json).unwrap();
    assert_eq!(root.products_list[0].product_id, "mariadb");
}

#[test]
fn parses_mirrors_grouped_by_country() {
    let root: MirrorListRoot = serde_json::from_str(include_str!("fixtures/mirrors.json")).unwrap();

    assert_eq!(root.mirror_list.len(), 2);
    let australia = root
        .mirror_list
        .get("Australia")
        .expect("Australia present");
    assert_eq!(australia.len(), 2);
    assert_eq!(australia[0].mirror_id, "aarnet_pty_ltd");
}

#[test]
fn parses_operating_systems() {
    let root: OsListRoot = serde_json::from_str(include_str!("fixtures/os.json")).unwrap();

    assert_eq!(root.os_list.len(), 5);
    assert!(root.os_list.iter().any(|os| os.os_id == "windows"));
}

#[test]
fn parses_architectures() {
    let root: ArchitectureListRoot =
        serde_json::from_str(include_str!("fixtures/cpu.json")).unwrap();

    assert_eq!(root.architecture_list.len(), 4);
    let amd64 = &root.architecture_list[0];
    // The filter id and the display name differ; --cpu takes the id.
    assert_eq!(amd64.architecture_id, "amd64");
    assert_eq!(amd64.architecture_name, "x86_64");
}

#[test]
fn parses_checksum_response() {
    let root: ChecksumRoot = serde_json::from_str(include_str!("fixtures/checksum.json")).unwrap();

    // Nested one level deeper than the checksum map embedded in a file listing.
    let checksum = &root.response.checksum;
    assert_eq!(checksum.len(), 4);
    assert_eq!(
        checksum["md5sum"].as_deref(),
        Some("424df7487f340fd8a9056bc43d5ec8b7")
    );
}
