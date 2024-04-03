use anyhow::anyhow;
use normal_server::actors::service_broadcast_manager::{self, ServiceBroadcastManager};
use smallvec::SmallVec;
use tokio::select;

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

    let service_broadcaster =
        ServiceBroadcaster::new(settings.bind_to.clone(), settings.broadcast.clone()).await?;
    let service_manager = ServiceBroadcastManager::new(service_broadcaster);
    let (launched_service_manager, service_manager_fut) = service_manager.launch();

    let bind_to = &settings.bind_to;
    let bind_to = (bind_to.ip.as_str(), bind_to.port);

    let server_fut = HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .app_data(web::Data::new(launched_service_manager.clone()))
    })
    .bind(bind_to)
    .context(format!("failed to bind to {}:{}", bind_to.0, bind_to.1))?
    .run();

    fn handle_service_broadcast_manager_errors(
        errors: SmallVec<[service_broadcast_manager::Error; 2]>,
    ) {
        for error in errors {
            log::error!("service broadcast manager ended with error: {:#?}", error);
        }
    }

    select! {
        result = server_fut => {
            result.context("the server ended with error")?;
        },
        result = service_manager_fut => {
            handle_service_broadcast_manager_errors(result.unwrap());
            return Err(anyhow!("the service broadcast manager ended with error"));
        }
    }

    Ok(())
}
