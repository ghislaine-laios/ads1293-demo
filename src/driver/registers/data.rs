use bitfield::bitfield;

bitfield! {
    pub struct MainConfig(u8);
    impl Debug;
    bool;
    power_down, set_power_down: 2;
    standby, set_standby: 1;
    start_conversion, set_start_conversion: 0;
}
