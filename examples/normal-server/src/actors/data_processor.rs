use std::sync::Arc;

use actix_http::{
    ws::{self, ProtocolError},
    Payload,
};
use actix_web::{
    error::PayloadError,
    web::{self, Bytes, BytesMut},
};
use futures::Stream;
use serde::{Deserialize, Serialize};
use tokio::select;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::codec::Encoder;

use crate::actors::Handler;

use self::{actions::Action, streams::DataComing};

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
    #[error("an internal bug occurred")]
    InternalBug(Arc<anyhow::Error>),
    #[error("the given payload throw an error")]
    PayloadError(Arc<PayloadError>),
}

impl DataProcessor {
    pub fn new(
        payload: web::Payload,
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
            },
            ReceiverStream::new(rx),
        )
    }

    pub fn launch(self) -> tokio::task::JoinHandle<()> {
        actix_rt::spawn(async move { self.task().await })
    }

    async fn task(mut self) {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<actions::Action>(1);

        let data_coming = self.data_coming.take();
        let Some(mut data_coming) = data_coming else {
            log::error!("Cannot take the websocket data source. This task may has run before.");
            return;
        };

        let mut err = None;
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
        let result = async {
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
        .await;

        // if let Err(err) = result {
        //     let generic_err = if !matches!(
        //         err,
        //         DataProcessingError::SendToPeerError(SendToPeerError::ChannelClosed)
        //     ) {
        //         self.notify_err(err.clone())
        //             .await
        //             .map_err(|e| DataProcessingError::InternalBug(Arc::new(e.into())))
        //             .map_err(|e| GenericDataProcessingError::Uninformed(e))?;
        //         GenericDataProcessingError::Informed(err)
        //     } else {
        //         GenericDataProcessingError::Uninformed(err)
        //     };

        //     Err(generic_err)
        // } else {
        //     Ok(())
        // }

        result
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
    use super::{
        actions, ConnectionStatus, DataProcessor, GenericDataProcessingError, SendToPeerError,
    };
    use crate::actors::data_processor::DataProcessingError;
    use crate::actors::Handler;
    use std::sync::Arc;

    use actix_http::ws;

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

#[cfg(test)]
mod tests {
    use std::time::{self, Duration};

    use actix_web::{
        get, middleware::Logger, post, web, App, Error, HttpRequest, HttpResponse, HttpServer,
    };
    use anyhow::Context;
    use futures::SinkExt;
    use tokio::join;
    use tokio_tungstenite::connect_async;
    use url::Url;

    use crate::{actors::data_processor::DataProcessor, settings::Settings};

    #[get("/")]
    async fn start_connection(
        req: HttpRequest,
        stream: web::Payload,
    ) -> Result<HttpResponse, Error> {
        let mut res = actix_web_actors::ws::handshake(&req)?;
        let (data_processor, stream) = DataProcessor::new(stream);
        let _ = data_processor.launch();
        Ok(res.streaming(stream))
    }

    #[actix_web::test]
    async fn it_works() {
        use super::Data;
        use tokio_tungstenite::tungstenite::Message::*;

        crate::tests_utils::setup_logger();
        let settings = Settings::new().unwrap();

        let bind_to = settings.bind_to.clone();
        let bind_to = (bind_to.ip.as_str(), bind_to.port);

        let fut =
            HttpServer::new(move || App::new().wrap(Logger::default()).service(start_connection))
                .bind(bind_to)
                .context(format!("failed to bind to {}:{}", bind_to.0, bind_to.1))
                .unwrap()
                .run();

        let main_work = actix_rt::spawn(fut);
        actix_rt::time::sleep(Duration::from_millis(500)).await;

        let (mut socket, resp) = connect_async(
            Url::parse(format!("ws://localhost:{}/", settings.bind_to.port).as_str()).unwrap(),
        )
        .await
        .unwrap();

        dbg!(resp);

        let total_num: u32 = 3000;
        let mut data_arr = Vec::with_capacity(total_num.try_into().unwrap());
        for i in 1..total_num {
            data_arr.push(Data {
                id: i,
                value: i * 2,
            })
        }

        for data in data_arr {
            socket
                .feed(Text(serde_json::to_string(&data).unwrap()))
                .await
                .unwrap();
            socket.flush().await.unwrap();
        }

        actix_rt::time::sleep(Duration::from_millis(500)).await;
    }
}
