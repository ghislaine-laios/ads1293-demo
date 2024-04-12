use super::actor_context::{EventLoopInstruction, WebsocketActorContextHandler};
use crate::actors::websocket::actor_context::DataProcessingHandlerInfo;
use actix_http::ws::{self, ProtocolError};
use actix_web::web::{Bytes, BytesMut};
use log::{LevelFilter, STATIC_MAX_LEVEL};
use tokio::sync::mpsc;
use tokio_util::codec::{Decoder, Encoder};

#[derive(Clone, Copy, Debug)]
pub enum ConnectionStatus {
    Activated,
    SeverRequestClosing,
    PeerRequestClosing,
    Closed,
}

#[derive(Debug, thiserror::Error)]
pub enum WebsocketDataProcessingError {
    #[error("failed to decode the incoming websocket frame")]
    FrameDecodeFailed {
        #[source]
        source: ProtocolError,
        raw: Bytes,
    },
    #[error("the incoming websocket frame \"{}\" is not supported", .r#type)]
    NotSupportedFrame { r#type: &'static str },
    #[error("failed to send message to the peer")]
    SendToPeerError(#[source] SendToPeerError),
    #[error("the handler failed to process the bytes")]
    BytesProcessingFailed(#[source] anyhow::Error),
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

pub struct WebsocketContext {
    decode_buf: BytesMut,
    codec: ws::Codec,
    status: ConnectionStatus,
    ws_sender: mpsc::Sender<Bytes>,
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

    pub(super) async fn handle_raw<Handler>(
        &mut self,
        handler: &mut Handler,
        bytes: Bytes,
    ) -> Result<EventLoopInstruction, WebsocketDataProcessingError>
    where
        Handler: WebsocketActorContextHandler,
    {
        self.decode_buf.extend_from_slice(&bytes[..]);

        loop {
            let frame = match self.codec.decode(&mut self.decode_buf) {
                Ok(r) => r,
                Err(e) => {
                    return Err(WebsocketDataProcessingError::FrameDecodeFailed {
                        source: e,
                        raw: bytes,
                    })
                }
            };

            let Some(frame) = frame else { break };

            let event_loop_instruction = self.handle_frame(handler, frame).await?;

            if matches!(event_loop_instruction, EventLoopInstruction::Break) {
                return Ok(event_loop_instruction);
            }
        }

        Ok(EventLoopInstruction::Continue)
    }

    async fn handle_frame<Handler>(
        &mut self,
        handler: &mut Handler,
        frame: ws::Frame,
    ) -> Result<EventLoopInstruction, WebsocketDataProcessingError>
    where
        Handler: WebsocketActorContextHandler,
    {
        log::trace!(frame:?; "New frame received.");

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
                return Ok(EventLoopInstruction::Continue);
            }
            ConnectionStatus::Closed => {
                // Both ends have reached consensus;
                // anything sent after that will be ignored.
                return Ok(EventLoopInstruction::Continue);
            }
        };

        match frame {
            actix_http::ws::Frame::Text(text) => self.process_bytes(handler, text).await,
            actix_http::ws::Frame::Binary(_) => {
                Err(WebsocketDataProcessingError::NotSupportedFrame { r#type: "binary" })
            }
            actix_http::ws::Frame::Continuation(_) => {
                Err(WebsocketDataProcessingError::NotSupportedFrame {
                    r#type: "continuation",
                })
            }
            actix_http::ws::Frame::Ping(msg) => self
                .send_to_peer(ws::Message::Pong(msg))
                .await
                .map(|_| EventLoopInstruction::Continue)
                .map_err(WebsocketDataProcessingError::SendToPeerError),
            actix_http::ws::Frame::Pong(_) => Ok(EventLoopInstruction::Continue),
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

                r.map(|_| EventLoopInstruction::Continue)
                    .map_err(WebsocketDataProcessingError::SendToPeerError)
            }
        }
    }

    async fn process_bytes<Handler>(
        &mut self,
        handler: &mut Handler,
        bytes: Bytes,
    ) -> Result<EventLoopInstruction, WebsocketDataProcessingError>
    where
        Handler: WebsocketActorContextHandler,
    {
        if STATIC_MAX_LEVEL >= LevelFilter::Trace {
            let data_processing_handler_info = DataProcessingHandlerInfo::new(handler);
            log::trace! {
                data_processing_handler_info:serde, raw_bytes:? = bytes;
                "Raw bytes is about to be processed by the handler."
            };
        }

        handler
            .handle_bytes_with_context(self, bytes)
            .await
            .map_err(WebsocketDataProcessingError::BytesProcessingFailed)
    }

    pub async fn send_to_peer(&mut self, msg: ws::Message) -> Result<(), SendToPeerError> {
        log::trace!(msg:?; "Send message to the peer.");

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
}
