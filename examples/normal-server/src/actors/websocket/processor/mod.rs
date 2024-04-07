use self::actions::{Started, Stopping};
use super::{context::WebsocketContext, subtask::Subtask, websocket_handler::WebsocketHandler};
use crate::actors::{
    interval::watch_dog::{LaunchedWatchDog, Timeout},
    websocket::feed_raw_data,
    Handler,
};
use actix_web::web::{self, Bytes};
use futures::{Future, TryFutureExt};
use tokio::{select, sync::mpsc};

pub mod actions;
mod before_launched;
mod processing_error;
pub use self::ProcessorAfterLaunched as Processor;
pub use before_launched::*;
pub use processing_error::ProcessingError;

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
            .await
            .map_err(|e| ProcessingError::StartProcessingDataFailed(e))?;

        let error = {
            let process_incoming_raw = async {
                while let Some(bytes) = raw_incoming_rx.recv().await {
                    self.watch_dog.do_notify_alive().await.unwrap();
                    self.websocket_context
                        .handle_raw(bytes, &mut self.process_data_handler)
                        .await
                        .map_err(|e| ProcessingError::ProcessDataFailed(e))?;
                }

                Ok(())
            };
            actix_rt::pin!(process_incoming_raw);

            
            let error = select! {
                biased;

                process_end = &mut process_incoming_raw => {
                    process_end.err()
                },
                feed_end = &mut feed_raw_data => {
                    feed_end.map_err(ProcessingError::FeedRawDataError).err()
                },
                sub_task_end = &mut sub_task => {
                    sub_task_end.map_err(
                        |e| ProcessingError::SubtaskError(Box::new(e))
                    ).err()
                },
                _ = &mut watch_dog => {None},
            };
            
            error
        };

        self.process_data_handler
            .handle(Stopping)
            .await
            .map_err(|e| ProcessingError::StopProcessingDataFailed(e))?;

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
