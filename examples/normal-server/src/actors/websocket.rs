use std::sync::Arc;

use actix_web::{error::PayloadError, web::Bytes};
use futures::Stream;
use futures_util::stream::StreamExt;
use tokio::{
    sync::mpsc::{Receiver, Sender},
    task::{JoinError, JoinHandle},
};

#[derive(Debug, thiserror::Error, Clone)]
pub enum FeedRawDataError {
    #[error("the given action channel is closed")]
    ActionChannelClosed,
    #[error("failed to retrieve new bytes from the source stream (payload error occurred)")]
    PayloadError(Arc<PayloadError>),
}

pub async fn feed_raw_data<S, A, M>(
    stream: S,
    action_sender: Sender<A>,
    mut map_fn: M,
) -> Result<(), FeedRawDataError>
where
    S: Stream<Item = Result<Bytes, PayloadError>> + Unpin,
    M: FnMut(Bytes) -> A,
{
    let mut stream = stream.map(|result| result.map(&mut map_fn));

    while let Some(result) = stream.next().await {
        match result {
            Ok(action) => action_sender
                .send(action)
                .await
                .map_err(|_| FeedRawDataError::ActionChannelClosed)?,
            Err(e) => Err(FeedRawDataError::PayloadError(Arc::new(e)))?,
        };
    }

    Ok(())
}

pub fn ws_output_stream<E: std::error::Error>(
    mut rx: Receiver<Bytes>,
    mut join_handle: JoinHandle<Result<(), E>>,
    panic_handle: impl FnOnce(JoinError) -> E,
    error_handle: Option<impl FnOnce(E) -> E>,
) -> impl Stream<Item = Result<Bytes, E>> {
    let stream = async_stream::stream! {
        loop {
            tokio::select! {
                bytes = rx.recv() => {
                    match bytes {
                        Some(bytes) => yield Ok(bytes),
                        None => break,
                    }
                },
                join_result = &mut join_handle => {
                    let task_result = match join_result {
                        Ok(r) => r,
                        Err(e) => {
                            let e = panic_handle(e);
                            yield Err(e);
                            break
                        },
                    };

                    if let Err(mut e) = task_result {
                        if let Some(handle) = error_handle {
                            e = handle(e);
                        }
                        yield Err(e);
                    }

                    break
                }
            }
        }
    };

    stream
}
