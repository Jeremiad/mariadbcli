#[macro_use] extern crate prettytable;
mod api;
mod clicommands;

#[tokio::main]
async fn main(){
    let _product = "mariadb".to_string();
    let _point_release = "10.5".to_string();
    let _point_product = "10.5.11".to_string();

    clicommands::parse_arguments().await;

    //let _foo = api::get_products().await.unwrap();

    /*let _foo = match _foo {
        Ok(ok) => ok,
        Err(error) => panic!("{:?}", error),
    };*/
    /*let foo2 = api::get_product_version(&product).await;
    let foo3 = api::get_list_of_point_releases(&product, &point_release).await;
    let foo4 = api::get_list_of_files_for_release(&product, &point_product).await;*/
}