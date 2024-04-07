use normal_data::Data;

use crate::actors::{
    data_processor::DataProcessorId,
    data_pusher::{DataPusherId, LaunchedDataPusher},
};

#[derive(Debug)]
pub enum Action {
    RegisterDataProcessor(RegisterDataProcessor),
    UnregisterDataProcessor(UnregisterDataProcessor),
    RegisterDataPusher(RegisterDataPusher),
    UnRegisterDataPusher(UnRegisterDataPusher),
    NewDataFromProcessor(NewDataFromProcessor),
}

#[derive(Debug)]
pub struct RegisterDataProcessor(pub DataProcessorId);

#[derive(Debug)]
pub struct UnregisterDataProcessor(pub DataProcessorId);

#[derive(Debug)]
pub struct RegisterDataPusher(pub DataPusherId, pub LaunchedDataPusher);

#[derive(Debug)]
pub struct UnRegisterDataPusher(pub DataPusherId);

#[derive(Debug)]
pub struct NewDataFromProcessor(pub DataProcessorId, pub Data);
