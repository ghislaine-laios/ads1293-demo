pub struct DataPusherBuilder {}

pub struct DataPusher {}

pub(super) mod actions {
    use actix_web::web::Bytes;

    #[derive(Debug)]
    pub enum Action {
        RawFromWebsocket(RawFromWebsocket),
    }

    #[derive(Debug)]
    pub struct RawFromWebsocket(pub Bytes);
}
