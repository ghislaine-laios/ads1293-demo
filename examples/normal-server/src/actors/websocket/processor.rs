use self::actions::{Started, Stopping};
pub use self::ProcessorAfterLaunched as Processor;
use super::{fut_into_output_stream, FeedRawDataError};
use crate::actors::{
    interval::watch_dog::{LaunchedWatchDog, Timeout, WatchDog},
    websocket::feed_raw_data,
    Handler,
};
use actix_http::ws::{self, ProtocolError};
use actix_web::web::{self, Bytes, BytesMut};
use futures::{Future, Stream, TryFutureExt};
use normal_data::Data;
use std::time::Duration;
use tokio::{select, sync::mpsc};
use tokio_util::codec::Encoder;

#[derive(Clone, Copy, Debug)]
pub enum ConnectionStatus {
    Activated,
    SeverRequestClosing,
    PeerRequestClosing,
    Closed,
}

#[derive(Debug, thiserror::Error)]
pub enum ProcessingError<PSE, PE, PSTE> {
    #[error("failed to feed raw data")]
    FeedRawDataError(FeedRawDataError),
    #[error("failed to decode the incoming websocket frame")]
    FrameDecodeFailed(ProtocolError),
    #[error("failed to decode the data from the text frame")]
    DataDecodeFailed(serde_json::Error),
    #[error("failed to send message to the peer")]
    SendToPeerError(SendToPeerError),
    #[error("the incoming websocket frame is not supported")]
    NotSupportedFrame(String),
    #[error("the started hook of the process data handler ended with error")]
    StartProcessingDataFailed(PSE),
    #[error("failed to process the data")]
    ProcessDataFailed(PE),
    #[error("the stopping hook of the process data handler ended with error")]
    StopProcessingDataFailed(PSTE),
    #[error("an unknown internal bug occurred")]
    InternalBug(anyhow::Error),
}

#[derive(thiserror::Error, Debug)]
pub enum SendToPeerError {
    #[error("to-peer channel closed")]
    ChannelClosed,
    #[error("the close frame is going to be sent twice")]
    DuplicatedClose,
    #[error("cannot send message after the close frame has been sent")]
    SendMessageAfterClosed,
    #[error("the given message cannot be encoded into bytes")]
    EncodingFailed(ws::ProtocolError),
}

// #[derive(Builder)]
pub struct ProcessorMeta<
    const WATCH_DOG_TIMEOUT_SECONDS: u64,
    const WATCH_DOG_CHECK_INTERVAL_SECONDS: u64,
    P: Handler<Data, Output = Result<(), PE>>
        + Handler<Started, Output = Result<(), PSE>>
        + Handler<Stopping, Output = Result<(), PSTE>>,
    PSE,
    PE,
    PSTE,
> {
    pub process_data_handler: P,
}

pub struct ProcessorBeforeLaunched<P, PSE, PE, PSTE>
where
    P: Handler<Data, Output = Result<(), PE>>
        + Handler<Started, Output = Result<(), PSE>>
        + Handler<Stopping, Output = Result<(), PSTE>>,
{
    raw_data_input_stream: web::Payload,
    watch_dog: WatchDog,
    process_data_handler: P,
}

impl<P, PSE, PE, PSTE> ProcessorBeforeLaunched<P, PSE, PE, PSTE>
where
    P: Handler<Data, Output = Result<(), PE>>
        + Handler<Started, Output = Result<(), PSE>>
        + Handler<Stopping, Output = Result<(), PSTE>>,
{
    pub fn launch_inline(
        self,
    ) -> impl Stream<Item = Result<Bytes, ProcessingError<PSE, PE, PSTE>>> {
        let (launched_watch_dog, timeout_fut) = self.watch_dog.launch_inline();

        let (ws_sender, ws_receiver) = mpsc::channel(8);

        let processor = ProcessorAfterLaunched {
            watch_dog: launched_watch_dog,
            decode_buf: BytesMut::new(),
            codec: ws::Codec::new(),
            status: ConnectionStatus::Activated,
            ws_sender,
            process_data_handler: self.process_data_handler,
        };

        let fut = processor.task(self.raw_data_input_stream, timeout_fut);

        fut_into_output_stream(
            ws_receiver,
            fut,
            Some(|e| {
                log::error!("");
                e
            }),
        )
    }
}

pub struct ProcessorAfterLaunched<P, PSE, PE, PSTE>
where
    P: Handler<Data, Output = Result<(), PE>>
        + Handler<Started, Output = Result<(), PSE>>
        + Handler<Stopping, Output = Result<(), PSTE>>,
{
    watch_dog: LaunchedWatchDog,
    decode_buf: BytesMut,
    codec: ws::Codec,
    status: ConnectionStatus,
    ws_sender: mpsc::Sender<Bytes>,
    process_data_handler: P,
}

impl<P, PSE, PE, PSTE> ProcessorAfterLaunched<P, PSE, PE, PSTE>
where
    P: Handler<Data, Output = Result<(), PE>>
        + Handler<Started, Output = Result<(), PSE>>
        + Handler<Stopping, Output = Result<(), PSTE>>,
{
    pub fn new<
        const WATCH_DOG_TIMEOUT_SECONDS: u64,
        const WATCH_DOG_CHECK_INTERVAL_SECONDS: u64,
    >(
        payload: web::Payload,
        meta: ProcessorMeta<
            WATCH_DOG_CHECK_INTERVAL_SECONDS,
            WATCH_DOG_CHECK_INTERVAL_SECONDS,
            P,
            PSE,
            PE,
            PSTE,
        >,
    ) -> ProcessorBeforeLaunched<P, PSE, PE, PSTE> {
        ProcessorBeforeLaunched {
            raw_data_input_stream: payload,
            watch_dog: WatchDog::new(
                Duration::from_secs(WATCH_DOG_TIMEOUT_SECONDS),
                Duration::from_secs(WATCH_DOG_CHECK_INTERVAL_SECONDS),
            ),
            process_data_handler: meta.process_data_handler,
        }
    }

    pub async fn task(
        mut self,
        raw_data_stream: web::Payload,
        watch_dog: impl Future<Output = Option<Timeout>>,
    ) -> Result<(), ProcessingError<PSE, PE, PSTE>> {
        log::debug!("new websocket processor (actor) started");
        let (raw_incoming_tx, mut raw_incoming_rx) = mpsc::channel::<Bytes>(1);

        let feed_raw_data = feed_raw_data(raw_data_stream, raw_incoming_tx, |bytes| bytes);

        actix_rt::pin!(feed_raw_data);
        actix_rt::pin!(watch_dog);

        self.process_data_handler
            .handle(Started)
            .map_err(ProcessingError::StartProcessingDataFailed)
            .await?;

        let mut error = None;
        while error.is_none() {
            select! {
                biased;

                bytes = raw_incoming_rx.recv() => {
                    let Some(bytes) = bytes else {break};
                    error = self.handle(bytes).await.err();

                },
                feed_end = &mut feed_raw_data => {
                    error = feed_end.map_err(ProcessingError::FeedRawDataError).err();
                },
                _ = &mut watch_dog => break,
            }
        }

        self.process_data_handler
            .handle(Stopping)
            .map_err(ProcessingError::StopProcessingDataFailed)
            .await
    }
}

pub(super) mod handlers {
    use super::actions::Started;
    use super::actions::Stopping;
    use super::Data;
    use super::ProcessingError;
    use super::ProcessorAfterLaunched;
    use crate::actors::Handler;

    use actix_web::web::Bytes;
    use tokio_util::codec::Decoder;

    impl<P, PSE, PE, PSTE> Handler<Bytes> for ProcessorAfterLaunched<P, PSE, PE, PSTE>
    where
        P: Handler<Data, Output = Result<(), PE>>
            + Handler<Started, Output = Result<(), PSE>>
            + Handler<Stopping, Output = Result<(), PSTE>>,
    {
        type Output = Result<(), ProcessingError<PSE, PE, PSTE>>;

        async fn handle(&mut self, bytes: Bytes) -> Self::Output {
            let _ = self.watch_dog.do_notify_alive().await.map_err(|e| {
                log::warn!("cannot notify the watch dog. Error: {:?}", e);
            });

            self.decode_buf.extend_from_slice(&bytes[..]);

            while let Some(frame) = self
                .codec
                .decode(&mut self.decode_buf)
                .map_err(ProcessingError::FrameDecodeFailed)?
            {
                self.handle_frame(frame).await?
            }

            Ok(())
        }
    }
}

impl<P, PSE, PE, PSTE> ProcessorAfterLaunched<P, PSE, PE, PSTE>
where
    P: Handler<Data, Output = Result<(), PE>>
        + Handler<Started, Output = Result<(), PSE>>
        + Handler<Stopping, Output = Result<(), PSTE>>,
{
    async fn handle_frame(
        &mut self,
        frame: ws::Frame,
    ) -> Result<(), ProcessingError<PSE, PE, PSTE>> {
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

            match frame {
                actix_http::ws::Frame::Text(text) => {
                    let data = serde_json::from_slice(&text[..])
                        .map_err(ProcessingError::DataDecodeFailed)?;
                    self.process_data(data).await?;
                    Ok(())
                }
                actix_http::ws::Frame::Binary(_) => {
                    Err(ProcessingError::NotSupportedFrame("binary".to_string()))
                }
                actix_http::ws::Frame::Continuation(_) => Err(ProcessingError::NotSupportedFrame(
                    "continuation".to_string(),
                )),
                actix_http::ws::Frame::Ping(msg) => self
                    .send_to_peer(ws::Message::Pong(msg))
                    .await
                    .map_err(ProcessingError::SendToPeerError),
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

                    r.map_err(ProcessingError::SendToPeerError)
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
            .map_err(SendToPeerError::EncodingFailed)?;

        self.ws_sender
            .send(buf.freeze())
            .await
            .map_err(|_| SendToPeerError::ChannelClosed)?;

        self.status = status;

        Ok(())
    }

    async fn process_data(&mut self, data: Data) -> Result<(), ProcessingError<PSE, PE, PSTE>> {
        log::trace!("data: {:?}", data);
        self.process_data_handler
            .handle(data)
            .await
            .map_err(ProcessingError::ProcessDataFailed)
    }
}

impl<P, PSE, PE, PSTE> Drop for ProcessorAfterLaunched<P, PSE, PE, PSTE>
where
    P: Handler<Data, Output = Result<(), PE>>
        + Handler<Started, Output = Result<(), PSE>>
        + Handler<Stopping, Output = Result<(), PSTE>>,
{
    fn drop(&mut self) {
        log::debug!("A websocket processor is dropped")
    }
}

pub mod actions {
    #[derive(Debug)]
    pub struct Started;

    #[derive(Debug)]
    pub struct Stopping;
}
