use self::{
    interval::CheckAlive,
    mutation::Mutation,
    saver::{DataSaver, LaunchedDataSaver},
    streams::DataComing,
};
use super::service_broadcast_manager::LaunchedServiceBroadcastManager;
use crate::{
    actors::Handler,
    entities::{
        data,
        data_transaction::{self, ActiveModel},
    },
};
use actix_http::ws::{self, ProtocolError};
use actix_web::{
    error::PayloadError,
    web::{self, Bytes, BytesMut},
};
use anyhow::Context;
use async_stream::stream;
use chrono::Local;
use futures::Stream;
use sea_orm::{DatabaseConnection, DbErr, Set};
use std::{sync::Arc, time::Duration};
use tokio::{join, select, sync::mpsc};
use tokio_util::codec::Encoder;

pub use normal_data::Data;

mod mutation;
mod saver;

#[derive(Clone, Copy, Debug)]
pub enum ConnectionStatus {
    Activated,
    SeverRequestClosing,
    PeerRequestClosing,
    Closed,
}

pub struct DataProcessorBuilder {
    data_coming: streams::DataComing<web::Payload>,
    alive_checker: CheckAlive,
    data_saver: DataSaver,
    mutation: Mutation,
    launched_service_broadcast_manager: LaunchedServiceBroadcastManager,
}

impl DataProcessorBuilder {
    pub async fn launch(
        self,
    ) -> Result<impl Stream<Item = Result<Bytes, DataProcessingError>>, DbErr> {
        let data_transaction = self
            .mutation
            .insert_data_transaction(ActiveModel {
                start_time: Set(chrono::Local::now().naive_local()),
                ..Default::default()
            })
            .await?;

        let (launched_data_saver, _data_saver_join_handle) = self.data_saver.launch();

        let (tx, mut rx) = mpsc::channel(8);

        let data_processor = DataProcessor {
            decode_buf: BytesMut::new(),
            codec: ws::Codec::new(),
            ws_sender: tx,
            status: ConnectionStatus::Activated,
            launched_service_broadcast_manager: self.launched_service_broadcast_manager,
            last_pong: actix_rt::time::Instant::now(),
            data_transaction,
            launched_data_saver,
        };

        let mut data_processor_join_handle =
            data_processor.launch(self.data_coming, self.alive_checker);

        let ws_stream = stream! {
            loop {
                tokio::select! {
                    bytes = rx.recv() => {
                        match bytes {
                            Some(bytes) => yield(Ok(bytes)),
                            _ => break
                        }
                    },
                    join_result = &mut data_processor_join_handle => {
                        let task_result = match join_result {
                            Ok(r) => r,
                            Err(e) => {
                                log::error!("A data processor task panicked: {:#?}", e);
                                yield(Err(DataProcessingError::InternalBug(Arc::new(e.into()))));
                                break
                            }
                        };

                        if let Err(e) = task_result {
                            log::error!(
                                "A data processor task returned with error: {:#?}", e);
                                yield(Err(e))
                        }

                        break
                    }
                };
            }
        };

        Ok(ws_stream)
    }
}

pub struct DataProcessor {
    decode_buf: BytesMut,
    codec: ws::Codec,
    ws_sender: mpsc::Sender<Bytes>,
    status: ConnectionStatus,
    launched_service_broadcast_manager: LaunchedServiceBroadcastManager,
    last_pong: actix_rt::time::Instant,
    data_transaction: data_transaction::Model,
    launched_data_saver: LaunchedDataSaver,
}

#[derive(Debug, thiserror::Error)]
pub enum GenericDataProcessingError {
    #[error("an informed data processing error occurred")]
    Informed(DataProcessingError),
    #[error("an uninformed data processing error occurred")]
    Uninformed(DataProcessingError),
}

#[derive(Debug, thiserror::Error, Clone)]
pub enum DataProcessingError {
    #[error("failed to decode the incoming websocket frame")]
    FrameDecodeFailed(Arc<ProtocolError>),
    #[error("failed to decode the data from the text frame")]
    DataDecodeFailed(Arc<serde_json::Error>),
    #[error("failed to send message to the peer")]
    SendToPeerError(SendToPeerError),
    #[error("the incoming websocket frame is not supported")]
    NotSupportedFrame(String),
    #[error("the given payload throw an error")]
    PayloadError(Arc<PayloadError>),
    #[error("the data value {} is outranged", .0)]
    DataOutrange(u32),
    #[error("failed to save data using data saver")]
    SaveDataFailed,
    #[error("an unknown internal bug occurred")]
    InternalBug(Arc<anyhow::Error>),
}

impl DataProcessor {
    pub fn new(
        payload: web::Payload,
        launched_service_broadcast_manager: LaunchedServiceBroadcastManager,
        db_coon: DatabaseConnection,
    ) -> DataProcessorBuilder {
        DataProcessorBuilder {
            data_coming: DataComing::new(payload),
            alive_checker: CheckAlive::new(Duration::from_secs(15)),
            data_saver: DataSaver::new(db_coon.clone()),
            mutation: Mutation(db_coon),
            launched_service_broadcast_manager,
        }
    }

    fn launch(
        self,
        data_coming: DataComing<web::Payload>,
        alive_checker: interval::CheckAlive,
    ) -> tokio::task::JoinHandle<Result<(), DataProcessingError>> {
        actix_rt::spawn(async move { self.task(data_coming, alive_checker).await })
    }

    async fn task(
        mut self,
        mut data_coming: DataComing<web::Payload>,
        mut alive_checker: interval::CheckAlive,
    ) -> Result<(), DataProcessingError> {
        log::debug!("new data processor started");
        let (tx, mut rx) = tokio::sync::mpsc::channel::<actions::Action>(1);

        let mut err = self
            .launched_service_broadcast_manager
            .register_connection()
            .await
            .context("failed to register this connection to the manager due to closed channel")
            .map_err(|e| DataProcessingError::InternalBug(Arc::new(e)))
            .err();

        while err.is_none() {
            alive_checker.update_last_pong(self.last_pong);

            select! {
                action = rx.recv() => {
                    let Some(action) = action else {break;};
                    match self.process_action(action).await {
                        Ok(_)=>{},
                        Err(e) => {
                            err = Some(e);
                        }
                    }
                },
                action = data_coming.next() => {
                    let Some(action) = action else {break;};
                    match action {
                        Ok(act) => {
                            tx.send(act).await.unwrap();
                        },
                        Err(e) => {
                            err = Some(DataProcessingError::PayloadError(Arc::new(e)));
                        },
                    }
                },
                alive = alive_checker.tick() => {
                    if !alive {break;}
                }
            }
        }

        let e = self
            .launched_service_broadcast_manager
            .unregister_connection()
            .await
            .context("failed to unregister this connection to the manager due to closed channel");
        if e.is_err() {
            log::error!("{:?}", e);
        }

        if let Some(err) = err {
            return Err(err);
        }

        join!(async {1}, async {2});

        Ok(())
    }

    async fn process_action(&mut self, action: actions::Action) -> Result<(), DataProcessingError> {
        log::trace!("action: {:?}", &action);

        let add_raw = match action {
            actions::Action::AddRaw(add_raw) => add_raw,
        };

        self.handle(add_raw).await
    }
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum SendToPeerError {
    #[error("to-peer channel closed")]
    ChannelClosed,
    #[error("the close frame is going to be sent twice")]
    DuplicatedClose,
    #[error("cannot send message after the close frame has been sent")]
    SendMessageAfterClosed,
    #[error("the given message cannot be encoded into bytes")]
    EncodingFailed(Arc<ProtocolError>),
}

#[derive(thiserror::Error, Debug, Clone)]
#[error("failed to send error through to-peer channel due to its closure")]
pub struct ErrorNotificationFailed;

impl DataProcessor {
    async fn handle_frame(&mut self, frame: ws::Frame) -> Result<(), DataProcessingError> {
        log::trace!("frame: {:?}", frame);
        async {
            match self.status {
                ConnectionStatus::Activated => {}
                ConnectionStatus::SeverRequestClosing => {
                    // After the server sends the close frame,
                    // the server can still process remaining frames
                    // sent by the peer.
                }
                ConnectionStatus::PeerRequestClosing => {
                    // After the peer sends the close frame,
                    // anything sent by the peer will be ignored.
                    return Ok(());
                }
                ConnectionStatus::Closed => {
                    // Both ends have reached consensus;
                    // anything sent after that will be ignored.
                    return Ok(());
                }
            }

            self.last_pong = tokio::time::Instant::now();

            match frame {
                actix_http::ws::Frame::Text(text) => {
                    let data = serde_json::from_slice(&text[..])
                        .map_err(|e| DataProcessingError::DataDecodeFailed(Arc::new(e)))?;
                    self.process_data(data).await?;
                    Ok(())
                }
                actix_http::ws::Frame::Binary(_) => {
                    Err(DataProcessingError::NotSupportedFrame("binary".to_string()))
                }
                actix_http::ws::Frame::Continuation(_) => Err(
                    DataProcessingError::NotSupportedFrame("continuation".to_string()),
                ),
                actix_http::ws::Frame::Ping(msg) => self
                    .send_to_peer(ws::Message::Pong(msg))
                    .await
                    .map_err(DataProcessingError::SendToPeerError),
                actix_http::ws::Frame::Pong(_) => Ok(()),
                actix_http::ws::Frame::Close(_) => {
                    let r = match self.status {
                        ConnectionStatus::Activated => {
                            self.status = ConnectionStatus::PeerRequestClosing;
                            self.send_to_peer(ws::Message::Close(None)).await
                        }
                        ConnectionStatus::SeverRequestClosing => {
                            self.status = ConnectionStatus::Closed;
                            Ok(())
                        }

                        ConnectionStatus::PeerRequestClosing | ConnectionStatus::Closed => {
                            unreachable!()
                        }
                    };

                    r.map_err(DataProcessingError::SendToPeerError)
                }
            }
        }
        .await
    }

    async fn send_to_peer(&mut self, msg: ws::Message) -> Result<(), SendToPeerError> {
        let status = if matches!(msg, ws::Message::Close(_)) {
            match self.status {
                ConnectionStatus::Activated => ConnectionStatus::SeverRequestClosing,
                ConnectionStatus::PeerRequestClosing => ConnectionStatus::Closed,
                ConnectionStatus::SeverRequestClosing | ConnectionStatus::Closed => {
                    return Err(SendToPeerError::DuplicatedClose)
                }
            }
        } else if matches!(
            self.status,
            ConnectionStatus::SeverRequestClosing | ConnectionStatus::Closed
        ) {
            return Err(SendToPeerError::SendMessageAfterClosed);
        } else {
            self.status
        };

        let mut buf = BytesMut::with_capacity(32);

        self.codec
            .encode(msg, &mut buf)
            .map_err(|e| SendToPeerError::EncodingFailed(Arc::new(e)))?;

        self.ws_sender
            .send(buf.freeze())
            .await
            .map_err(|_| SendToPeerError::ChannelClosed)?;

        self.status = status;

        Ok(())
    }
}

impl DataProcessor {
    async fn process_data(&mut self, data: Data) -> Result<(), DataProcessingError> {
        log::trace!("data: {:?}", data);

        self.launched_data_saver
            .save_timeout(
                data::ActiveModel {
                    data_transaction_id: Set(self.data_transaction.id),
                    id: Set(data.id.into()),
                    value: Set(data
                        .value
                        .try_into()
                        .map_err(|_| DataProcessingError::DataOutrange(data.value))?),
                },
                Duration::from_millis(200),
            )
            .await
            .map_err(|_| DataProcessingError::SaveDataFailed)?;

        Ok(())
    }
}

#[derive(Clone)]
pub struct LaunchedDataProcessor {
    pub tx: tokio::sync::mpsc::Sender<actions::Action>,
}

impl Drop for DataProcessor {
    fn drop(&mut self) {
        log::debug!("A data processor is dropped at {:?}.", Local::now());
    }
}

pub(super) mod actions {

    use actix_web::web::Bytes;

    #[derive(Debug)]
    pub enum Action {
        AddRaw(AddRaw),
    }

    #[derive(Debug)]
    pub struct AddRaw(pub Bytes);
}

pub mod handlers {
    use super::{actions, DataProcessor};
    use crate::actors::data_processor::DataProcessingError;
    use crate::actors::Handler;
    use std::sync::Arc;

    use tokio_util::codec::Decoder;

    impl Handler<actions::AddRaw> for DataProcessor {
        type Output = Result<(), DataProcessingError>;

        async fn handle(&mut self, action: actions::AddRaw) -> Self::Output {
            let actions::AddRaw(bytes) = action;
            self.decode_buf.extend_from_slice(&bytes[..]);

            while let Some(frame) = self
                .codec
                .decode(&mut self.decode_buf)
                .map_err(|e| DataProcessingError::FrameDecodeFailed(Arc::new(e)))?
            {
                self.handle_frame(frame).await?
            }

            Ok(())
        }
    }
}

pub(super) mod interval {
    use std::time::Duration;

    pub struct CheckAlive {
        timeout: Duration,
        last_pong: tokio::time::Instant,
        ticker: tokio::time::Interval,
    }

    impl CheckAlive {
        pub fn new(timeout: Duration) -> Self {
            Self {
                timeout,
                last_pong: tokio::time::Instant::now(),
                ticker: tokio::time::interval(Duration::from_secs(1)),
            }
        }

        pub async fn tick(&mut self) -> bool {
            let instant = self.ticker.tick().await;

            let last_pong = self.last_pong.clone();
            let duration = instant.duration_since(last_pong);
            log::debug!("check alive duration: {:?}", duration);

            duration <= self.timeout
        }

        pub fn update_last_pong(&mut self, pong: tokio::time::Instant) {
            self.last_pong = pong
        }
    }
}

pub(super) mod streams {
    use super::actions::Action;
    use crate::actors::data_processor::actions::AddRaw;

    use actix_web::{error::PayloadError, web::Bytes};

    use futures::{Stream, StreamExt};
    use pin_project::pin_project;

    #[pin_project]
    #[derive(Debug)]
    pub struct DataComing<S: Stream<Item = Result<Bytes, PayloadError>> + Unpin>(#[pin] S);

    impl<S: Stream<Item = Result<Bytes, PayloadError>> + Unpin> DataComing<S> {
        pub fn new(stream: S) -> Self {
            Self(stream)
        }

        pub async fn next(&mut self) -> Option<Result<Action, PayloadError>> {
            let stream = &mut self.0;

            let Some(bytes) = stream.next().await else {
                return None;
            };

            let bytes = match bytes {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };

            Some(Ok(Action::AddRaw(AddRaw(bytes))))
        }
    }
}
