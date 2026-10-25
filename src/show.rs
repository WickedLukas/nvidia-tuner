use nvml_wrapper::{Device, Nvml, error::NvmlError};
use nvml_wrapper::enum_wrappers::device::{Clock, PerformanceState, TemperatureSensor};
use std::fmt::Display;

// Prints the current GPU state and the allowed ranges of the settings, used by --show.
// It only reads values, so it does not need root and nothing has to be reset on exit.
pub fn show(
    nvml: &Nvml,
    index: u32
) -> Result<(), String> {
    let device = nvml.device_by_index(index).map_err(|e| format!("Failed to get GPU: {}", e))?;

    println!("GPU {}: {}", index, value(device.name()));
    println!("Driver version:       {}", value(nvml.sys_driver_version()));
    println!("Temperature:          {}", value(device.temperature(TemperatureSensor::Gpu).map(|t| format!("{} °C", t))));
    println!("Performance state:    {}", value(device.performance_state().map(|p| format!("P{}", p.as_c()))));

    println!();
    println!("Core clock:           {}", value(device.clock_info(Clock::Graphics).map(|c| format!("{} MHz", c))));
    println!("Core clock offset:    {}", clock_offset(&device, Clock::Graphics));
    println!("Memory clock:         {}", value(device.clock_info(Clock::Memory).map(|c| format!("{} MHz", c))));
    println!("Memory clock offset:  {}", clock_offset(&device, Clock::Memory));

    println!();
    println!("Power usage:          {}", value(device.power_usage().map(|p| format!("{:.1} W", p as f32 / 1000.0))));
    println!("Power limit:          {}", value(device.power_management_limit().map(watts)));
    println!("Default power limit:  {}", value(device.power_management_limit_default().map(watts)));
    println!("Allowed power limit:  {}", value(device.power_management_limit_constraints().map(|c| format!("{} to {}", watts(c.min_limit), watts(c.max_limit)))));

    println!();
    match device.num_fans() {
        Ok(num_fans) => {
            for fan in 0..num_fans {
                println!("Fan {} speed:          {}", fan, value(device.fan_speed(fan).map(|s| format!("{} %", s))));
            }
        }
        Err(e) => println!("Fans:                 not available ({})", e)
    }
    println!("Allowed fan speed:    {}", value(device.min_max_fan_speed().map(|(min, max)| format!("{} to {} %", min, max))));

    Ok(())
}

fn value<T: Display>(
    result: Result<T, NvmlError>
) -> String {
    match result {
        Ok(value) => value.to_string(),
        Err(e) => format!("not available ({})", e)
    }
}

fn watts(
    milliwatts: u32
) -> String {
    format!("{} W", milliwatts / 1000)
}

// Uses the per P-state API (driver >= 555) and falls back to the legacy API, like the setters in nvml.rs
fn clock_offset(
    device: &Device,
    clock: Clock
) -> String {
    match device.clock_offset(clock, PerformanceState::Zero) {
        Ok(offset) => format!("{} MHz (allowed {} to {} MHz)", offset.clock_offset_mhz, offset.min_clock_offset_mhz, offset.max_clock_offset_mhz),
        Err(_) => {
            let legacy_offset = if clock == Clock::Graphics { device.gpc_clock_vf_offset() } else { device.mem_clock_vf_offset() };
            value(legacy_offset.map(|o| format!("{} MHz", o)))
        }
    }
}
