use crate::actors::handler::ContextHandler;
use actix_http::ws::{self, ProtocolError};
use actix_web::web::{Bytes, BytesMut};
use futures::TryFutureExt;
use std::fmt::Debug;
use tokio::sync::mpsc;
use tokio_util::codec::{Decoder, Encoder};

use super::neo::WebsocketActorContextHandler;

#[derive(Clone, Copy, Debug)]
pub enum ConnectionStatus {
    Activated,
    SeverRequestClosing,
    PeerRequestClosing,
    Closed,
}
pub struct WebsocketContext {
    decode_buf: BytesMut,
    codec: ws::Codec,
    status: ConnectionStatus,
    ws_sender: mpsc::Sender<Bytes>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProcessingError<E: Debug> {
    #[error("failed to decode the incoming websocket frame")]
    FrameDecodeFailed(ProtocolError),
    #[error("the incoming websocket frame is not supported")]
    NotSupportedFrame(String),
    #[error("failed to send message to the peer")]
    SendToPeerError(SendToPeerError),
    #[error("failed to process the data")]
    ProcessDataFailed(E),
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

impl WebsocketContext {
    pub fn new(ws_sender: mpsc::Sender<Bytes>) -> Self {
        Self {
            decode_buf: BytesMut::new(),
            codec: ws::Codec::new(),
            status: ConnectionStatus::Activated,
            ws_sender,
        }
    }

    pub(super) async fn handle_raw_old<P, E>(
        &mut self,
        bytes: Bytes,
        handler: &mut P,
    ) -> Result<(), ProcessingError<E>>
    where
        P: ContextHandler<Bytes, Context = Self, Output = Result<(), E>>,
        E: Debug + 'static,
    {
        self.decode_buf.extend_from_slice(&bytes[..]);

        while let Some(frame) = self
            .codec
            .decode(&mut self.decode_buf)
            .map_err(ProcessingError::FrameDecodeFailed)?
        {
            self.handle_frame(handler, frame).await?;
        }

        Ok(())
    }

    async fn handle_frame<P, E>(
        &mut self,
        handler: &mut P,
        frame: ws::Frame,
    ) -> Result<(), ProcessingError<E>>
    where
        P: ContextHandler<Bytes, Context = Self, Output = Result<(), E>>,
        E: Debug + 'static,
    {
        log::trace!("frame: {:?}", frame);

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
                // let data =
                //     serde_json::from_slice(&text[..]).map_err(ProcessingError::DataDecodeFailed)?;
                self.process_bytes(handler, text).await?;

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

    pub async fn send_to_peer(&mut self, msg: ws::Message) -> Result<(), SendToPeerError> {
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

    pub async fn do_close(&mut self) {
        if matches!(
            self.status,
            ConnectionStatus::SeverRequestClosing | ConnectionStatus::Closed
        ) {
            return;
        }

        let _r = self.send_to_peer(ws::Message::Close(None)).await;
    }

    async fn process_bytes<P, E>(
        &mut self,
        handler: &mut P,
        bytes: Bytes,
    ) -> Result<(), ProcessingError<E>>
    where
        P: ContextHandler<Bytes, Context = Self, Output = Result<(), E>>,
        E: Debug + 'static,
    {
        log::trace!("bytes: {:?}", bytes);

        handler
            .handle_with_context(self, bytes)
            .map_err(|e| ProcessingError::ProcessDataFailed(e))
            .await
    }
}
