use crate::actors::{
    interval::watch_dog::LaunchedWatchDog, websocket::actor_context::EventLoopInstruction,
};
use actix_web::web::Bytes;
use futures::future::pending;
use tokio::sync::mpsc;

use super::{
    DataProcessingHandlerInfo, TaskExecutionError, WebsocketActorContextHandler, WebsocketContext,
};

pub(super) async fn process_incoming_raw<Handler: WebsocketActorContextHandler>(
    websocket_context: &mut WebsocketContext,
    data_processing_handler: &mut Handler,
    data_processing_handler_info: &DataProcessingHandlerInfo,
    mut raw_incoming_rx: mpsc::Receiver<Bytes>,
    mut action_rx: mpsc::Receiver<Handler::Action>,
    watch_dog: LaunchedWatchDog,
) -> Result<(), TaskExecutionError> {
    let map_handling_bytes_error = |source| TaskExecutionError::HandlingBytesError {
        source,
        data_processing_handler_info: data_processing_handler_info.clone(),
    };

    let map_handling_action_error = |source| TaskExecutionError::HandlingActionError {
        source,
        data_processing_handler_info: data_processing_handler_info.clone(),
    };

    macro_rules! break_event_loop {
        ($handle_result: expr, $loop_label: tt) => {
            if matches!($handle_result, EventLoopInstruction::Break) {
                break $loop_label;
            }
        };
    }

    let mut rx_closed = false;

    'event_loop: loop {
        let action_rx_wrapper = async {
            if !rx_closed {
                action_rx.recv().await
            } else {
                pending().await
            }
        };

        tokio::select! {
            biased;

            bytes = raw_incoming_rx.recv() => {
                let Some(bytes) = bytes else {
                    log::debug!(data_processing_handler_info:?; "The raw_incoming_rx has closed.");
                    break 'event_loop
                };
                watch_dog.do_notify_alive().await.unwrap();
                let handle_result = websocket_context
                    .handle_raw(data_processing_handler, bytes)
                    .await
                    .map_err(map_handling_bytes_error)?;
                break_event_loop!(handle_result, 'event_loop);

            },
            action = action_rx_wrapper => {
                let Some(action) = action else {
                    rx_closed = true;
                    continue 'event_loop
                };
                let handle_result = data_processing_handler
                    .handle_action_with_context(websocket_context, action)
                    .await
                    .map_err(map_handling_action_error)?;
                break_event_loop!(handle_result, 'event_loop);
            }

        }
    }

    Ok(())
}
