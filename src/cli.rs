use clap::Parser;

use crate::utils::validate_fan_speed_update_period;

#[derive(Parser)]
#[command(version, about)]
pub struct Cli {
    /// GPU index
    #[arg(short, long, default_value = "0")]
    pub index: u32,

    /// Core clock offset (MHz)
    #[arg(short, long, allow_hyphen_values(true))]
    pub core_clock_offset: Option<i32>,

    /// Memory clock offset (MHz)
    #[arg(short, long, allow_hyphen_values(true))]
    pub memory_clock_offset: Option<i32>,

    /// Maximum core clock (MHz). Locks the core clock to the range from the lowest supported clock up to this value. Combined with a core clock offset this undervolts the GPU.
    #[arg(short = 'x', long)]
    pub max_core_clock: Option<u32>,

    /// Power limit (W)
    #[arg(short = 'l', long)]
    pub power_limit: Option<u32>,

    /// Temperature (°C) and fan speed (%) pairs in the format temp1:fan1,temp2:fan2,...
    #[arg(short, long)]
    pub pairs: Option<String>,

    /// Fan control mode. curve=upstream table (default), intent=heat-intent law.
    /// [fork 新增字段] Cli.fan_mode：模式分派键（main 风扇段据此二选一）；并加入 --show 互斥表
    /// （show 只读、mode 属改动类参数，同域禁混）。
    /// --pairs stays required (range validation & upgrade path); ignored under intent.
    #[arg(long, value_parser = clap::builder::PossibleValuesParser::new(["curve", "intent"]), default_value = "curve")]
    pub fan_mode: String,

    /// Intent law: free zone lower edge (°C). Below it no cooling duty.
    #[arg(long, default_value = "50")]
    pub intent_free_lo: u32,

    /// Intent law: pre-emptive ramp warning line (°C).
    #[arg(long, default_value = "65")]
    pub intent_warn: u32,

    /// Intent law: free zone upper edge (°C). Above it fast-cooling permitted.
    #[arg(long, default_value = "70")]
    pub intent_free_hi: u32,

    /// Intent law: duty floor (%) — never lower, fan never stops (driver locks ≥30 on this GPU).
    #[arg(long, default_value = "30")]
    pub intent_floor: u32,

    /// Intent law: utilization (%) at or above counts as busy intent.
    #[arg(long, default_value = "10")]
    pub intent_load_line: u32,

    /// Intent law: max duty increase per tick (%).
    #[arg(long, default_value = "4")]
    pub intent_slope_up: u32,

    /// Intent law: max duty decrease per tick (%) outside of the retreat exception.
    #[arg(long, default_value = "10")]
    pub intent_slope_down: u32,

    /// Fan speed update period (s)
    #[arg(short, long, default_value = "2", value_parser = validate_fan_speed_update_period)]
    pub fan_speed_update_period: u64,

    /// Temperature hysteresis (°C)
    #[arg(short, long, default_value = "5")]
    pub temperature_hysteresis: u32,

    /// Show the current GPU settings and their allowed ranges without changing anything
    #[arg(short, long, conflicts_with_all = ["core_clock_offset", "memory_clock_offset", "max_core_clock", "power_limit", "pairs", "fan_speed_update_period", "temperature_hysteresis", "fan_mode"])]
    pub show: bool
}
