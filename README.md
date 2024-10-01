# NVIDIA-TUNER

A simple Rust CLI tool for overlocking, undervolting and controlling the fan of NVIDIA GPUs on Linux. Using the NVML library it equally supports X11 and Wayland.

## Features

* Set core clock offset.
* Set memory clock offset.
* Set power limit.
* Fan control using a custom linear fan curve.
* Use temperature hysteresis to prevent the fan from spinning up and down too frequently.
* Automatically set the fan control back to default on termination.

## Usage

**This tool is still under testing and it is impossible for me to guarantee that it works on every hardware, so use it at your own risk**

Show all possible options:

```bash
./nvidia-tuner --help
```

Usage example:
```bash
./nvidia-tuner ---core-clock-offset 150 --memory-clock-offset 800 --power-limit 180 --pairs 50:30,70:40,90:60,100:100
```

This command takes temperature and fan speed pairs as an argument. In this example the fan speed will be 30% up to 50°C and 100% above 100°C.
The fan speed between the given temperature and fan speed pairs is linearly interpolated to enable smooth transitions.

## Run on startup

1. Download the binary file from [the latest release](https://github.com/WickedLukas/nvidia-tuner/releases).
2. Copy it to `/usr/local/sbin/`.
3. Create the systemd service file `/etc/systemd/system/nvidia-tuner.service` with the following content:

```service
[Unit]
Description=nvidia-tuner
After=graphical.target

[Service]
Type=oneshot
ExecStart=/usr/local/sbin/nvidia-tuner ---core-clock-offset 150 --memory-clock-offset 800 --power-limit 180 --pairs 50:30,70:40,90:60,100:100
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
