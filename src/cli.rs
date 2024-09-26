use clap::Parser;

#[derive(Parser)]
#[command(version, about)]
pub struct Cli {
    /// GPU index
    #[arg(short, long, default_value = "0")]
    pub index: u32,

    /// Core clock offset (MHz)
    #[arg(short, long)]
    pub core_clock_offset: Option<i32>,

    /// Memory clock offset (MHz)
    #[arg(short, long)]
    pub memory_clock_offset: Option<i32>,

    /// Power limit (W)
    #[arg(short = 'l', long)]
    pub power_limit: Option<u32>,

    /// Temperature (°C) and fan speed (%) pairs in the format temp1:fan1,temp2:fan2,...
    #[arg(short, long)]
    pub pairs: Option<String>,

    /// Fan speed update period (s)
    #[arg(short, long, default_value = "2")]
    pub fan_speed_update_period: u64,

    /// Temperature hysteresis (°C)
    #[arg(short, long, default_value = "5")]
    pub temperature_hysteresis: u32
}