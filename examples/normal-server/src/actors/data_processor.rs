use self::streams::DataComing;
use super::service_broadcast_manager::LaunchedServiceBroadcastManager;
use crate::actors::Handler;
use actix_http::ws::{self, ProtocolError};
use actix_web::{
    error::PayloadError,
    web::{self, Bytes, BytesMut},
};
use anyhow::Context;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::select;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::codec::Encoder;

#[derive(Debug, Deserialize, Serialize)]
pub struct Data {
    pub id: u32,
    pub value: u32,
}

#[derive(Clone, Copy, Debug)]
pub enum ConnectionStatus {
    Activated,
    SeverRequestClosing,
    PeerRequestClosing,
    Closed,
}

pub struct DataProcessor {
    buf: BytesMut,
    codec: ws::Codec,
    ws_sender: tokio::sync::mpsc::Sender<Result<Bytes, DataProcessingError>>,
    last_pong: tokio::time::Instant,
    status: ConnectionStatus,
    data_coming: Option<streams::DataComing<web::Payload>>,
    launched_service_broadcast_manager: LaunchedServiceBroadcastManager,
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
    #[error("an unknown internal bug occurred")]
    InternalBug(Arc<anyhow::Error>),
}

impl DataProcessor {
    pub fn new(
        payload: web::Payload,
        launched_service_broadcast_manager: LaunchedServiceBroadcastManager,
    ) -> (Self, impl Stream<Item = Result<Bytes, DataProcessingError>>) {
        let (tx, rx) = tokio::sync::mpsc::channel(8);

        (
            Self {
                buf: BytesMut::new(),
                codec: ws::Codec::new(),
                ws_sender: tx,
                last_pong: tokio::time::Instant::now(),
                status: ConnectionStatus::Activated,
                data_coming: Some(DataComing::new(payload)),
                launched_service_broadcast_manager,
            },
            ReceiverStream::new(rx),
        )
    }

    pub fn launch(self) -> tokio::task::JoinHandle<()> {
        actix_rt::spawn(async move {
            self.task().await;
        })
    }

    async fn task(mut self) {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<actions::Action>(1);

        let data_coming = self.data_coming.take();
        let Some(mut data_coming) = data_coming else {
            log::error!("Cannot take the websocket data source. This task may has run before.");
            return;
        };

        let mut err = self
            .launched_service_broadcast_manager
            .register_connection()
            .await
            .context("failed to register this connection to the manager due to closed channel")
            .map_err(|e| DataProcessingError::InternalBug(Arc::new(e)))
            .err();
        while err.is_none() {
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
                }
            }
        }

        let _ = self
            .launched_service_broadcast_manager
            .unregister_connection()
            .await
            .context("failed to unregister this connection to the manager due to closed channel");

        let Some(err) = err else {
            return;
        };

        let generic_err = if !matches!(
            err,
            DataProcessingError::SendToPeerError(SendToPeerError::ChannelClosed)
        ) {
            self.notify_err(err.clone())
                .await
                .map(|_| GenericDataProcessingError::Informed(err))
                .map_err(|e| DataProcessingError::InternalBug(Arc::new(e.into())))
                .map_err(|e| GenericDataProcessingError::Uninformed(e))
                .unwrap_or_else(|e| e)
        } else {
            GenericDataProcessingError::Uninformed(err)
        };

        if let GenericDataProcessingError::Uninformed(err) = generic_err {
            log::error!("encountered an informed error: {:#?}", err);
        }
    }

    async fn process_action(&mut self, action: actions::Action) -> Result<(), DataProcessingError> {
        log::debug!("action: {:?}", &action);

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

            match frame {
                actix_http::ws::Frame::Text(text) => {
                    let data = serde_json::from_slice(&text[..])
                        .map_err(|e| DataProcessingError::DataDecodeFailed(Arc::new(e)))?;
                    self.process_data(data).await;
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
                actix_http::ws::Frame::Pong(_) => {
                    self.last_pong = tokio::time::Instant::now();
                    Ok(())
                }
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
        // The status after the message was sent successfully
        let mut status = self.status;
        let make_result = || -> Result<Result<Bytes, DataProcessingError>, SendToPeerError> {
            if matches!(msg, ws::Message::Close(_)) {
                match self.status {
                    ConnectionStatus::Activated => {
                        status = ConnectionStatus::SeverRequestClosing;
                    }
                    ConnectionStatus::PeerRequestClosing => {
                        status = ConnectionStatus::Closed;
                    }
                    ConnectionStatus::SeverRequestClosing | ConnectionStatus::Closed => {
                        return Err(SendToPeerError::DuplicatedClose);
                    }
                }
            } else if matches!(
                self.status,
                ConnectionStatus::SeverRequestClosing | ConnectionStatus::Closed
            ) {
                return Err(SendToPeerError::SendMessageAfterClosed);
            }

            let mut buf = BytesMut::with_capacity(16);
            self.codec
                .encode(msg, &mut buf)
                .map_err(|e| SendToPeerError::EncodingFailed(Arc::new(e)))?;

            let buf = buf.freeze();
            Ok(Ok(buf))
        };

        let result = make_result()?;

        self.ws_sender
            .send(result)
            .await
            .map_err(|_| SendToPeerError::ChannelClosed)?;

        self.status = status;

        Ok(())
    }

    async fn notify_err(&self, err: DataProcessingError) -> Result<(), ErrorNotificationFailed> {
        self.ws_sender
            .send(Err(err))
            .await
            .map_err(|_| ErrorNotificationFailed)
    }

    async fn process_data(&mut self, data: Data) {
        log::info!("data: {:?}", data);
    }
}

#[derive(Clone)]
pub struct LaunchedDataProcessor {
    pub tx: tokio::sync::mpsc::Sender<actions::Action>,
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
            self.buf.extend_from_slice(&bytes[..]);

            while let Some(frame) = self
                .codec
                .decode(&mut self.buf)
                .map_err(|e| DataProcessingError::FrameDecodeFailed(Arc::new(e)))?
            {
                self.handle_frame(frame).await?
            }

            Ok(())
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
