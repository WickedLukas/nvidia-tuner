# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`nvidia-tuner` is a Linux-only Rust CLI that overclocks/undervolts (core and memory clock offsets), sets the power limit, and runs a custom fan curve on NVIDIA GPUs via NVML, so it works on both X11 and Wayland. It is usually run as a long-lived systemd service (see README.md).

## Commands

```bash
cargo build                 # debug build
cargo build --release       # release build (LTO enabled)
cargo clippy                # lint
cargo test                  # there are no tests yet; pure logic lives in src/utils.rs
cargo test <name>           # run a single test once tests exist
cargo run -- --help         # show CLI options
```

Running the binary for real needs an NVIDIA GPU, driver >= 520, and root. It re-execs itself through sudo, doas or pkexec if it is not already root, and it changes real hardware settings. Don't run it with tuning flags unless the user asks.

## Architecture

- `src/main.rs`: escalates privileges, initializes NVML, checks the driver version, applies one-shot settings (clock offsets, power limit), then, if `--pairs` is given, enters an endless fan-control loop with temperature hysteresis. The loop only calls the NVML setter when the interpolated fan speed changes.
- `src/nvml.rs`: thin wrappers around raw NVML C calls. NVML is used two ways:
  - `nvml_wrapper::Nvml` (safe API) only for init, driver version, getting the device handle, and the read-only `--show` output in `src/show.rs`.
  - `nvml_wrapper_sys::NvmlLib` loaded directly from `libnvidia-ml.so.1` for every actual get/set call (`nvmlDeviceSetGpcClkVfOffset`, `nvmlDeviceSetFanSpeed_v2`, etc.). The raw `nvmlDevice_t` is wrapped in `SafeNvmlDevice` (manually `Send`) inside `Arc<Mutex<…>>` so signal-handler threads can share it.
- `src/utils.rs`: parsing and validation of `temp:fan` pairs (temperatures must strictly increase, fan speeds must not decrease) and linear fan-speed interpolation.
- `src/show.rs`: `--show`, prints the current state and allowed ranges. It runs before privilege escalation and returns before cleanup is set up, because it changes nothing. clap rejects combining it with tuning options.
- `src/cli.rs`: clap derive definitions. `src/constants.rs`: limits (max temp 100°C, max fan 100%, max update period 10 s).

### Cleanup invariant (important)

Every setting the tool changes must be reset to its default on exit (not to the value it had before the tool started), and new features must follow this too. Settings the tool did not change are left untouched. `setup_cleanup` runs before any setting is changed. It installs a panic hook and signal-handler threads that call `reset_changed_settings`. `TunerState` records which settings were changed: clock offsets (with the NVML API used, reset to 0), the power limit (with the default limit, read via `nvmlDeviceGetPowerManagementDefaultLimit` before the change), the core clock lock (reset with `nvmlDeviceResetGpuLockedClocks`) and the fans (reset with `nvmlDeviceSetDefaultFanSpeed_v2`). Once `reset` is set, all setters do nothing, so the main loop can't override the reset settings. Device and state locks are always taken in the order device → state. Stop signals (SIGTSTP/SIGTTIN/SIGTTOU) are ignored so the process can't be frozen with modified settings.

Because settings are reset on exit, the process always keeps running (fan loop, or `thread::park` without `--pairs`) until it receives a termination signal. The fan loop tolerates `MAX_CONSECUTIVE_FAN_CONTROL_ERRORS` failed updates in a row and then panics, which resets everything. Errors in `main` are surfaced through `.unwrap()` on purpose, so they go through the panic hook.

## Conventions

- Fallible functions return `Result<_, String>`, and callers add context with `.map_err(|e| format!("Failed to …: {}", e))`.
- NVML return codes are converted with `check_result` in `src/nvml.rs`.
- Clock offsets use the per P-state API (`nvmlDeviceGetClockOffsets`/`nvmlDeviceSetClockOffsets`, driver >= 555) and set the offset on every supported P-state, which matches the behaviour of the legacy VF-offset API. Measured on an RTX 4070 with driver 615: the offset is one shared value per clock domain (writing a single P-state with either API changes every P-state and the legacy getter), both APIs use the same scale, and a memory offset of +50 raises the reported P0 memory clock by +25 MHz. Setting every P-state is kept anyway, in case other GPUs keep real per P-state offsets. If the new API is unavailable, they fall back to the legacy `nvmlDevice{Set,Get}{GpcClk,MemClk}VfOffset` calls, which are marked `TODO: Legacy API` and can be removed once drivers < 555 are dropped. Check that a function field on `NvmlLib` exists (`.is_ok()`) before calling a newer NVML function: calling a missing one panics.
