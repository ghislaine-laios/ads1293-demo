use anyhow::Context;



#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    use actix_web::middleware::Logger;
    use actix_web::{App, HttpServer};
    use settings::Settings;

    env_logger::init();

    let settings = Settings::new()?;
    let bind_to = &settings.bind_to;
    let bind_to = (bind_to.ip.as_str(), bind_to.port);

    HttpServer::new(move || App::new().wrap(Logger::default()))
        .bind(bind_to)
        .context(format!("failed to bind to {}:{}", bind_to.0, bind_to.1))?
        .run()
        .await?;

    Ok(())
}
