use clap::Parser;
use nvml_wrapper::Nvml;
use nvml_wrapper_sys::bindings::NvmlLib;
use std::{thread, time::Duration};
use std::sync::{Arc, Mutex};

mod constants;
mod cli;
mod nvml;
mod show;
mod utils;
mod util;
mod intent;

use cli::Cli;
use constants::MAX_CONSECUTIVE_FAN_CONTROL_ERRORS;
use nvml::{SafeNvmlDevice, TunerState, set_core_clock_offset, set_memory_clock_offset, get_min_core_clock, set_locked_core_clocks, get_power_limit_constraints, set_power_limit, init_fan_control, get_min_max_fan_speed, get_temperature, set_fan_speed, setup_cleanup};
use show::show;
use utils::{parse_temperature_fan_speed_pairs, check_temperature_fan_speed_pairs, check_fan_speed_range, interpolate_fan_speed};
use util::get_utilization;
use intent::{IntentCfg, decide, State};

fn main() {
    let cli = Cli::parse();

    // Only read and print the current state. This changes nothing, so it needs neither root nor a reset on exit.
    if cli.show {
        let nvml = Nvml::init().map_err(|e| format!("Failed to initialize NVML: {}", e)).unwrap();
        show(&nvml, cli.index).unwrap();
        return;
    }

    // Validate all input before any setting is changed
    let temp_fan_pairs: Option<Vec<utils::TempFanPair>> = cli.pairs.as_ref().map(|pairs| {
        let temp_fan_pairs = parse_temperature_fan_speed_pairs(pairs).map_err(|e| format!("Failed to parse temperature and fan speed pairs: {}", e)).unwrap();
        check_temperature_fan_speed_pairs(&temp_fan_pairs).unwrap();
        temp_fan_pairs
    });

    escalate_privileges().map_err(|e| format!("Failed to escalate privileges: {}", e)).unwrap();

    // INITIALIZE NVML

    let nvml = Nvml::init().map_err(|e| format!("Failed to initialize NVML: {}", e)).unwrap();
    check_driver_version(&nvml).unwrap();

    let nvml_lib = Arc::new(unsafe { NvmlLib::new("libnvidia-ml.so.1") }.map_err(|e| format!("Failed to load NVML library: {}", e)).unwrap());

    let device = nvml.device_by_index(cli.index).map_err(|e| format!("Failed to get GPU: {}", e)).unwrap();
    let device_handle: Arc<Mutex<SafeNvmlDevice>> = Arc::new(Mutex::new(SafeNvmlDevice { handle: unsafe { device.handle() } }));

    // Reset all changed settings to default on exit. This must be set up before any setting is changed.
    let tuner_state = Arc::new(Mutex::new(TunerState::default()));
    setup_cleanup(Arc::clone(&nvml_lib), Arc::clone(&device_handle), Arc::clone(&tuner_state))
    .map_err(|e| format!("Failed to setup cleanup: {}", e)).unwrap();

    if let Some(temp_fan_pairs) = &temp_fan_pairs {
        if let Some((min_fan_speed, max_fan_speed)) = get_min_max_fan_speed(&nvml_lib, &device_handle)
        .map_err(|e| format!("Failed to get the supported fan speed range: {}", e)).unwrap() {
            check_fan_speed_range(temp_fan_pairs, min_fan_speed, max_fan_speed).unwrap();
        }
    }

    let min_core_clock: Option<u32> = cli.max_core_clock.map(|max_core_clock| {
        let min_core_clock = get_min_core_clock(&nvml_lib, &device_handle).map_err(|e| format!("Failed to get the lowest supported core clock: {}", e)).unwrap();
        if max_core_clock <= min_core_clock {
            panic!("Maximum core clock of {} MHz must be above the lowest supported core clock of {} MHz", max_core_clock, min_core_clock);
        }
        min_core_clock
    });

    if let Some(power_limit) = cli.power_limit {
        let (min_power_limit, max_power_limit) = get_power_limit_constraints(&nvml_lib, &device_handle)
        .map_err(|e| format!("Failed to get the allowed power limit range: {}", e)).unwrap();
        if power_limit * 1000 < min_power_limit || power_limit * 1000 > max_power_limit {
            // Round the range inwards, so every value within the shown range is accepted
            panic!("Power limit of {} W is out of the allowed range [{}, {}] W", power_limit, min_power_limit.div_ceil(1000), max_power_limit / 1000);
        }
    }

    // OVERCLOCK

    if let Some(core_clock_offset) = cli.core_clock_offset {
        set_core_clock_offset(&nvml_lib, &device_handle, &tuner_state, core_clock_offset)
        .map_err(|e| format!("Failed to set core clock offset: {}", e)).unwrap();
    }

    if let Some(memory_clock_offset) = cli.memory_clock_offset {
        set_memory_clock_offset(&nvml_lib, &device_handle, &tuner_state, memory_clock_offset)
        .map_err(|e| format!("Failed to set memory clock offset: {}", e)).unwrap();
    }

    if let (Some(min_core_clock), Some(max_core_clock)) = (min_core_clock, cli.max_core_clock) {
        set_locked_core_clocks(&nvml_lib, &device_handle, &tuner_state, min_core_clock, max_core_clock)
        .map_err(|e| format!("Failed to lock core clock: {}", e)).unwrap();
    }

    if let Some(power_limit) = cli.power_limit {
        set_power_limit(&nvml_lib, &device_handle, &tuner_state, power_limit)
        .map_err(|e| format!("Failed to set power limit: {}", e)).unwrap();
    }

    // FAN CONTROL

    // [fork 新增·启动门卫] main()：intent 模式强制要求 --pairs——复用上游"有 pairs 才进风扇接管"
    // 门控（init_fan_control 的 AUTO→手动切换、退出交还链全挂在该块上）；缺 pairs 直接拒启动，
    // 防"参数写了 mode 却静默不接管"的旁路假象。curve 模式行为不受本门卫影响。
    if cli.fan_mode == "intent" && temp_fan_pairs.is_none() {
        panic!("--fan-mode intent requires --pairs (fan-control gate; curve table itself unused in intent mode)");
    }
    if let Some(temp_fan_pairs) = temp_fan_pairs {
        init_fan_control(&nvml_lib, &device_handle, &tuner_state)
        .map_err(|e| format!("Failed to initialize fan control: {}", e)).unwrap();

        // 意图律环（八格决策表移植；u=意图/T=状态/d̂=势能，逐拍全表重判）。
        if cli.fan_mode == "intent" {
            let icfg = IntentCfg {
                free_lo: cli.intent_free_lo, warn: cli.intent_warn, free_hi: cli.intent_free_hi,
                floor: cli.intent_floor, load_line: cli.intent_load_line,
                slope_up: cli.intent_slope_up, slope_down: cli.intent_slope_down,
            };
            if !(icfg.free_lo < icfg.warn && icfg.warn < icfg.free_hi
                 && icfg.floor <= icfg.free_lo && icfg.slope_up > 0 && icfg.slope_down > 0) {
                panic!("intent config invalid: {icfg:?}");
            }
            let (min_fan, max_fan) = get_min_max_fan_speed(&nvml_lib, &device_handle)
                .map_err(|e| format!("Failed to get fan speed range: {}", e)).unwrap()
                .unwrap_or((30, 100));
            if icfg.free_hi > max_fan {
                panic!("intent config: free_hi {} above driver max {} — would pin the fan at max forever", icfg.free_hi, max_fan);
            }
            let mut t_prev: Option<u32> = None;
            let mut duty: u32 = icfg.floor.max(min_fan);
            let mut state = State::IdleLow;
            let mut util_warned = false;
            let mut u_hold: u32 = 0; // util 失效=冻结最后已知意图（不判空载、防满载误降地板）
            eprintln!("intent: online free {}..{} warn {} floor {} (driver min {}) slopes {}/{}",
                icfg.free_lo, icfg.free_hi, icfg.warn, icfg.floor, min_fan, icfg.slope_up, icfg.slope_down);
            // [fork 新增·意图环] update_intent()：单拍=读 T(上游 get_temperature)+读 u(新 util::get_utilization,
            // 失败冻结最后已知意图) → intent::decide 出 (duty, state) → 驱动包络钳位 → set_fan_speed(上游)。
            // 每拍决策日志一行（t/u/duty/state 可回放质询）；连续失败计数走上游同一 fail-safe（panic→交还 AUTO）。
            let mut update_intent = || -> Result<(), String> {
                let t = get_temperature(&nvml_lib, &device_handle)?;
                // u 采集失败降级=按空载意图（保守：撤干预回地板；温度再热会自然再触发）
                let u = match get_utilization(&nvml_lib, &device_handle) {
                    Ok(v) => { u_hold = v; v }
                    Err(e) => {
                        if !util_warned { eprintln!("intent: utilization unavailable ({}), holding last intent", e); util_warned = true; }
                        u_hold
                    }
                };
                (duty, state) = decide(&icfg, t, t_prev, u, duty, max_fan);
                let duty = duty.max(min_fan); // 驱动包络铁钳：永不写进锁死区
                eprintln!("t={} u={} duty={} state={:?}", t, u, duty, state);
                set_fan_speed(&nvml_lib, &device_handle, &tuner_state, duty)
                    .map_err(|e| format!("Failed to set fan speed: {}", e))?;
                t_prev = Some(t);
                Ok(())
            };
            let mut errs: u32 = 0;
            loop {
                if let Err(e) = update_intent() {
                    errs += 1;
                    eprintln!("{} (consecutive error {} of {})", e, errs, MAX_CONSECUTIVE_FAN_CONTROL_ERRORS);
                    if errs >= MAX_CONSECUTIVE_FAN_CONTROL_ERRORS { panic!("Fan control failed {} times in a row", errs); }
                } else { errs = 0; }
                thread::sleep(Duration::from_secs(cli.fan_speed_update_period));
            }
        }

        let mut hyst_upper_temp: u32 = 0;
        let mut last_fan_speed: i64 = -1;
        let first_temp = temp_fan_pairs[0].temperature;
        let mut update_fan_speed = || -> Result<(), String> {
            let mut temperature = get_temperature(&nvml_lib, &device_handle).map_err(|e| format!("Failed to get temperature: {}", e))?;

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
                set_fan_speed(&nvml_lib, &device_handle, &tuner_state, fan_speed).map_err(|e| format!("Failed to set fan speed: {}", e))?;
                last_fan_speed = i64::from(fan_speed);
            }
            Ok(())
        };

        // Retry on errors, so fan control is not lost due to a single failure.
        // Give up after too many consecutive errors, which resets the changed settings via the panic hook.
        let mut consecutive_errors: u32 = 0;
        loop {
            if let Err(e) = update_fan_speed() {
                consecutive_errors += 1;
                eprintln!("{} (consecutive error {} of {})", e, consecutive_errors, MAX_CONSECUTIVE_FAN_CONTROL_ERRORS);
                if consecutive_errors >= MAX_CONSECUTIVE_FAN_CONTROL_ERRORS {
                    panic!("Fan control failed {} times in a row", consecutive_errors);
                }
            } else {
                consecutive_errors = 0;
            }

            thread::sleep(Duration::from_secs(cli.fan_speed_update_period));
        }
    }

    // Keep running until a Term-signal is received, so the changed settings are reset to default on exit
    loop {
        thread::park();
    }
}

fn escalate_privileges(
) -> Result<(), Box<dyn std::error::Error>> {
    if sudo2::running_as_root() {
        return Ok(());
    }

    if which::which("sudo").is_ok() {
        sudo2::escalate_if_needed()?;
    } else if which::which("doas").is_ok() {
        sudo2::doas()?;
    } else if which::which("pkexec").is_ok() {
        sudo2::pkexec()?;
    } else {
        return Err("Root privileges are required to run this command.".into());
    }

    Ok(())
}

fn check_driver_version(
    nvml: &Nvml
) -> Result<(), String> {
    let driver_version: String = nvml.sys_driver_version().map_err(|e| format!("Failed to get driver version: {}", e))?;

    let major: i32;
    if let Some(first_string) = driver_version.split('.').next() {
        major = first_string.parse::<i32>().map_err(|e| format!("Failed to parse major version from '{}': {}", first_string, e))?;
    } else {
        return Err(format!("Failed to split driver version '{}'.", driver_version));
    }

    let major_min: i32 = 520;
    if major < major_min
    {
        return Err(format!("Your driver version v{} is not supported. Driver version v{} or newer is required.", major, major_min))
    }
    Ok(())
}
