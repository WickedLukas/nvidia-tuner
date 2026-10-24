use nvml_wrapper_sys::bindings::{
    nvmlDevice_t, nvmlReturn_t, nvmlClockOffset_t, nvmlClockType_t, nvmlPstates_t, NvmlLib,
    nvmlClockType_enum_NVML_CLOCK_GRAPHICS, nvmlClockType_enum_NVML_CLOCK_MEM, nvmlPStates_enum_NVML_PSTATE_UNKNOWN, NVML_MAX_GPU_PERF_PSTATES,
    nvmlReturn_enum_NVML_ERROR_NOT_SUPPORTED, nvmlReturn_enum_NVML_ERROR_FUNCTION_NOT_FOUND, nvmlReturn_enum_NVML_ERROR_ARGUMENT_VERSION_MISMATCH
};
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
    pub original_core_clock_offset: Option<OriginalClockOffset>,   // MHz
    pub original_memory_clock_offset: Option<OriginalClockOffset>, // MHz
    pub original_power_limit: Option<u32>                          // mW
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

// Original clock offset, depending on which NVML API was used to change it
pub enum OriginalClockOffset {
    // Per P-state API (driver >= 555): original offset of every changed P-state
    PerPState(Vec<(nvmlPstates_t, i32)>),
    // Legacy VF offset API (driver < 555)
    Legacy(i32)
}

fn new_clock_offset(
    clock_type: nvmlClockType_t,
    pstate: nvmlPstates_t,
    offset: i32
) -> nvmlClockOffset_t {
    nvmlClockOffset_t {
        // Implements NVML_STRUCT_VERSION(ClockOffset, 1), as detailed in nvml.h
        version: (size_of::<nvmlClockOffset_t>() | (1 << 24)) as u32,
        type_: clock_type,
        pstate,
        clockOffsetMHz: offset,
        minClockOffsetMHz: 0,
        maxClockOffsetMHz: 0
    }
}

// Returns None if the supported P-states can not be read from this device
fn get_supported_pstates(
    nvml_lib: &NvmlLib,
    device: nvmlDevice_t
) -> Result<Option<Vec<nvmlPstates_t>>, String> {
    let mut pstates = [nvmlPStates_enum_NVML_PSTATE_UNKNOWN; NVML_MAX_GPU_PERF_PSTATES as usize];
    let result = unsafe { nvml_lib.nvmlDeviceGetSupportedPerformanceStates(device, pstates.as_mut_ptr(), NVML_MAX_GPU_PERF_PSTATES) };
    if is_unavailable(result) {
        return Ok(None)
    }
    check_result(result)?;
    Ok(Some(pstates.into_iter().filter(|&pstate| pstate != nvmlPStates_enum_NVML_PSTATE_UNKNOWN).collect()))
}

// Return codes which mean the per P-state API can not be used, so the legacy API is used instead
fn is_unavailable(
    result: nvmlReturn_t
) -> bool {
    result == nvmlReturn_enum_NVML_ERROR_NOT_SUPPORTED
    || result == nvmlReturn_enum_NVML_ERROR_FUNCTION_NOT_FOUND
    || result == nvmlReturn_enum_NVML_ERROR_ARGUMENT_VERSION_MISMATCH
}

// Reads the original offset of every supported P-state.
// Returns None if the per P-state API is not available, e.g. because the driver is older than 555.
fn get_per_pstate_clock_offsets(
    nvml_lib: &NvmlLib,
    device: nvmlDevice_t,
    clock_type: nvmlClockType_t,
    offset: i32
) -> Result<Option<Vec<(nvmlPstates_t, i32)>>, String> {
    if nvml_lib.nvmlDeviceGetClockOffsets.is_err() || nvml_lib.nvmlDeviceSetClockOffsets.is_err() {
        return Ok(None)
    }

    let Some(pstates) = get_supported_pstates(nvml_lib, device).map_err(|e| format!("Failed to get supported P-states: {}", e))? else {
        return Ok(None)
    };

    let mut original_offsets = Vec::new();
    for pstate in pstates {
        let mut info = new_clock_offset(clock_type, pstate, 0);
        let result = unsafe { nvml_lib.nvmlDeviceGetClockOffsets(device, &mut info) };
        if is_unavailable(result) {
            // This P-state has no adjustable clock offset
            continue;
        }
        check_result(result).map_err(|e| format!("Failed to get clock offset of P-state {}: {}", pstate, e))?;

        if offset < info.minClockOffsetMHz || offset > info.maxClockOffsetMHz {
            return Err(format!("Offset of {} MHz is out of range [{}, {}] MHz", offset, info.minClockOffsetMHz, info.maxClockOffsetMHz))
        }
        original_offsets.push((pstate, info.clockOffsetMHz));
    }

    if original_offsets.is_empty() {
        Ok(None)
    } else {
        Ok(Some(original_offsets))
    }
}

fn set_per_pstate_clock_offset(
    nvml_lib: &NvmlLib,
    device: nvmlDevice_t,
    clock_type: nvmlClockType_t,
    pstate: nvmlPstates_t,
    offset: i32
) -> Result<(), String> {
    let mut info = new_clock_offset(clock_type, pstate, offset);
    check_result(unsafe { nvml_lib.nvmlDeviceSetClockOffsets(device, &mut info) })
    .map_err(|e| format!("Failed to set clock offset of P-state {}: {}", pstate, e))
}

// TODO: Legacy API. Shall be removed once drivers older than 555 are no longer supported.
fn get_legacy_clock_offset(
    nvml_lib: &NvmlLib,
    device: nvmlDevice_t,
    clock_type: nvmlClockType_t
) -> Result<i32, String> {
    let mut offset: i32 = 0;
    if clock_type == nvmlClockType_enum_NVML_CLOCK_GRAPHICS {
        check_result(unsafe { nvml_lib.nvmlDeviceGetGpcClkVfOffset(device, &mut offset) })?;
    } else {
        check_result(unsafe { nvml_lib.nvmlDeviceGetMemClkVfOffset(device, &mut offset) })?;
    }
    Ok(offset)
}

// TODO: Legacy API. Shall be removed once drivers older than 555 are no longer supported.
fn set_legacy_clock_offset(
    nvml_lib: &NvmlLib,
    device: nvmlDevice_t,
    clock_type: nvmlClockType_t,
    offset: i32
) -> Result<(), String> {
    if clock_type == nvmlClockType_enum_NVML_CLOCK_GRAPHICS {
        check_result(unsafe { nvml_lib.nvmlDeviceSetGpcClkVfOffset(device, offset) })
    } else {
        check_result(unsafe { nvml_lib.nvmlDeviceSetMemClkVfOffset(device, offset) })
    }
}

fn restore_clock_offset(
    nvml_lib: &NvmlLib,
    device: nvmlDevice_t,
    clock_type: nvmlClockType_t,
    original: &OriginalClockOffset
) -> Result<(), String> {
    match original {
        OriginalClockOffset::PerPState(original_offsets) => {
            // Try to restore every P-state, even if restoring a previous one failed
            let errors: Vec<String> = original_offsets.iter()
                .filter_map(|&(pstate, offset)| set_per_pstate_clock_offset(nvml_lib, device, clock_type, pstate, offset).err())
                .collect();
            if errors.is_empty() { Ok(()) } else { Err(errors.join(", ")) }
        }
        OriginalClockOffset::Legacy(offset) => set_legacy_clock_offset(nvml_lib, device, clock_type, *offset)
    }
}

// Sets the offset for every P-state which supports it, as the legacy API did.
// Falls back to the legacy API if the per P-state API is not available.
fn set_clock_offset(
    nvml_lib: &NvmlLib,
    device_handle: &Arc<Mutex<SafeNvmlDevice>>,
    tuner_state: &Arc<Mutex<TunerState>>,
    clock_type: nvmlClockType_t,
    offset: i32
) -> Result<(), String> {
    let device_handle = device_handle.lock().unwrap();
    let mut tuner_state = tuner_state.lock().unwrap();
    if tuner_state.restored {
        return Ok(())
    }
    let device = device_handle.handle;

    let original = if clock_type == nvmlClockType_enum_NVML_CLOCK_GRAPHICS {
        &mut tuner_state.original_core_clock_offset
    } else {
        &mut tuner_state.original_memory_clock_offset
    };

    if original.is_none() {
        *original = Some(match get_per_pstate_clock_offsets(nvml_lib, device, clock_type, offset)? {
            Some(original_offsets) => OriginalClockOffset::PerPState(original_offsets),
            None => {
                println!("Per P-state clock offsets are not supported, using the legacy clock offset API");
                OriginalClockOffset::Legacy(get_legacy_clock_offset(nvml_lib, device, clock_type)
                .map_err(|e| format!("Failed to get original offset: {}", e))?)
            }
        });
    }

    match original.as_ref().unwrap() {
        OriginalClockOffset::PerPState(original_offsets) => {
            for &(pstate, _) in original_offsets {
                set_per_pstate_clock_offset(nvml_lib, device, clock_type, pstate, offset)?;
            }
            Ok(())
        }
        OriginalClockOffset::Legacy(_) => set_legacy_clock_offset(nvml_lib, device, clock_type, offset)
    }
}

pub fn set_core_clock_offset(
    nvml_lib: &NvmlLib,
    device_handle: &Arc<Mutex<SafeNvmlDevice>>,
    tuner_state: &Arc<Mutex<TunerState>>,
    offset: i32
) -> Result<(), String> {
    set_clock_offset(nvml_lib, device_handle, tuner_state, nvmlClockType_enum_NVML_CLOCK_GRAPHICS, offset)
}

pub fn set_memory_clock_offset(
    nvml_lib: &NvmlLib,
    device_handle: &Arc<Mutex<SafeNvmlDevice>>,
    tuner_state: &Arc<Mutex<TunerState>>,
    offset: i32
) -> Result<(), String> {
    set_clock_offset(nvml_lib, device_handle, tuner_state, nvmlClockType_enum_NVML_CLOCK_MEM, offset)
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

    if let Some(original) = &tuner_state.original_core_clock_offset {
        if let Err(e) = restore_clock_offset(nvml_lib, device_handle.handle, nvmlClockType_enum_NVML_CLOCK_GRAPHICS, original) {
            errors.push(format!("Failed to restore core clock offset: {}", e));
        }
    }

    if let Some(original) = &tuner_state.original_memory_clock_offset {
        if let Err(e) = restore_clock_offset(nvml_lib, device_handle.handle, nvmlClockType_enum_NVML_CLOCK_MEM, original) {
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
