use nvml_wrapper_sys::bindings::{nvmlDevice_t, NvmlLib};
use std::{panic, process};
use std::sync::{Once, Arc, Mutex};
use signal_hook::{consts::signal::*, iterator::Signals};

use crate::constants::MAX_TEMPERATURE;

// Safe wrapper for nvmlDevice_t
pub struct SafeNvmlDevice {
    pub handle: nvmlDevice_t
}
unsafe impl Send for SafeNvmlDevice {}

// TODO: Shall be deprecated at some point.
pub fn set_core_clock_offset(
    nvml_lib: &NvmlLib,
    device_handle: &Arc<Mutex<SafeNvmlDevice>>,
    offset: i32
) -> Result<(), String> {
    let device_handle = device_handle.lock().unwrap();
    let result = unsafe { nvml_lib.nvmlDeviceSetGpcClkVfOffset(device_handle.handle, offset) };
    if result != 0 {
        Err(format!("Error code: {}", result))
    } else {
        Ok(())
    }
}

// TODO: Shall be deprecated at some point.
pub fn set_memory_clock_offset(
    nvml_lib: &NvmlLib,
    device_handle: &Arc<Mutex<SafeNvmlDevice>>,
    offset: i32
) -> Result<(), String> {
    let device_handle = device_handle.lock().unwrap();
    let result = unsafe { nvml_lib.nvmlDeviceSetMemClkVfOffset(device_handle.handle, offset) };
    if result != 0 {
        Err(format!("Error code: {}", result))
    } else {
        Ok(())
    }
}

pub fn set_power_limit(
    nvml_lib: &NvmlLib,
    device_handle: &Arc<Mutex<SafeNvmlDevice>>,
    limit: u32
) -> Result<(), String> {
    let device_handle = device_handle.lock().unwrap();
    let result = unsafe { nvml_lib.nvmlDeviceSetPowerManagementLimit(device_handle.handle, limit * 1000) };
    if result != 0 {
        Err(format!("Error code: {}", result))
    } else {
        Ok(())
    }
}

// This is used so nvmlDeviceSetFanSpeed_v2 (main loop) can no more be called after nvmlDeviceSetDefaultFanSpeed_v2 (panic hook or Term-signal),
// This is important to make sure the default fan speed is used after the program stopped.
pub struct FanSpeedState {
    pub default: bool
}

pub fn get_temperature(
    nvml_lib: &NvmlLib,
    device_handle: &Arc<Mutex<SafeNvmlDevice>>
) -> Result<u32, String> {
    let mut temp = MAX_TEMPERATURE;
    let device_handle = device_handle.lock().unwrap();
    let result = unsafe { nvml_lib.nvmlDeviceGetTemperature(device_handle.handle, nvml_wrapper_sys::bindings::nvmlTemperatureSensors_enum_NVML_TEMPERATURE_GPU, &mut temp) };
    if result != 0 {
        Err(format!("Error code: {}", result))
    } else {
        Ok(temp)
    }
}

fn get_num_fans(
    nvml_lib: &NvmlLib,
    device_handle: &Arc<Mutex<SafeNvmlDevice>>
) -> Result<u32, String> {
    let mut num: u32 = 0;
    let device_handle = device_handle.lock().unwrap();
    let result = unsafe { nvml_lib.nvmlDeviceGetNumFans(device_handle.handle, &mut num) };
    if result != 0 {
        Err(format!("Error code: {}", result))
    } else {
        Ok(num)
    }
}

pub fn set_fan_speed(
    nvml_lib: &NvmlLib,
    device_handle: &Arc<Mutex<SafeNvmlDevice>>,
    fan_speed_state: &Arc<Mutex<FanSpeedState>>,
    speed: u32
) -> Result<(), String> {
    let num_fans = get_num_fans(&nvml_lib, &device_handle).map_err(|e| format!("Failed to get the number of fans: {}", e))?;
    
    let device_handle = device_handle.lock().unwrap();
    let fan_speed_state = fan_speed_state.lock().unwrap();
    if fan_speed_state.default {
        return Ok(())
    }

    for fan in 0..num_fans {
        let result = unsafe { nvml_lib.nvmlDeviceSetFanSpeed_v2(device_handle.handle, fan, speed) };
        if result != 0 {
            return Err(format!("Error code: {}", result))
        }
    }
    Ok(())
}

fn set_default_fan_speed(
    nvml_lib: &NvmlLib,
    device_handle: &Arc<Mutex<SafeNvmlDevice>>,
    fan_speed_state: &Arc<Mutex<FanSpeedState>>
) -> Result<(), String> {
    let num_fans = get_num_fans(&nvml_lib, device_handle).map_err(|e| format!("Failed to get the number of fans: {}", e))?;
    
    let device_handle = device_handle.lock().unwrap();
    let mut fan_speed_state = fan_speed_state.lock().unwrap();
    fan_speed_state.default = true;

    for fan in 0..num_fans {
        let result = unsafe { nvml_lib.nvmlDeviceSetDefaultFanSpeed_v2(device_handle.handle, fan) };
        if result != 0 {
            return Err(format!("Error code: {}", result))
        }
    }
    Ok(())
}

pub fn setup_cleanup(
    nvml_lib: Arc<NvmlLib>,
    device_handle: Arc<Mutex<SafeNvmlDevice>>,
    fan_speed_state: Arc<Mutex<FanSpeedState>>
) -> Result<(), String> {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // Set panic hook
        panic::set_hook(Box::new({
            let nvml_lib = Arc::clone(&nvml_lib);
            let device_handle = Arc::clone(&device_handle);
            let fan_speed_state = Arc::clone(&fan_speed_state);
            move |info| {
                eprintln!("Panic occurred: {:?}", info);
                if let Err(e) = set_default_fan_speed(&nvml_lib, &device_handle, &fan_speed_state) {
                    eprintln!("!!! Setting the default fan speed failed on panic !!!\n\n{}", e);
                } else {
                    println!("Successfully set default fan speed on panic!");
                }
            }
        }));

        // Handle Term-signals
        let mut term_signals = Signals::new(&[SIGALRM, SIGHUP, SIGINT, SIGIO, SIGPIPE, SIGPROF, SIGTERM, SIGUSR1, SIGUSR2, SIGVTALRM])
        .map_err(|e| format!("Error setting up Term-signal handler: {}", e)).unwrap();
        std::thread::spawn(move || {
            for signal in term_signals.forever() {
                println!("Term-signal received: {}", signal);
                if let Err(e) = set_default_fan_speed(&nvml_lib, &device_handle, &fan_speed_state) {
                    eprintln!("!!! Setting the default fan speed failed on exit !!!\n\n{}", e);
                } else {
                    println!("Successfully set default fan speed on exit!");
                }
                process::exit(0);
            }
        });

        // Handle Stop-signals
        let mut stop_signals = Signals::new(&[SIGTSTP, SIGTTIN, SIGTTOU]).map_err(|e| format!("Error setting up Stop-signal handler: {}", e)).unwrap();
        std::thread::spawn(move || {
            for signal in stop_signals.forever() {
                println!("Ignored stop due to Stop-signal: {}", signal);
            }
        });
    });
    Ok(())
}