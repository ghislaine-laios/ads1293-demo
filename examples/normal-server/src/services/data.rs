use crate::{
    actors::{
        data_hub::LaunchedDataHub, data_processor::ReceiveDataFromHardware,
        data_pusher::DataPusher, service_broadcast_manager::LaunchedServiceBroadcastManager,
    },
    errors,
};
use actix_web::{get, web, Error, HttpRequest, HttpResponse};
use sea_orm::DatabaseConnection;
use std::sync::Arc;

/// The device push data to this endpoint using websocket.
#[get("/push-data")]
pub async fn push_data(
    req: HttpRequest,
    stream: web::Payload,
    launched_service_broadcast_manager: web::Data<LaunchedServiceBroadcastManager>,
    launched_data_hub: web::Data<LaunchedDataHub>,
    db_coon: web::Data<DatabaseConnection>,
) -> Result<HttpResponse, Error> {
    Ok(actix_web_actors::ws::handshake(&req)?.streaming(
        ReceiveDataFromHardware::launch_inline(
            stream,
            Arc::unwrap_or_clone(db_coon.into_inner()),
            Arc::unwrap_or_clone(launched_service_broadcast_manager.into_inner()),
            Arc::unwrap_or_clone(launched_data_hub.into_inner()),
        )
        .await
        .map_err(|e| e.into())
        .map_err(errors::Error::InternalError)?,
    ))
}

#[get("/data")]
pub async fn retrieve_data(
    req: HttpRequest,
    stream: web::Payload,
    launched_data_hub: web::Data<LaunchedDataHub>,
) -> Result<HttpResponse, Error> {
    let mut resp = actix_web_actors::ws::handshake(&req)?;

    Ok(resp.streaming(
        DataPusher::launch_inline(stream, Arc::unwrap_or_clone(launched_data_hub.into_inner()))
            .await,
    ))
}

#[cfg(test)]
mod tests {
    use crate::{app, settings::Settings};
    use futures::{SinkExt, StreamExt};
    use normal_data::Data;
    use std::time::Duration;
    use tokio::select;
    use tokio_tungstenite::connect_async;
    use url::Url;

    #[actix_web::test]
    async fn it_works() {
        use tokio_tungstenite::tungstenite::Message::*;

        crate::tests_utils::setup_logger();
        let settings = Settings::new().unwrap();

        let main = actix_rt::spawn(async move { app().await });

        actix_rt::time::sleep(Duration::from_millis(500)).await;

        let port = settings.bind_to.port;

        let listener_task = async move {
            let (mut socket, resp) = connect_async(
                Url::parse(format!("ws://localhost:{}/data", port).as_str()).unwrap(),
            )
            .await
            .unwrap();

            dbg!(resp);

            while let Some(item) = socket.next().await {
                match item {
                    Ok(item) => {
                        log::info!("receive item: {:?}", item)
                    }
                    Err(e) => {
                        log::error!("listener task receive error: {:#?}", e);
                        panic!()
                    }
                }
            }
        };

        let listener_task = actix_rt::spawn(listener_task);

        let port = settings.bind_to.port;

        let client_task = async move {
            let (mut socket, resp) = connect_async(
                Url::parse(format!("ws://localhost:{}/push-data", port).as_str()).unwrap(),
            )
            .await
            .unwrap();

            dbg!(resp);

            // Feed the data. Lots of data.

            let total_num: u32 = 300;
            let mut data_arr = Vec::with_capacity(total_num.try_into().unwrap());
            for i in 0..total_num {
                data_arr.push(Data {
                    id: i,
                    value: i * 2,
                })
            }

            for data in data_arr.iter() {
                socket
                    .feed(Text(serde_json::to_string(data).unwrap()))
                    .await
                    .unwrap();
            }
            socket.flush().await.unwrap();

            for i in 0..60 {
                data_arr[i].id += total_num;
                socket
                    .send(Text(serde_json::to_string(&data_arr[i]).unwrap()))
                    .await
                    .unwrap();
                actix_rt::time::sleep(Duration::from_millis(10)).await;
            }
        };

        let client_task = actix_rt::spawn(client_task);

        select! {
            r = main => {
                if let Err(e) = r {
                    panic!("main task ended with error {:#?}", e)
                }
            },
            r = client_task => {
                if let Err(e) = r {
                    panic!("client task panicked: {:#?}", e)
                }
            }
            r = listener_task => {
                if let Err(e) = r {
                    panic!("listener task panicked: {:#?}", e)
                }
            }
        }
    }
}
