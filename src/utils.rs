use crate::constants::{MAX_TEMPERATURE, MAX_FAN_SPEED, MAX_FAN_SPEED_UPDATE_PERIOD};

#[derive(Debug)]
pub struct TempFanPair {
    pub temperature: u32,
    pub fan_speed: u32
}

pub fn validate_fan_speed_update_period(
    value: &str
)-> Result<u64, String> {
    let period: u64 = value.parse::<u64>().map_err(|e| format!("Failed to parse fan speed update period from '{}': {}", value, e))?;
    if period > MAX_FAN_SPEED_UPDATE_PERIOD {
        Err(format!("Fan speed update period exceeds limit of {}: {}", MAX_FAN_SPEED_UPDATE_PERIOD, period))
    } else {
        Ok(period)
    }
}

fn parse_temperature_fan_speed_parts(
    parts: &[&str]
) -> Result<TempFanPair, String> {
    if parts.len() != 2 {
        return Err(format!("Invalid temperature and fan speed pair format"));
    }

    let temperature = parts[0].trim().parse::<u32>().map_err(|e| 
        format!("Failed to parse temperature from '{}': {}", parts[0], e))?;
    let fan_speed = parts[1].trim().parse::<u32>().map_err(|e| 
        format!("Failed to parse fan speed from '{}': {}", parts[1], e))?;

    if temperature > MAX_TEMPERATURE {
        return Err(format!("Temperature within pairs exceeds limit of {}°C: {}°C", MAX_TEMPERATURE, temperature))
    }
    if fan_speed > MAX_FAN_SPEED {
        return Err(format!("Fan speed within pairs exceeds limit of {}%: {}%", MAX_FAN_SPEED, fan_speed))
    }
    Ok(TempFanPair { temperature, fan_speed })
}

pub fn parse_temperature_fan_speed_pairs(
    pairs: &str
) -> Result<Vec<TempFanPair>, String> {
    pairs
        .split(',')
        .map(|pair| {
            let parts: Vec<&str> = pair.trim().split(':').collect();
            let temp_fan_pair = parse_temperature_fan_speed_parts(&parts)?;

            Ok(temp_fan_pair)
        })
        .collect()
}

pub fn check_temperature_fan_speed_pairs(
    pairs: &Vec<TempFanPair>,
) -> Result<(), String> {
    let num_pairs = pairs.len();

    if num_pairs == 0 {
        return Err(format!("No temperature fan speed pair provided"))
    }

    for i in 1..num_pairs {
        if pairs[i].temperature <= pairs[i - 1].temperature {
            return Err(format!("Temperature is not increasing"))
        }
        if pairs[i].fan_speed < pairs[i - 1].fan_speed {
            return Err(format!("Fan speed is decreasing"))
        }
    }
    Ok(())
}

pub fn interpolate_fan_speed(
    temp_fan_pairs: &Vec<TempFanPair>,
    current_temp: u32,
) -> u32 {
    let num_pairs = temp_fan_pairs.len();

    // Handle out-of-bounds temperatures and if there is only one temperature fan speed pair
    if current_temp <= temp_fan_pairs[0].temperature {
        return temp_fan_pairs[0].fan_speed;
    }
    if current_temp >= temp_fan_pairs[num_pairs - 1].temperature {
        return temp_fan_pairs[num_pairs - 1].fan_speed;
    }

    // Find the appropriate range for interpolation
    for i in 1..num_pairs {
        let lower = &temp_fan_pairs[i - 1];
        let upper = &temp_fan_pairs[i];

        if current_temp >= lower.temperature && current_temp <= upper.temperature {
            // Return the linearly interpolated fan speed
            return  ((lower.fan_speed as f32 * (upper.temperature as f32 - current_temp as f32) +
                    upper.fan_speed as f32 * (current_temp as f32 - lower.temperature as f32)) /
                    (upper.temperature as f32 - lower.temperature as f32)).round() as u32;
        }
    }
    
    // This point should never be reached
    return MAX_FAN_SPEED;
}