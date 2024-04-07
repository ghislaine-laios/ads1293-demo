use self::actions::{Started, Stopping};
pub use self::ProcessorAfterLaunched as Processor;
use super::{
    context::WebsocketContext, fut_into_output_stream, subtask::Subtask,
    websocket_handler::WebsocketHandler, FeedRawDataError,
};
use crate::actors::{
    interval::watch_dog::{LaunchedWatchDog, Timeout, WatchDog},
    websocket::feed_raw_data,
    Handler,
};
use actix_web::web::{self, Bytes};
use futures::{Future, Stream, TryFutureExt};
use std::{fmt::Debug, time::Duration};
use tokio::{select, sync::mpsc};

#[derive(Debug, thiserror::Error)]
pub enum ProcessingError {
    #[error("failed to feed raw data")]
    FeedRawDataError(FeedRawDataError),

    #[error("the started hook of the process data handler ended with error")]
    StartProcessingDataFailed(Box<dyn Debug>),
    #[error("failed to process the data by the ws context")]
    ProcessDataFailed(super::context::ProcessingError),
    #[error("the stopping hook of the process data handler ended with error")]
    StopProcessingDataFailed(Box<dyn Debug>),
    #[error("the subtask ended with error")]
    SubtaskError(Box<dyn Debug>),
    #[error("an unknown internal bug occurred")]
    InternalBug(anyhow::Error),
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
    P: WebsocketHandler<Context = WebsocketContext>,
    S: Subtask,
{
    raw_data_input_stream: web::Payload,
    watch_dog: WatchDog,
    process_data_handler: P,
    subtask: S,
}

impl<P, S> ProcessorBeforeLaunched<P, S>
where
    P: WebsocketHandler<Context = WebsocketContext>,
    S: Subtask,
    <S as Subtask>::OutputError: 'static,
{
    pub fn launch_inline(
        self,
        ws_output_channel_size: Option<usize>,
    ) -> impl Stream<Item = Result<Bytes, ProcessingError>> {
        let (launched_watch_dog, timeout_fut) = self.watch_dog.launch_inline();

        let (ws_sender, ws_receiver) = mpsc::channel(ws_output_channel_size.unwrap_or(8));

        let processor = ProcessorAfterLaunched {
            watch_dog: launched_watch_dog,
            websocket_context: WebsocketContext::new(ws_sender),
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
    P: WebsocketHandler<Context = WebsocketContext>,
{
    watch_dog: LaunchedWatchDog,
    websocket_context: WebsocketContext,
    process_data_handler: P,
}

impl<P> ProcessorAfterLaunched<P>
where
    P: WebsocketHandler<Context = WebsocketContext>,
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
    ) -> Result<(), ProcessingError>
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
            .await
            .map_err(|e| ProcessingError::StartProcessingDataFailed(Box::new(e)))?;

        let error = {
            let process_incoming_raw = async {
                while let Some(bytes) = raw_incoming_rx.recv().await {
                    self.websocket_context
                        .handle_raw(bytes, &mut self.process_data_handler)
                        .await
                        .map_err(|e| ProcessingError::ProcessDataFailed(e))?;
                }

                Ok(())
            };
            actix_rt::pin!(process_incoming_raw);

            let mut error = None;
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
            .await
            .map_err(|e| ProcessingError::StopProcessingDataFailed(Box::new(e)))?;

        if let Some(e) = error {
            Err(e)
        } else {
            Ok(())
        }
    }
}

impl<P> Drop for ProcessorAfterLaunched<P>
where
    P: WebsocketHandler<Context = WebsocketContext>,
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
