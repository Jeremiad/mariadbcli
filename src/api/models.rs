use serde::{Deserialize, Serialize};
use std::collections::HashMap;

//
// https://mariadb.org/downloads-rest-api/#all-products
//
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductListRoot {
    #[serde(rename = "products_list")]
    pub products_list: Vec<ProductsList>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductsList {
    #[serde(rename = "product_id")]
    pub product_id: String,
    pub name: String,
    pub description: String,
    pub license: String,
}

//
// https://mariadb.org/downloads-rest-api/#list-of-major-minor-releases
//
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MajorReleasesRoot {
    #[serde(rename = "major_releases")]
    pub major_releases: Vec<MajorRelease>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MajorRelease {
    #[serde(rename = "release_id")]
    pub release_id: String,
    #[serde(rename = "release_name")]
    pub release_name: String,
    #[serde(rename = "release_status")]
    pub release_status: String,
}

//
// https://mariadb.org/downloads-rest-api/#list-of-point-releases
//
#[derive(Serialize, Deserialize)]
pub struct PointReleasesRoot {
    pub releases: HashMap<String, Release>,
}

//
// https://mariadb.org/downloads-rest-api/#list-of-files-for-a-release
//
#[derive(Serialize, Deserialize)]
pub struct ReleaseFilesRoot {
    pub release_data: Release,
}

//
// Common
//

#[derive(Serialize, Deserialize)]
pub struct Release {
    pub release_id: String,
    pub release_name: String,
    pub date_of_release: String,
    pub release_notes_url: String,
    pub change_log: String,
    pub files: Vec<File>,
}

#[derive(Serialize, Deserialize)]
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