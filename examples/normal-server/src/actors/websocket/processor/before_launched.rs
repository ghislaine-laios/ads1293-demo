use crate::actors::{
    interval::watch_dog::WatchDog,
    websocket::{
        context::WebsocketContext, fut_into_output_stream, subtask::Subtask,
        websocket_handler::WebsocketHandler,
    },
};
use actix_web::web::{self, Bytes};
use futures::Stream;
use std::time::Duration;
use tokio::sync::mpsc;

use super::{ProcessingError, Processor};

pub struct ProcessorMeta<P: WebsocketHandler, S: Subtask> {
    pub process_data_handler: P,
    pub subtask: S,
    pub watch_dog_timeout_seconds: u64,
    pub watch_dog_check_interval_seconds: u64,
}

pub struct ProcessorBeforeLaunched<P, S>
where
    P: WebsocketHandler<Context = WebsocketContext>,
    S: Subtask,
    <S as Subtask>::OutputError: 'static,
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

        let processor = Processor {
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

pub fn new_ws_processor<P, S>(
    payload: web::Payload,
    meta: ProcessorMeta<P, S>,
) -> ProcessorBeforeLaunched<P, S>
where
    P: WebsocketHandler<Context = WebsocketContext>,
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
