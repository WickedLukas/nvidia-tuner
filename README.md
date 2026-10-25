# NVIDIA-TUNER

A simple Rust CLI tool for overclocking, undervolting and controlling the fan of NVIDIA GPUs on Linux. Using the NVML library it equally supports X11 and Wayland.

## Features

* Set core clock offset.
* Set memory clock offset.
* Lock the maximum core clock for undervolting.
* Set power limit.
* Fan control using a custom linear fan curve.
* Use temperature hysteresis to prevent the fan from spinning up and down too frequently.
* Automatically reset all changed settings (clock offsets, locked core clock, power limit and fan control) to their defaults on termination.
* Keep fan control running if single temperature reads or fan speed updates fail.

## Usage

**This tool is still under testing and it is impossible for me to guarantee that it works on every hardware, so use it at your own risk**

Show all possible options:

```bash
./nvidia-tuner --help
```

All options can be combined. The tool keeps running until it is terminated (e.g. with Ctrl+C or `systemctl stop`) and then resets all settings it changed to their defaults: clock offsets to 0, the power limit to the default power limit of the GPU, and the locked core clock and fan control back to the driver. Settings which were not changed by the tool are left untouched.

The values below are examples. Stable offsets and clocks differ from GPU to GPU, so start with small values and increase them step by step.

### Custom fan curve

```bash
./nvidia-tuner --pairs 50:30,70:40,90:60,100:100
```

The pairs are `temperature:fan speed` in °C and %. In this example the fan speed is 30% up to 50°C and 100% at 100°C. Between the pairs the fan speed is linearly interpolated for smooth transitions. Fan speeds outside the range supported by the GPU (e.g. below 30% on many cards) are rejected.

### Overclocking

```bash
./nvidia-tuner --core-clock-offset 150 --memory-clock-offset 800
```

Raises the core and memory clocks by the given offsets in MHz.

### Undervolting

```bash
./nvidia-tuner --core-clock-offset 170 --max-core-clock 2600
```

The core clock offset shifts the voltage frequency curve, so each clock needs less voltage. Locking the maximum core clock keeps the GPU at or below that clock, which then runs at a lower voltage. The lowest supported clock is used as the minimum, so the GPU still clocks down in idle.

### Lower power consumption

```bash
./nvidia-tuner --power-limit 165
```

Limits the power consumption to the given value in W.

### Everything combined

```bash
./nvidia-tuner --core-clock-offset 170 --max-core-clock 2600 --memory-clock-offset 800 --power-limit 165 --pairs 50:30,70:40,90:60,100:100
```

## Run on startup

1. Download the binary file from [the latest release](https://github.com/WickedLukas/nvidia-tuner/releases).
2. Copy it to `/usr/local/sbin/`.
3. Create the systemd service file `/etc/systemd/system/nvidia-tuner.service` with the following content:

```service
[Unit]
Description=nvidia-tuner

[Service]
Type=exec
ExecStart=/usr/local/sbin/nvidia-tuner --core-clock-offset 170 --max-core-clock 2600 --memory-clock-offset 800 --power-limit 165 --pairs 50:30,70:40,90:60,100:100
Restart=on-failure
RestartSec=5
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=graphical.target
```

4. Reload the systemd manager configuration to recognize the new service:

```bash
sudo systemctl daemon-reload
```

5. Start the service:

```bash
sudo systemctl start nvidia-tuner.service
```

6. Enable the service to start automatically at boot:

```bash
sudo systemctl enable nvidia-tuner.service
```

7. Check the systemd journal for any errors:

```bash
sudo journalctl -u nvidia-tuner.service
```
