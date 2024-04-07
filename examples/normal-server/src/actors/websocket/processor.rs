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
use std::{fmt::Debug, time::Duration};
use tokio::{select, sync::mpsc};
use tokio_util::codec::Encoder;

pub trait WebsocketHandler:
    Handler<Started, Output = Result<(), Self::StartedError>>
    + Handler<Data, Output = Result<(), Self::ProcessDataError>>
    + Handler<Stopping, Output = Result<(), Self::StoppingError>>
    + Debug
where
    Self::StartedError: Debug,
    Self::ProcessDataError: Debug,
    Self::StoppingError: Debug,
{
    type StartedError;
    type ProcessDataError;
    type StoppingError;
}

impl<T, TS, TP, TST> WebsocketHandler for T
where
    T: Handler<Started, Output = Result<(), TS>>
        + Handler<Data, Output = Result<(), TP>>
        + Handler<Stopping, Output = Result<(), TST>>
        + Debug,
    TS: Debug,
    TP: Debug,
    TST: Debug,
{
    type StartedError = TS;

    type ProcessDataError = TP;

    type StoppingError = TST;
}

pub trait Subtask
where
    Self::OutputError: Debug + 'static,
{
    type OutputError;

    #[allow(async_fn_in_trait)]
    async fn task(self) -> Result<(), Self::OutputError>;
}

impl<T, E> Subtask for T
where
    T: Future<Output = Result<(), E>>,
    E: Debug + std::error::Error + 'static,
{
    type OutputError = E;
    fn task(self) -> impl Future<Output = Result<(), Self::OutputError>> {
        self
    }
}

pub struct NoSubtask;

impl Subtask for NoSubtask {
    type OutputError = ();

    async fn task(self) -> Result<(), Self::OutputError> {
        loop {
            actix_rt::time::sleep(Duration::from_secs(60 * 60 * 24)).await;
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ConnectionStatus {
    Activated,
    SeverRequestClosing,
    PeerRequestClosing,
    Closed,
}

#[derive(Debug, thiserror::Error)]
pub enum ProcessingError<P: WebsocketHandler> {
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
    StartProcessingDataFailed(P::StartedError),
    #[error("failed to process the data")]
    ProcessDataFailed(P::ProcessDataError),
    #[error("the stopping hook of the process data handler ended with error")]
    StopProcessingDataFailed(P::StoppingError),
    #[error("the subtask ended with error")]
    SubtaskError(Box<dyn Debug>),
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
pub struct ProcessorMeta<P: WebsocketHandler, S: Subtask> {
    pub process_data_handler: P,
    pub subtask: S,
    pub watch_dog_timeout_seconds: u64,
    pub watch_dog_check_interval_seconds: u64,
}

// TODO: simplify the generic parameters
pub struct ProcessorBeforeLaunched<P, S>
where
    P: WebsocketHandler,
    S: Subtask,
{
    raw_data_input_stream: web::Payload,
    watch_dog: WatchDog,
    process_data_handler: P,
    subtask: S,
}

impl<P, S> ProcessorBeforeLaunched<P, S>
where
    P: WebsocketHandler,
    S: Subtask,
    <S as Subtask>::OutputError: 'static,
{
    pub fn launch_inline(
        self,
        ws_output_channel_size: Option<usize>,
    ) -> impl Stream<Item = Result<Bytes, ProcessingError<P>>> {
        let (launched_watch_dog, timeout_fut) = self.watch_dog.launch_inline();

        let (ws_sender, ws_receiver) = mpsc::channel(ws_output_channel_size.unwrap_or(8));

        let processor = ProcessorAfterLaunched {
            watch_dog: launched_watch_dog,
            decode_buf: BytesMut::new(),
            codec: ws::Codec::new(),
            status: ConnectionStatus::Activated,
            ws_sender,
            process_data_handler: self.process_data_handler,
        };

        let fut = processor.task(self.raw_data_input_stream, timeout_fut, self.subtask);

        fut_into_output_stream(
            ws_receiver,
            fut,
            Some(|e| {
                log::error!("the processor ended with error: {:?}", e);
                e
            }),
        )
    }
}

pub struct ProcessorAfterLaunched<P>
where
    P: WebsocketHandler,
{
    watch_dog: LaunchedWatchDog,
    decode_buf: BytesMut,
    codec: ws::Codec,
    status: ConnectionStatus,
    ws_sender: mpsc::Sender<Bytes>,
    process_data_handler: P,
}

impl<P> ProcessorAfterLaunched<P>
where
    P: WebsocketHandler,
{
    pub fn new<S>(payload: web::Payload, meta: ProcessorMeta<P, S>) -> ProcessorBeforeLaunched<P, S>
    where
        S: Subtask,
    {
        ProcessorBeforeLaunched {
            raw_data_input_stream: payload,
            watch_dog: WatchDog::new(
                Duration::from_secs(meta.watch_dog_timeout_seconds),
                Duration::from_secs(meta.watch_dog_check_interval_seconds),
            ),
            process_data_handler: meta.process_data_handler,
            subtask: meta.subtask,
        }
    }

    pub async fn task<S>(
        mut self,
        raw_data_stream: web::Payload,
        watch_dog: impl Future<Output = Option<Timeout>>,
        sub_task: S,
    ) -> Result<(), ProcessingError<P>>
    where
        S: Subtask,
    {
        log::debug!("new websocket processor (actor) started");
        let (raw_incoming_tx, mut raw_incoming_rx) = mpsc::channel::<Bytes>(1);

        let feed_raw_data = feed_raw_data(raw_data_stream, raw_incoming_tx, |bytes| bytes);

        actix_rt::pin!(feed_raw_data);
        actix_rt::pin!(watch_dog);

        let sub_task = sub_task.task();
        actix_rt::pin!(sub_task);

        self.process_data_handler
            .handle(Started)
            .map_err(ProcessingError::StartProcessingDataFailed)
            .await?;

        let error = {
            let process_incoming_raw = async {
                while let Some(bytes) = raw_incoming_rx.recv().await {
                    self.handle(bytes).await?;
                }

                Ok::<(), ProcessingError<P>>(())
            };
            actix_rt::pin!(process_incoming_raw);

            let mut error: Option<ProcessingError<P>> = None;
            while error.is_none() {
                select! {
                    biased;

                    process_end = &mut process_incoming_raw => {
                        error = process_end.err();
                        break
                    },
                    feed_end = &mut feed_raw_data => {
                        error = feed_end.map_err(ProcessingError::FeedRawDataError).err();
                        break
                    },
                    sub_task_end = &mut sub_task => {
                        error = sub_task_end.map_err(
                            |e| ProcessingError::SubtaskError(Box::new(e))
                        ).err();
                        break
                    },
                    _ = &mut watch_dog => break,
                }
            }
            error
        };

        self.process_data_handler
            .handle(Stopping)
            .map_err(ProcessingError::StopProcessingDataFailed)
            .await?;

        if let Some(e) = error {
            Err(e)
        } else {
            Ok(())
        }
    }
}

pub(super) mod handlers {

    use super::ProcessingError;
    use super::ProcessorAfterLaunched;

    use super::WebsocketHandler;
    use crate::actors::Handler;

    use actix_web::web::Bytes;
    use tokio_util::codec::Decoder;

    impl<P> Handler<Bytes> for ProcessorAfterLaunched<P>
    where
        P: WebsocketHandler,
    {
        type Output = Result<(), ProcessingError<P>>;

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

impl<P> ProcessorAfterLaunched<P>
where
    P: WebsocketHandler,
{
    async fn handle_frame(&mut self, frame: ws::Frame) -> Result<(), ProcessingError<P>> {
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

    async fn process_data(&mut self, data: Data) -> Result<(), ProcessingError<P>> {
        log::trace!("data: {:?}", data);
        self.process_data_handler
            .handle(data)
            .await
            .map_err(ProcessingError::ProcessDataFailed)
    }
}

impl<P> Drop for ProcessorAfterLaunched<P>
where
    P: WebsocketHandler,
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
