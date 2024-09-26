use clap::Parser;
use nvml_wrapper::Nvml;
use nvml_wrapper_sys::bindings::NvmlLib;
use std::{thread, time::Duration};
use std::sync::{Arc, Mutex};

mod constants;
mod cli;
mod nvml;
mod utils;

use cli::Cli;
use nvml::{SafeNvmlDevice, set_core_clock_offset, set_memory_clock_offset, set_power_limit, FanSpeedState, get_temperature, set_fan_speed, setup_cleanup};
use utils::{parse_temperature_fan_speed_pairs, check_temperature_fan_speed_pairs, interpolate_fan_speed};

fn main() {
    let cli = Cli::parse();

    sudo2::escalate_if_needed().map_err(|e| format!("Failed to escalate privileges {}", e)).unwrap();

    // INITIALIZE NVML

    let nvml = Nvml::init().map_err(|e| format!("Failed to initialize NVML: {}", e)).unwrap();
    check_driver_version(&nvml).unwrap();

    let nvml_lib = Arc::new(unsafe { NvmlLib::new("libnvidia-ml.so").map_err(|e| format!("Failed to load NVML library: {}", e)).unwrap() });

    let device = nvml.device_by_index(cli.index).map_err(|e| format!("Failed to get GPU: {}", e)).unwrap();
    let device_handle: Arc<Mutex<SafeNvmlDevice>> = Arc::new(Mutex::new(SafeNvmlDevice { handle: unsafe { device.handle() } }));

    // OVERCLOCK

    if let Some(core_clock_offset) = cli.core_clock_offset {
        set_core_clock_offset(&nvml_lib, &device_handle, core_clock_offset)
        .map_err(|e| format!("Failed to set core clock offset: {}", e)).unwrap();
    }

    if let Some(memory_clock_offset) = cli.memory_clock_offset {
        set_memory_clock_offset(&nvml_lib, &device_handle, memory_clock_offset)
        .map_err(|e| format!("Failed to set memory clock offset: {}", e)).unwrap();
    }

    if let Some(power_limit) = cli.power_limit {
        set_power_limit(&nvml_lib, &device_handle, power_limit)
        .map_err(|e| format!("Failed to set power limit: {}", e)).unwrap();
    }

    // FAN CONTROL

    if let Some(pairs) = cli.pairs {
        let temp_fan_pairs: Vec<utils::TempFanPair> = parse_temperature_fan_speed_pairs(&pairs).map_err(|e| format!("Failed to parse temperature and fan speed pairs: {}", e)).unwrap();
        check_temperature_fan_speed_pairs(&temp_fan_pairs).unwrap();
    
        let fan_speed_state = Arc::new(Mutex::new(FanSpeedState { default: false }));
        setup_cleanup(Arc::clone(&nvml_lib), Arc::clone(&device_handle), Arc::clone(&fan_speed_state))
        .map_err(|e| format!("Failed to setup cleanup: {}", e)).unwrap();
    
        let mut hyst_upper_temp: u32 = 0;
        let mut last_fan_speed: i64 = -1;
        let first_temp = temp_fan_pairs[0].temperature;
        loop {
            let mut temperature = get_temperature(&nvml_lib, &device_handle).map_err(|e| format!("Failed to get temperature: {}", e)).unwrap();
    
            // Apply hysteresis to temperature
            if cli.temperature_hysteresis > 0 {
                if (temperature < hyst_upper_temp) && ((temperature + cli.temperature_hysteresis) >= hyst_upper_temp) {
                    if temperature > first_temp {
                        temperature = hyst_upper_temp;
                    }
                }
                else {
                    hyst_upper_temp = temperature;
                }
            }
    
            let fan_speed = interpolate_fan_speed(&temp_fan_pairs, temperature);
    
            // Set fan speed if it has changed
            if last_fan_speed != i64::from(fan_speed) {
                set_fan_speed(&nvml_lib, &device_handle, &fan_speed_state, fan_speed)
                .map_err(|e| format!("Failed to set fan speed: {}", e)).unwrap();
    
                last_fan_speed = i64::from(fan_speed);
            }
    
            thread::sleep(Duration::from_secs(cli.fan_speed_update_period));
        }
    }
}

fn check_driver_version(
    nvml: &Nvml
) -> Result<(), String> {
    let driver_version: String = nvml.sys_driver_version().map_err(|e| format!("Failed to get driver version: {}", e))?;

    let major: i32;
    if let Some(first_string) = driver_version.split('.').next() {
        major = first_string.parse::<i32>().map_err(|e| format!("Failed to parse major version from '{}': {}", first_string, e))?;
    } else {
        return Err(format!("Failed to split driver version '{}'", driver_version));
    }

    let major_min: i32 = 520;
    if major < major_min
    {
        return Err(format!("Your driver version v{} is not supported. Driver version v{} or newer is required", major, major_min))
    }
    Ok(())
}