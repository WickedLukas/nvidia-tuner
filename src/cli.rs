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

    /// Fan speed update period (s)
    #[arg(short, long, default_value = "2", value_parser = validate_fan_speed_update_period)]
    pub fan_speed_update_period: u64,

    /// Temperature hysteresis (°C)
    #[arg(short, long, default_value = "5")]
    pub temperature_hysteresis: u32,

    /// Show the current GPU settings and their allowed ranges without changing anything
    #[arg(short, long, conflicts_with_all = ["core_clock_offset", "memory_clock_offset", "max_core_clock", "power_limit", "pairs", "fan_speed_update_period", "temperature_hysteresis"])]
    pub show: bool
}