use super::websocket_context::{WebsocketContext, WebsocketDataProcessingError};
use super::FeedRawDataError;
use crate::actors::{
    interval::watch_dog::WatchDog,
    websocket::{
        actor_context::process_incoming_raw::process_incoming_raw, feed_raw_data,
        fut_into_output_stream,
    },
};
use actix_web::web::{self, Bytes};
use futures::{Future, Stream};
use serde::Serialize;
use std::{any::type_name, pin::Pin, time::Duration};
use tokio::sync::mpsc;

mod process_incoming_raw;

pub enum EventLoopInstruction {
    Continue,
    Break,
}

#[derive(Debug)]
pub struct NoAction;

#[allow(async_fn_in_trait)]
pub trait WebsocketActorContextHandler {
    type Action;

    async fn started(&mut self, context: &mut WebsocketContext) -> anyhow::Result<()>;

    // When the handler was dropped this hook would not be called.
    // Use `Drop` to release resources.
    async fn stopped(
        &mut self,
        #[allow(unused_variables)] context: &mut WebsocketContext,
        error: Option<TaskExecutionError>,
    ) -> anyhow::Result<Option<TaskExecutionError>> {
        Ok(error)
    }

    async fn handle_action_with_context(
        &mut self,
        context: &mut WebsocketContext,
        action: Self::Action,
    ) -> anyhow::Result<EventLoopInstruction>;

    async fn handle_bytes_with_context(
        &mut self,
        context: &mut WebsocketContext,
        bytes: Bytes,
    ) -> anyhow::Result<EventLoopInstruction>;

    fn get_id(&self) -> u64;
}

pub struct WebsocketActorContext<Handler: WebsocketActorContextHandler> {
    subtask: Pin<Box<dyn Future<Output = anyhow::Result<()>>>>,
    watch_dog: WatchDog,
    raw_data_stream: web::Payload,
    action_rx: mpsc::Receiver<Handler::Action>,
    websocket_context: WebsocketContext,
    data_processing_handler: Handler,
}

#[derive(Debug, thiserror::Error)]
pub enum TaskExecutionError {
    #[error("failed to call the started hook of the handler")]
    StartedHookCallingError {
        #[source]
        source: anyhow::Error,
        data_processing_handler_info: DataProcessingHandlerInfo,
    },
    #[error("failed to call the stopped hook of the handler")]
    StoppedHookCallingError {
        #[source]
        source: anyhow::Error,
        data_processing_handler_info: DataProcessingHandlerInfo,
    },
    #[error("the handler failed to handle the incoming action")]
    HandlingActionError {
        #[source]
        source: anyhow::Error,
        data_processing_handler_info: DataProcessingHandlerInfo,
    },
    #[error("the websocket context failed to handle the incoming raw data from websocket")]
    HandlingBytesError {
        #[source]
        source: WebsocketDataProcessingError,
        data_processing_handler_info: DataProcessingHandlerInfo,
    },
    #[error("failed to feed raw data")]
    FeedRawDataError {
        #[source]
        source: FeedRawDataError,
        data_processing_handler_info: DataProcessingHandlerInfo,
    },
    #[error("the subtask ended with error")]
    SubtaskError {
        #[source]
        source: anyhow::Error,
        data_processing_handler_info: DataProcessingHandlerInfo,
    },
}

#[derive(Debug, Serialize, Clone)]
pub struct DataProcessingHandlerInfo {
    id: u64,
    r#type: &'static str,
}

impl DataProcessingHandlerInfo {
    pub(super) fn new<Handler: WebsocketActorContextHandler>(
        h: &Handler,
    ) -> DataProcessingHandlerInfo {
        Self {
            id: h.get_id(),
            r#type: type_name::<Handler>(),
        }
    }
}

impl<Handler: WebsocketActorContextHandler> WebsocketActorContext<Handler> {
    pub fn launch_inline(
        payload: web::Payload,
        action_rx: mpsc::Receiver<Handler::Action>,
        data_processing_handler: Handler,
        watch_dog_timeout: Duration,
        watch_dog_check_interval: Duration,
        subtask: impl Future<Output = anyhow::Result<()>> + 'static,
    ) -> impl Stream<Item = Result<Bytes, TaskExecutionError>> {
        let (ws_sender, ws_receiver) = mpsc::channel(8);

        let actor_context = Self {
            subtask: Box::pin(subtask),
            watch_dog: WatchDog::new(watch_dog_timeout, watch_dog_check_interval),
            raw_data_stream: payload,
            action_rx,
            websocket_context: WebsocketContext::new(ws_sender),
            data_processing_handler,
        };

        let fut = actor_context.task();

        fut_into_output_stream(
            ws_receiver,
            fut,
            Some(|e| {
                log::error!(error:? = e; "A websocket actor context ended with error.");
                e
            }),
        )
    }

    async fn task(mut self) -> Result<(), TaskExecutionError> {
        let data_processing_handler_info =
            DataProcessingHandlerInfo::new(&self.data_processing_handler);

        log::debug!(data_processing_handler_info:serde; "The websocket actor context begin running...");

        let (raw_incoming_tx, raw_incoming_rx) = mpsc::channel::<Bytes>(1);

        let feed_raw_data_fut = feed_raw_data(self.raw_data_stream, raw_incoming_tx, |bytes| bytes);
        // actix_rt::pin!(feed_raw_data_fut);

        let (watch_dog, watch_dog_fut) = self.watch_dog.launch_inline();
        // actix_rt::pin!(watch_dog_fut);

        let subtask = self.subtask;

        self.data_processing_handler
            .started(&mut self.websocket_context)
            .await
            .map_err(|source| TaskExecutionError::StartedHookCallingError {
                source,
                data_processing_handler_info: data_processing_handler_info.clone(),
            })?;

        let map_feed_raw_data_error = |source| TaskExecutionError::FeedRawDataError {
            source,
            data_processing_handler_info: data_processing_handler_info.clone(),
        };

        let map_subtask_error = |source| TaskExecutionError::SubtaskError {
            source,
            data_processing_handler_info: data_processing_handler_info.clone(),
        };

        let _info = data_processing_handler_info.clone();
        let process_incoming_raw_fut = process_incoming_raw(
            &mut self.websocket_context,
            &mut self.data_processing_handler,
            &_info,
            raw_incoming_rx,
            self.action_rx,
            watch_dog,
        );
        // actix_rt::pin!(process_incoming_raw_fut);

        let error = tokio::select! {
            biased;

            end = process_incoming_raw_fut => {
                log::debug!(data_processing_handler_info:?; "The incoming channel is closed.");
                end.err()
            },
            end = feed_raw_data_fut => {
                log::debug!(data_processing_handler_info:?; "The raw data feeder is closed.");
                end.map_err(map_feed_raw_data_error).err()
            },
            end = subtask => {
                log::debug!(data_processing_handler_info:?; "The subtask has ended.");
                end.map_err(map_subtask_error).err()
            },
            _ = watch_dog_fut => {
                log::debug!(data_processing_handler_info:?; "The timeout has been reached.");
                None
            }
        };

        log::debug!(error:?, data_processing_handler_info:?; "A websocket actor context is stopping...");

        let result = self
            .data_processing_handler
            .stopped(&mut self.websocket_context, error)
            .await
            .map_err(|source| TaskExecutionError::StoppedHookCallingError {
                source,
                data_processing_handler_info: data_processing_handler_info.clone(),
            });

        match result {
            Ok(error) => {
                self.websocket_context.do_close().await;
                match error {
                    Some(error) => Err(error),
                    None => Ok(()),
                }
            }
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod debug_tests {
    use std::any::type_name;

    use anyhow::anyhow;

    use crate::{
        actors::websocket::actor_context::{DataProcessingHandlerInfo, TaskExecutionError},
        tests_utils::setup_logger,
    };

    #[ignore]
    #[test]
    fn debug_error_logging() {
        setup_logger();

        let e = TaskExecutionError::StartedHookCallingError {
            source: anyhow!("inner err"),
            data_processing_handler_info: DataProcessingHandlerInfo {
                id: 10,
                r#type: type_name::<TaskExecutionError>(),
            },
        };

        log::error!(e:debug; "{}", e);
    }
}
