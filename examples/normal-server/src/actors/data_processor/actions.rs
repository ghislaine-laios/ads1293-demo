use crate::actors::data_pusher::LaunchedDataPusher;

#[derive(Debug)]
pub enum Action {}

#[derive(Debug)]
pub struct LinkDataPusher(LaunchedDataPusher);
