use nvml_wrapper_sys::bindings::{nvmlDevice_t, nvmlReturn_t, NvmlLib};
use std::{panic, process};
use std::sync::{Once, Arc, Mutex};
use signal_hook::{consts::signal::*, iterator::Signals};

use crate::constants::MAX_TEMPERATURE;

// Safe wrapper for nvmlDevice_t
pub struct SafeNvmlDevice {
    pub handle: nvmlDevice_t
}
unsafe impl Send for SafeNvmlDevice {}

// Tracks every setting changed by this tool, so the original settings can be restored on exit (panic hook or Term-signal).
// Once restored is set, the setters no longer change anything. This is important to make sure the original settings are used after the program stopped.
#[derive(Default)]
pub struct TunerState {
    pub restored: bool,
    pub num_fans: u32,
    pub fan_speed_changed: bool,
    pub original_core_clock_offset: Option<i32>,   // MHz
    pub original_memory_clock_offset: Option<i32>, // MHz
    pub original_power_limit: Option<u32>          // mW
}

fn check_result(
    result: nvmlReturn_t
) -> Result<(), String> {
    if result != 0 {
        Err(format!("Error code: {}", result))
    } else {
        Ok(())
    }
}

// TODO: Shall be deprecated at some point.
pub fn set_core_clock_offset(
    nvml_lib: &NvmlLib,
    device_handle: &Arc<Mutex<SafeNvmlDevice>>,
    tuner_state: &Arc<Mutex<TunerState>>,
    offset: i32
) -> Result<(), String> {
    let device_handle = device_handle.lock().unwrap();
    let mut tuner_state = tuner_state.lock().unwrap();
    if tuner_state.restored {
        return Ok(())
    }

    if tuner_state.original_core_clock_offset.is_none() {
        let mut original_offset: i32 = 0;
        check_result(unsafe { nvml_lib.nvmlDeviceGetGpcClkVfOffset(device_handle.handle, &mut original_offset) })
        .map_err(|e| format!("Failed to get original core clock offset: {}", e))?;
        tuner_state.original_core_clock_offset = Some(original_offset);
    }

    check_result(unsafe { nvml_lib.nvmlDeviceSetGpcClkVfOffset(device_handle.handle, offset) })
}

// TODO: Shall be deprecated at some point.
pub fn set_memory_clock_offset(
    nvml_lib: &NvmlLib,
    device_handle: &Arc<Mutex<SafeNvmlDevice>>,
    tuner_state: &Arc<Mutex<TunerState>>,
    offset: i32
) -> Result<(), String> {
    let device_handle = device_handle.lock().unwrap();
    let mut tuner_state = tuner_state.lock().unwrap();
    if tuner_state.restored {
        return Ok(())
    }

    if tuner_state.original_memory_clock_offset.is_none() {
        let mut original_offset: i32 = 0;
        check_result(unsafe { nvml_lib.nvmlDeviceGetMemClkVfOffset(device_handle.handle, &mut original_offset) })
        .map_err(|e| format!("Failed to get original memory clock offset: {}", e))?;
        tuner_state.original_memory_clock_offset = Some(original_offset);
    }

    check_result(unsafe { nvml_lib.nvmlDeviceSetMemClkVfOffset(device_handle.handle, offset) })
}

pub fn set_power_limit(
    nvml_lib: &NvmlLib,
    device_handle: &Arc<Mutex<SafeNvmlDevice>>,
    tuner_state: &Arc<Mutex<TunerState>>,
    limit: u32
) -> Result<(), String> {
    let device_handle = device_handle.lock().unwrap();
    let mut tuner_state = tuner_state.lock().unwrap();
    if tuner_state.restored {
        return Ok(())
    }

    if tuner_state.original_power_limit.is_none() {
        let mut original_limit: u32 = 0;
        check_result(unsafe { nvml_lib.nvmlDeviceGetPowerManagementLimit(device_handle.handle, &mut original_limit) })
        .map_err(|e| format!("Failed to get original power limit: {}", e))?;
        tuner_state.original_power_limit = Some(original_limit);
    }

    check_result(unsafe { nvml_lib.nvmlDeviceSetPowerManagementLimit(device_handle.handle, limit * 1000) })
}

pub fn get_temperature(
    nvml_lib: &NvmlLib,
    device_handle: &Arc<Mutex<SafeNvmlDevice>>
) -> Result<u32, String> {
    let mut temp = MAX_TEMPERATURE;
    let device_handle = device_handle.lock().unwrap();
    check_result(unsafe { nvml_lib.nvmlDeviceGetTemperature(device_handle.handle, nvml_wrapper_sys::bindings::nvmlTemperatureSensors_enum_NVML_TEMPERATURE_GPU, &mut temp) })?;
    Ok(temp)
}

fn get_num_fans(
    nvml_lib: &NvmlLib,
    device_handle: &Arc<Mutex<SafeNvmlDevice>>
) -> Result<u32, String> {
    let mut num: u32 = 0;
    let device_handle = device_handle.lock().unwrap();
    check_result(unsafe { nvml_lib.nvmlDeviceGetNumFans(device_handle.handle, &mut num) })?;
    Ok(num)
}

// Reads the number of fans once, so it does not need to be read on every fan speed update.
pub fn init_fan_control(
    nvml_lib: &NvmlLib,
    device_handle: &Arc<Mutex<SafeNvmlDevice>>,
    tuner_state: &Arc<Mutex<TunerState>>
) -> Result<(), String> {
    let num_fans = get_num_fans(nvml_lib, device_handle).map_err(|e| format!("Failed to get the number of fans: {}", e))?;
    tuner_state.lock().unwrap().num_fans = num_fans;
    Ok(())
}

pub fn set_fan_speed(
    nvml_lib: &NvmlLib,
    device_handle: &Arc<Mutex<SafeNvmlDevice>>,
    tuner_state: &Arc<Mutex<TunerState>>,
    speed: u32
) -> Result<(), String> {
    let device_handle = device_handle.lock().unwrap();
    let mut tuner_state = tuner_state.lock().unwrap();
    if tuner_state.restored {
        return Ok(())
    }

    tuner_state.fan_speed_changed = true;
    for fan in 0..tuner_state.num_fans {
        check_result(unsafe { nvml_lib.nvmlDeviceSetFanSpeed_v2(device_handle.handle, fan, speed) })?;
    }
    Ok(())
}

fn restore_original_settings(
    nvml_lib: &NvmlLib,
    device_handle: &Arc<Mutex<SafeNvmlDevice>>,
    tuner_state: &Arc<Mutex<TunerState>>
) -> Result<(), String> {
    // Ignore poisoned locks, restoring the original settings is more important
    let device_handle = device_handle.lock().unwrap_or_else(|e| e.into_inner());
    let mut tuner_state = tuner_state.lock().unwrap_or_else(|e| e.into_inner());
    if tuner_state.restored {
        return Ok(())
    }
    tuner_state.restored = true;

    // Try to restore every setting, even if restoring a previous one failed
    let mut errors: Vec<String> = Vec::new();

    if tuner_state.fan_speed_changed {
        for fan in 0..tuner_state.num_fans {
            if let Err(e) = check_result(unsafe { nvml_lib.nvmlDeviceSetDefaultFanSpeed_v2(device_handle.handle, fan) }) {
                errors.push(format!("Failed to set default fan speed of fan {}: {}", fan, e));
            }
        }
    }

    if let Some(offset) = tuner_state.original_core_clock_offset {
        if let Err(e) = check_result(unsafe { nvml_lib.nvmlDeviceSetGpcClkVfOffset(device_handle.handle, offset) }) {
            errors.push(format!("Failed to restore core clock offset: {}", e));
        }
    }

    if let Some(offset) = tuner_state.original_memory_clock_offset {
        if let Err(e) = check_result(unsafe { nvml_lib.nvmlDeviceSetMemClkVfOffset(device_handle.handle, offset) }) {
            errors.push(format!("Failed to restore memory clock offset: {}", e));
        }
    }

    if let Some(limit) = tuner_state.original_power_limit {
        if let Err(e) = check_result(unsafe { nvml_lib.nvmlDeviceSetPowerManagementLimit(device_handle.handle, limit) }) {
            errors.push(format!("Failed to restore power limit: {}", e));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

pub fn setup_cleanup(
    nvml_lib: Arc<NvmlLib>,
    device_handle: Arc<Mutex<SafeNvmlDevice>>,
    tuner_state: Arc<Mutex<TunerState>>
) -> Result<(), String> {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // Set panic hook
        panic::set_hook(Box::new({
            let nvml_lib = Arc::clone(&nvml_lib);
            let device_handle = Arc::clone(&device_handle);
            let tuner_state = Arc::clone(&tuner_state);
            move |info| {
                eprintln!("Panic occurred: {:?}", info);
                if let Err(e) = restore_original_settings(&nvml_lib, &device_handle, &tuner_state) {
                    eprintln!("!!! Restoring the original settings failed on panic !!!\n\n{}", e);
                } else {
                    println!("Successfully restored original settings on panic!");
                }
            }
        }));

        // Handle Term-signals
        let mut term_signals = Signals::new(&[SIGALRM, SIGHUP, SIGINT, SIGIO, SIGPIPE, SIGPROF, SIGTERM, SIGUSR1, SIGUSR2, SIGVTALRM])
        .map_err(|e| format!("Error setting up Term-signal handler: {}", e)).unwrap();
        std::thread::spawn(move || {
            if let Some(signal) = term_signals.forever().next() {
                println!("Term-signal received: {}", signal);
                if let Err(e) = restore_original_settings(&nvml_lib, &device_handle, &tuner_state) {
                    eprintln!("!!! Restoring the original settings failed on exit !!!\n\n{}", e);
                } else {
                    println!("Successfully restored original settings on exit!");
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
