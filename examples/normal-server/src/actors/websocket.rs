use actix_web::{error::PayloadError, web::Bytes};
use futures::Stream;
use futures_util::stream::StreamExt;
use tokio::sync::mpsc::Sender;

#[derive(Debug, thiserror::Error)]
pub enum FeedRawDataError {
    #[error("the given action channel is closed")]
    ActionChannelClosed,
    #[error("failed to retrieve new bytes from the source stream (payload error occurred)")]
    PayloadError(PayloadError),
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
            Err(e) => Err(FeedRawDataError::PayloadError(e))?,
        };
    }

    Ok(())
}
