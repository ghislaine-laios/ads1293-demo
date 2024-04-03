use actix_web::{middleware::Logger, web, App, HttpServer};
use actors::{
    service_broadcast_manager::ServiceBroadcastManager, service_broadcaster::ServiceBroadcaster,
};
use anyhow::Context;
use settings::Settings;
use smallvec::SmallVec;
use tokio::select;

use crate::actors::service_broadcast_manager;

pub mod actors;
pub mod services;
pub mod settings;
#[cfg(test)]
pub mod tests_utils;

#[cfg(test)]
const MODULE_PATH: &'static str = module_path!();

pub async fn app() -> anyhow::Result<()> {
    let settings = Settings::new()?;

    let service_broadcaster =
        ServiceBroadcaster::new(settings.bind_to.clone(), settings.broadcast.clone()).await?;
    let service_manager = ServiceBroadcastManager::new(service_broadcaster);
    let (launched_service_broadcast_manager, service_broadcast_manager_fut) =
        service_manager.launch();

    let bind_to = &settings.bind_to;
    let bind_to = (bind_to.ip.as_str(), bind_to.port);

    let server_fut = HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .app_data(web::Data::new(launched_service_broadcast_manager.clone()))
            .service(services::push_data::push_data)
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
        result = service_broadcast_manager_fut => {
            handle_service_broadcast_manager_errors(result.unwrap());
            return Err(anyhow::anyhow!("the service broadcast manager ended with error"));
        }
    }

    Ok(())
}
