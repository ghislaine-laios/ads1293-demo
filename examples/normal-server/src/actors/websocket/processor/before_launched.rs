use crate::actors::{
    handler::ContextHandler,
    interval::watch_dog::WatchDog,
    websocket::{
        context::WebsocketContext, fut_into_output_stream, subtask::Subtask,
        websocket_handler::WebsocketHandler,
    },
};
use actix_web::web::{self, Bytes};
use futures::Stream;
use std::time::Duration;
use tokio::sync::mpsc::{self, Receiver};

use super::{ProcessingError, Processor};

pub struct ProcessorMeta {
    pub watch_dog_timeout_seconds: u64,
    pub watch_dog_check_interval_seconds: u64,
}

pub struct ProcessorBeforeLaunched {
    raw_data_input_stream: web::Payload,
    watch_dog: WatchDog,
}

impl ProcessorBeforeLaunched {
    pub fn launch_inline<A, P, S>(
        self,
        process_data_handler: P,
        subtask: S,
        action_rx: Receiver<A>,
    ) -> impl Stream<Item = Result<Bytes, ProcessingError<P>>>
    where
        P: ContextHandler<A, Context = WebsocketContext, Output = anyhow::Result<()>>,
        P: WebsocketHandler<Context = WebsocketContext>,
        S: Subtask,
        <S as Subtask>::OutputError: 'static,
    {
        let (launched_watch_dog, timeout_fut) = self.watch_dog.launch_inline();

        let (ws_sender, ws_receiver) = mpsc::channel(8);

        let processor = Processor {
            watch_dog: launched_watch_dog,
            websocket_context: WebsocketContext::new(ws_sender),
            process_data_handler: process_data_handler,
        };

        let fut = processor.task(
            self.raw_data_input_stream,
            timeout_fut,
            subtask,
            Some(action_rx),
        );

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

pub fn new_ws_processor(payload: web::Payload, meta: ProcessorMeta) -> ProcessorBeforeLaunched {
    ProcessorBeforeLaunched {
        raw_data_input_stream: payload,
        watch_dog: WatchDog::new(
            Duration::from_secs(meta.watch_dog_timeout_seconds),
            Duration::from_secs(meta.watch_dog_check_interval_seconds),
        ),
    }
}
