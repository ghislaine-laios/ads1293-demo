#![allow(non_camel_case_types)]

pub mod access;
pub mod data;
pub mod addressable;

use enum_variant_type::EnumVariantType;

#[derive(EnumVariantType)]
pub enum Register {
    CONFIG,
    FLEX_CH1_CN,
}
