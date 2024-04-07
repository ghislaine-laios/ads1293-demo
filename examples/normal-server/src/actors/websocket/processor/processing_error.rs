use crate::actors::websocket::{websocket_handler::WebsocketHandler, FeedRawDataError};
use std::fmt::Debug;

#[derive(Debug, thiserror::Error)]
pub enum ProcessingError<P: WebsocketHandler> {
    #[error("failed to feed raw data")]
    FeedRawDataError(FeedRawDataError),

    #[error("the started hook of the process data handler ended with error")]
    StartProcessingDataFailed(P::StartedError),
    #[error("failed to process the data by the ws context")]
    ProcessDataFailed(super::super::context::ProcessingError<P::ProcessDataError>),
    #[error("the stopping hook of the process data handler ended with error")]
    StopProcessingDataFailed(P::StoppingError),
    #[error("the subtask ended with error")]
    SubtaskError(Box<dyn Debug>),
    #[error("an unknown internal bug occurred")]
    InternalBug(anyhow::Error),
}
