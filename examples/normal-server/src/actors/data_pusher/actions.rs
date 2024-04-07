use normal_data::Data;

use crate::actors::data_processor::DataProcessorId;

#[derive(Debug)]
pub enum Action {
    NewData(NewData),
}

#[derive(Debug)]
pub struct NewData(pub DataProcessorId, pub Data);
