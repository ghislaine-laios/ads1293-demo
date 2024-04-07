use actix_web::web::Bytes;
use core::fmt::Debug;

use crate::actors::{handler::ContextHandler, Handler};

use super::processor::actions::{Started, Stopping};

pub trait WebsocketHandler:
    Handler<Started, Output = Result<(), <Self as WebsocketHandler>::StartedError>>
    + ContextHandler<
        Bytes,
        Context = <Self as WebsocketHandler>::Context,
        Output = Result<(), <Self as WebsocketHandler>::ProcessDataError>,
    > + Handler<Stopping, Output = Result<(), <Self as WebsocketHandler>::StoppingError>>
    + Debug
where
    Self::StartedError: Debug + 'static,
    Self::ProcessDataError: Debug + 'static,
    Self::StoppingError: Debug + 'static,
{
    type StartedError;
    type ProcessDataError;
    type StoppingError;
    type Context;
}

impl<T, TS, TP, TST, Context> WebsocketHandler for T
where
    T: Handler<Started, Output = Result<(), TS>>
        + ContextHandler<Bytes, Context = Context, Output = Result<(), TP>>
        + Handler<Stopping, Output = Result<(), TST>>
        + Debug,
    TS: Debug + 'static,
    TP: Debug + 'static,
    TST: Debug + 'static,
{
    type StartedError = TS;

    type ProcessDataError = TP;

    type StoppingError = TST;

    type Context = Context;
}
