//! Response shapes of the MariaDB Downloads REST API.
//!
//! Field names already match the JSON, so no serde renaming is needed.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// <https://mariadb.org/downloads-rest-api/#all-products>
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductListRoot {
    pub products_list: Vec<ProductsList>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductsList {
    pub product_id: String,
    pub name: String,
    pub description: String,
    pub license: Option<String>,
}

/// <https://mariadb.org/downloads-rest-api/#list-of-major-minor-releases>
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MajorReleasesRoot {
    pub major_releases: Vec<MajorRelease>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MajorRelease {
    pub release_id: String,
    pub release_name: String,
    pub release_status: String,
}

/// <https://mariadb.org/downloads-rest-api/#list-of-point-releases>
///
/// Keyed by version string rather than returned as a list.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointReleasesRoot {
    pub releases: HashMap<String, Release>,
}

/// <https://mariadb.org/downloads-rest-api/#list-of-files-for-a-release>
///
/// Keyed by version string, like [`PointReleasesRoot`], even though a request
/// for a single point release returns exactly one entry.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReleaseFilesRoot {
    pub release_data: HashMap<String, Release>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Release {
    pub release_id: String,
    pub release_name: String,
    pub date_of_release: String,
    pub release_notes_url: String,
    pub change_log: String,
    pub files: Vec<File>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct File {
    pub file_id: i64,
    pub file_name: String,
    pub package_type: Option<String>,
    pub os: Option<String>,
    pub cpu: Option<String>,
    pub checksum: HashMap<String, Option<String>>,
    pub signature: Option<String>,
    pub checksum_url: String,
    pub signature_url: String,
    pub file_download_url: String,
}

/// <https://mariadb.org/downloads-rest-api/#available-mirrors>
///
/// Mirrors are grouped by country name.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirrorListRoot {
    pub mirror_list: HashMap<String, Vec<Mirror>>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mirror {
    pub mirror_id: String,
    pub mirror_name: String,
}

/// <https://mariadb.org/downloads-rest-api/#available-operating-systems>
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OsListRoot {
    pub os_list: Vec<Os>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Os {
    pub os_id: String,
    pub os_name: String,
}

/// <https://mariadb.org/downloads-rest-api/#available-architectures>
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArchitectureListRoot {
    pub architecture_list: Vec<Architecture>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Architecture {
    pub architecture_id: String,
    pub architecture_name: String,
}

/// <https://mariadb.org/downloads-rest-api/#file-checksums>
///
/// Wrapped one level deeper than the checksum map embedded in [`File`].
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChecksumRoot {
    pub response: ChecksumResponse,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChecksumResponse {
    pub checksum: HashMap<String, Option<String>>,
}
