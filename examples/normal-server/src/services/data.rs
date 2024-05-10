use crate::{
    actors::{
        data_hub::{LaunchedDataHub, LaunchedDataHubController},
        data_processor::ReceiveDataFromHardware,
        data_pusher::DataPusher,
        service_broadcast_manager::LaunchedServiceBroadcastManager,
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
    launched_data_hub_and_controller: web::Data<(LaunchedDataHub, LaunchedDataHubController)>,
    db_coon: web::Data<DatabaseConnection>,
) -> Result<HttpResponse, Error> {
    let (launched_data_hub, data_hub_controller) =
        Arc::unwrap_or_clone(launched_data_hub_and_controller.into_inner());

    Ok(actix_web_actors::ws::handshake(&req)?.streaming(
        ReceiveDataFromHardware::launch_inline(
            stream,
            Arc::unwrap_or_clone(db_coon.into_inner()),
            Arc::unwrap_or_clone(launched_service_broadcast_manager.into_inner()),
            launched_data_hub,
            data_hub_controller,
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
    launched_data_hub_and_controller: web::Data<(LaunchedDataHub, LaunchedDataHubController)>,
) -> Result<HttpResponse, Error> {
    let mut resp = actix_web_actors::ws::handshake(&req)?;

    let (_, data_hub_controller) =
        Arc::unwrap_or_clone(launched_data_hub_and_controller.into_inner());

    Ok(resp.streaming(
        DataPusher::launch_inline(stream, data_hub_controller)
            .await
            .unwrap(),
    ))
}

#[cfg(test)]
mod tests {
    use crate::{app, settings::Settings};
    use abort_on_drop::ChildTask;
    use futures::{SinkExt, StreamExt};
    use mint::Vector3;
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

        actix_rt::time::sleep(Duration::from_millis(100)).await;

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

        let listener_task = ChildTask::from(actix_rt::spawn(listener_task));

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
                    ecg: (i * 2, i & 4),
                    quaternion: mint::Quaternion::from([
                        -0.110839844,
                        -0.06317139,
                        0.00018310547,
                        0.9918213,
                    ]),
                    accel: Vector3::from([f32::MAX, f32::MAX, f32::MAX]),
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

        let client_task = ChildTask::from(actix_rt::spawn(client_task));

        select! {
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
        tokio::time::sleep(Duration::from_millis(10)).await;

        drop(main)
    }
}
