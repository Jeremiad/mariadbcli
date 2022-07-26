extern crate clap;
use prettytable::{Table};
use std::env;
use crate::api::requests;

use clap::App;
use clap::load_yaml;

pub async fn parse_arguments()
{
    let yaml = load_yaml!("clicommands.yml");
    let matches = App::from_yaml(yaml).get_matches();

    if matches.is_present("list") {
        list().await;
    }
    if let Some(release) = matches.value_of("listreleases") {
        list_releases(release).await;
    }
}

pub async fn list()
{
    let products = requests::get_products().await.unwrap();
  
    let mut table = Table::new();
    table.add_row(row!["Product id", "Product name", "License"]);

    for product in products.products_list {
        table.add_row(row![product.product_id, product.name, product.license.unwrap_or_default()]);
    }
    println!("Products:");
    table.printstd();
}

pub async fn list_releases(product_id: &str)
{
    let releases = requests::get_product_releases(product_id).await.unwrap();

    let mut table = Table::new();
    table.add_row(row!["Id", "Name", "Status"]);
    println!("Product Releases:");
    for release in releases.major_releases {
        table.add_row(row![release.release_id, release.release_name, release.release_status]);
    }
    table.printstd();
}

pub async fn list_of_point_releases(product_id: &str, release_id: &str)
{
    let point_releases = requests::get_list_of_point_releases(product_id, release_id).await.unwrap();

    let mut table = Table::new();
    println!("Point release info:");
    table.add_row(row!["Id", "Name", "Release date", "Release Notes", "Change log", "Files"]);
    for (k, v) in point_releases.releases {
        table.add_row(row![k, v.release_id, v.release_name, v.date_of_release, v.release_notes_url, v.change_log/*, v.files*/]);
    }
    table.printstd();
}

pub fn install()
{
    let args: Vec<String> = env::args().collect();
    println!("{:?}", args);
}

pub fn download()
{
    let args: Vec<String> = env::args().collect();
    println!("{:?}", args);
}