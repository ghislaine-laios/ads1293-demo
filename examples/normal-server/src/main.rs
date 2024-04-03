#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    use actix_web::middleware::Logger;
    use actix_web::web;
    use actix_web::{App, HttpServer};
    use anyhow::Context;
    use normal_server::actors::service_broadcaster::ServiceBroadcaster;
    use normal_server::settings::Settings;

    env_logger::init();

    let settings = Settings::new()?;

    let (service_broadcaster, _) =
        ServiceBroadcaster::new(settings.bind_to.clone(), settings.broadcast.clone())
            .await
            .expect("failed to setup new service broadcaster")
            .launch();

    let bind_to = &settings.bind_to;
    let bind_to = (bind_to.ip.as_str(), bind_to.port);

    HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .app_data(web::Data::new(service_broadcaster.clone()))
    })
    .bind(bind_to)
    .context(format!("failed to bind to {}:{}", bind_to.0, bind_to.1))?
    .run()
    .await?;

    Ok(())
}
