#![feature(map_try_insert)]

use crate::actors::{data_hub::DataHub, service_broadcast_manager};
use actix_web::{middleware::Logger, web, App, HttpServer};
use actors::{
    service_broadcast_manager::ServiceBroadcastManager, service_broadcaster::ServiceBroadcaster,
};
use anyhow::Context;
use migration::{Migrator, MigratorTrait};
use sea_orm::Database;
use settings::Settings;
use smallvec::SmallVec;
use std::time::Duration;
use tokio::select;

pub mod actors;
pub mod entities;
pub mod errors;
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

    let data_hub = DataHub::new();
    let (launched_data_hub, data_hub_controller, data_hub_fut) = data_hub.launch();

    let bind_to = &settings.bind_to;
    let bind_to = (bind_to.ip.as_str(), bind_to.port);

    let db_coon = Database::connect(&settings.database.url)
        .await
        .context("Can't connect to the database")?;

    actix_rt::time::timeout(Duration::from_secs(3), db_coon.ping())
        .await
        .context("timeout to ping the database")?
        .context("failed to ping the database")?;

    log::info!("Connected to the database.");

    // Confirm the application of pending migrations (in production).

    let pending_migrations = Migrator::get_pending_migrations(&db_coon)
        .await
        .expect("Failed to get the pending migrations");
    if !pending_migrations.is_empty() {
        return Err(anyhow::anyhow!("Pending migrations await application."));
    }

    let server_fut = HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .app_data(web::Data::new(launched_service_broadcast_manager.clone()))
            .app_data(web::Data::new((
                launched_data_hub.clone(),
                data_hub_controller.clone(),
            )))
            .app_data(web::Data::new(db_coon.clone()))
            .service(services::data::push_data)
            .service(services::data::retrieve_data)
    })
    .workers(2)
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
        },
        result = data_hub_fut => {
            result.context("the data hub panicked")?
            .context("the data hub ended with error")?
        }
    }

    Ok(())
}
