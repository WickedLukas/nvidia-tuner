//! v0.7 fork 新增：NVML 利用率采集（意图律输入 u）。
//! 惯用法与 nvml.rs::get_temperature 同构（NvmlLib 门面直调 + check_result）；
//! 绑定形状经 nvml-wrapper-sys 0.10.0 源码对质：fn 返回 nvmlReturn_t（非 Result），
//! 结构体无 Default derive（显式零初始化）——缺失符号 expect panic 走 fail-safe 交还路。

use crate::nvml::{check_result, SafeNvmlDevice};
use nvml_wrapper_sys::bindings::{nvmlUtilization_t, NvmlLib};
use std::sync::{Arc, Mutex};

/// GPU 利用率 %（意图源：满载必升温、空载必降温）
pub fn get_utilization(
    nvml_lib: &NvmlLib,
    device_handle: &Arc<Mutex<SafeNvmlDevice>>,
) -> Result<u32, String> {
    let mut util = nvmlUtilization_t { gpu: 0, memory: 0 };
    let device_handle = device_handle.lock().unwrap();
    check_result(unsafe { nvml_lib.nvmlDeviceGetUtilizationRates(device_handle.handle, &mut util) })?;
    Ok(util.gpu)
}
