use std::env;

use normal_server::{app, setup_production_logger};

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    setup_production_logger();

    app().await
}
