use std::sync::Once;

use crate::settings::Settings;

static INIT: Once = Once::new();

pub(crate) fn setup_logger() {
    INIT.call_once(|| {
        env_logger::Builder::from_default_env()
            .filter_module(super::MODULE_PATH, log::LevelFilter::Debug)
            .init();

        log::debug!(
            "The debug level of module path {} has been set to debug.",
            super::MODULE_PATH
        )
    })
}

pub(crate) fn settings() -> Settings {
    Settings::new().unwrap()
}

#[cfg(test)]
mod tests {
    use super::setup_logger;

    #[actix_rt::test]
    async fn it_works() {
        setup_logger();
        log::debug!("{:#?}", super::settings());
    }
}
