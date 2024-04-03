use normal_server::app;

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    app().await
}
