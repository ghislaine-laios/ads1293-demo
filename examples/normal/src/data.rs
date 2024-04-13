use ads1293_demo::driver::initialization::Application3Lead;
use ads1293_demo::driver::initialization::Initializer;
use ads1293_demo::driver::registers;
use ads1293_demo::driver::registers::access::ReadFromRegister;
use ads1293_demo::driver::registers::DataRegister;
use ads1293_demo::driver::registers::DATA_STATUS;
use ads1293_demo::driver::ADS1293;
use embedded_hal::spi::SpiDevice;
use esp_idf_sys::esp_timer_get_time;
use normal_data::Data;
use std::time::Duration;
use tokio::sync::mpsc::error::TrySendError;

pub async fn data(device: impl SpiDevice, data_sender: tokio::sync::mpsc::Sender<Data>) {
    let mut ads1293 = ADS1293::new(device);

    ads1293
        .init(Application3Lead)
        .expect("failed to init the ads1293");

    log::info!("ADS1293 initialized.");

    let main_config = ads1293
        .read(registers::CONFIG)
        .expect("failed to read the main config");

    log::info!("main_config: {:#?}", main_config);

    let loop_back_mode_config = ads1293
        .read(registers::CH_CNFG)
        .expect("fail to read loop back mode config");
    log::info!("loop_back_mode_config: {:#?}", loop_back_mode_config);

    let mut id = 0;
    let mut perf_counter = 0;
    let mut fail_counter = 60;
    let mut start_time = unsafe { esp_timer_get_time() };
    let frame_duration = Duration::from_secs_f32(1.0 / 60.0);
    let mut sleep_deadline = tokio::time::Instant::now();
    'out: loop {
        tokio::time::sleep_until(sleep_deadline).await;
        let processing_begin = tokio::time::Instant::now();
        perf_counter += 1;
        sleep_deadline = sleep_deadline.checked_add(frame_duration).unwrap();

        let _data_status = ads1293.read(DATA_STATUS).expect("fail to read data status");

        let data_vec = ads1293
            .stream_one()
            .expect("failed to read data under stream mode");
        log::trace!("data: {:?}", data_vec);

        for data in data_vec {
            let DataRegister::DATA_CH1_ECG(data) = data else {
                continue;
            };

            if let Err(e) = data_sender.try_send(Data {
                id,
                value: data.into(),
            }) {
                match e {
                    // I drop the data directly to ensure that
                    // the data's ID remains reliable for timing purposes.
                    TrySendError::Full(_data) => {
                        if fail_counter == 60 {
                            log::warn!("the data channel is full. the data will be dropped.");
                            fail_counter = 0;
                        }

                        fail_counter += 1;
                    }
                    TrySendError::Closed(_) => break 'out,
                }
            }

            break;
        }

        id += 1;

        if perf_counter == 600 {
            let end_time = unsafe { esp_timer_get_time() };
            let span = end_time - start_time;
            start_time = end_time;
            log::info!("last 600 frame: {} ms", span / 1000);
            let processing_end = tokio::time::Instant::now();
            let real_processing_time = processing_end.duration_since(processing_begin);
            log::info!("This frame take {} ms to be retrieved.", real_processing_time.as_millis());
            perf_counter = 0;
        }

        let processing_end = tokio::time::Instant::now();
        let real_processing_time = processing_end.duration_since(processing_begin);
        if real_processing_time >= Duration::from_millis(10) {
            log::warn!("Cation: data retrieving takes too much time.");
        }
    }
}
