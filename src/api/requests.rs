use crate::api::models;
use prettytable::{Table};

pub async fn get_products() -> Result<models::ProductListRoot, Box<dyn std::error::Error>> {
    
    let client = reqwest::Client::builder().build()?;

    let resp = client.get("https://downloads.mariadb.org/rest-api/products/")
        .send()
        .await?
        .text()
        .await?;
    let json: models::ProductListRoot = serde_json::from_str(&resp).unwrap();
    Ok(json)
}

pub async fn get_product_releases(product_id: &str) -> Result<models::MajorReleasesRoot, Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder().build()?;
    let resp = client.get(format!("https://downloads.mariadb.org/rest-api/{}", product_id))
        .send()
        .await?;

    if resp.status() != 200 {
        panic!("{}", resp.text().await?);
    }
    else {
        let json: models::MajorReleasesRoot = serde_json::from_str(&resp.text().await?).unwrap();
        Ok(json)
    }
}

pub async fn get_list_of_point_releases(product_id: &str, release_id: &str) -> Result<models::PointReleasesRoot, Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder().build()?;
    let resp = client.get(format!("https://downloads.mariadb.org/rest-api/{0}/{1}", product_id, release_id))
        .send()
        .await?;

    if resp.status() != 200 {
        panic!("{}", resp.text().await?);
    }
    else {
        let json: models::PointReleasesRoot = serde_json::from_str(&resp.text().await?).unwrap();
        Ok(json)
    }
}

pub async fn get_list_of_files_for_release(product_id: &String, point_product_id: &String) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder().build()?;
    let resp = client.get(format!("https://downloads.mariadb.org/rest-api/{0}/{1}", product_id, point_product_id))
        .send()
        .await?
        .text()
        .await?;
    let json: models::ReleaseFilesRoot = serde_json::from_str(&resp).unwrap();
    let mut table = Table::new();
    table.add_row(row!["Id", "Name", "Release Date", "Release notes", "Release Url", "Change log", "Files"]);
    table.add_row(row![json.release_data.release_id, json.release_data.release_name, json.release_data.date_of_release, json.release_data.release_notes_url, json.release_data.change_log]);
    table.printstd();

    /*let mut files_table = Table::new();
    files_table.add_row(row!["Id", "Name", "Type", "Os", "Cpu", "Url"]);
    for file in json.release_data.files {
        files_table.add_row(row![file.file_id, file.file_name, file.os, file.cpu, file.file_download_url]);
    }*/
    //files_table.print_tty(true);
    Ok(())
}