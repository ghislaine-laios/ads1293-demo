use esp_idf_svc::hal::{
    delay::FreeRtos,
    gpio::{Output, Pin, PinDriver},
};

pub fn start_program_blink<P: Pin>(led_pin: &mut PinDriver<'static, P, Output>) {
    led_pin.set_high().unwrap();
    for _ in 0..2 {
        FreeRtos::delay_ms(100);
        led_pin.set_low().unwrap();
        FreeRtos::delay_ms(200);
        led_pin.set_high().unwrap();
    }
}

pub fn in_program_blink<P: Pin>(led_pin: &mut PinDriver<'static, P, Output>, is_high_now: &mut bool) {
    if *is_high_now {
        led_pin.set_low().unwrap();
    } else {
        led_pin.set_high().unwrap();
    }
    *is_high_now = !(*is_high_now);
}
