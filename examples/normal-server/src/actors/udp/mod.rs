use crate::entities::{self, data_transaction::ActiveModel};

use super::{
    data_hub::{
        registration_keepers::DataProcessorRegistrationKeeper, LaunchedDataHub,
        LaunchedDataHubController,
    },
    data_processor::mutation::Mutation,
    interval::watch_dog::{LaunchedWatchDog, WatchDog},
};
use anyhow::{Context, Ok};
use decoder::DataDecoder;
use futures_util::{Future, StreamExt};
use sea_orm::{DatabaseConnection, Set};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio_util::udp::UdpFramed;

pub mod decoder;

pub struct UdpDataProcessorBeforeLaunch {
    data_transaction: entities::data_transaction::Model,
    data_hub: LaunchedDataHub,
    watch_dog: WatchDog,
    udp_addr: String,
    #[allow(unused)]
    registration_keeper: DataProcessorRegistrationKeeper,
}

pub struct UdpDataProcessor {
    id: u32,
    data_hub: LaunchedDataHub,
    watch_dog: LaunchedWatchDog,
    incoming_framed: UdpFramed<DataDecoder>,
}

impl UdpDataProcessor {
    pub async fn launch_inline(
        db_coon: DatabaseConnection,
        data_hub: LaunchedDataHub,
        data_hub_controller: LaunchedDataHubController,
        udp_addr: String,
    ) -> impl Future<Output = Result<(), anyhow::Error>> {
        let data_transaction = Mutation(db_coon.clone())
            .insert_data_transaction(ActiveModel {
                start_time: Set(chrono::Local::now().naive_local()),
                ..Default::default()
            })
            .await
            .unwrap();
        let id = data_transaction.id as u32;

        UdpDataProcessorBeforeLaunch {
            data_transaction,
            data_hub,
            watch_dog: WatchDog::new(
                Duration::from_secs(60 * 60 * 24 * 365),
                Duration::from_secs(1000),
            ),
            udp_addr,
            registration_keeper: DataProcessorRegistrationKeeper::new(id, data_hub_controller)
                .unwrap(),
        }
        .task()
    }
}

impl UdpDataProcessorBeforeLaunch {
    pub async fn task(self) -> Result<(), anyhow::Error> {
        let (watch_dog, watch_dog_fut) = self.watch_dog.launch_inline();

        let socket = UdpSocket::bind(&self.udp_addr).await.expect(
            format!(
                "failed to bind udp socket to the address {}",
                &self.udp_addr
            )
            .as_str(),
        );

        let framed = UdpFramed::new(socket, DataDecoder);

        let id = self.data_transaction.id as u32;
        let processor = UdpDataProcessor {
            id,
            data_hub: self.data_hub,
            watch_dog,
            incoming_framed: framed,
        };

        let processor_fut = processor.task();

        tokio::select! {
            end = processor_fut => {
                log::debug!(udp_data_processor_id:serde = id, udp_addr:serde = self.udp_addr; "The udp socket is closed.");
                end
            }
            _ = watch_dog_fut => {
                log::debug!(udp_data_processor_id:serde = id, udp_addr:serde = self.udp_addr; "The timeout has been reached.");
                Err(anyhow::anyhow!("timeout"))
            }
        }
    }
}

impl UdpDataProcessor {
    async fn task(mut self) -> anyhow::Result<()> {
        while let Some(result) = self.incoming_framed.next().await {
            let Result::Ok((data, addr)) = result else {
                return Err(result
                    .unwrap_err()
                    .context("failed to decode the udp packet"));
            };

            log::trace!(data:serde, addr:?; "New data is received.");

            self.watch_dog.do_notify_alive().await.unwrap();

            self.data_hub
                .new_data_from_processor(self.id, data)
                .await
                .context("failed to send data to the data hub")?;
        }

        Ok(())
    }
}

impl Drop for UdpDataProcessor {
    fn drop(&mut self) {
        log::debug!(id:? = self.id; "A udp data processor is dropped")
    }
}
