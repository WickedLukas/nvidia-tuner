# nvidia-tuner (gfd fork) — English

> Readme language index: [中文（主）](README.md) | **English**

Fork of [nvidia-tuner](https://github.com/WickedLukas/nvidia-tuner) (MIT) adding the **heat-intent fan law**. Design concept in one line: **utilization = intent, temperature = state, temperature derivative = momentum** — the fan is actuated to keep temperature from changing abruptly (thermal-shock protection of solder joints outranks "as cool as possible"). Retains the upstream linear fan-curve mode verbatim; `--fan-mode curve|intent` switches between them.

Releases ship two single-file binaries: dynamic (glibc >= 2.39) and fully static (any x86_64 Linux), with sha256 sums.

## Quick start

```bash
# intent mode (--pairs still required: range pre-check & upgrade path)
nvidia-tuner --fan-mode intent --pairs 40:30,85:90

# tunables (all defaulted): --intent-free-lo --intent-warn --intent-free-hi
#                           --intent-floor --intent-load-line --intent-slope-up --intent-slope-down

# upstream behaviour (default)
nvidia-tuner --fan-mode curve --pairs 40:30,60:45,70:65,85:90
```

Every decision tick emits one log line `t=68 u=100 duty=34 state=Rise` — every action is replayable and auditable against the decision table. NVML-based; X11 and Wayland alike; systemd unit example in the [Chinese README](README.md#部署systemd).

## Behaviour summary

* Busy intent (`u ≥ load_line`): fast cool above free-zone top; pre-emptive proportional ramp near the warning line (ahead of the threshold, not after); hold otherwise — no over-cooling.
* Idle intent: duty drops straight to the floor (fan never stops) — the *slow* cool-down comes from physics (natural convection ∝ temperature difference), not from a scripted decay.
* Startup gatekeeper rejects invalid configs before touching hardware; on consecutive NVML failures the tool panics and resets fans to driver AUTO (upstream fail-safe, inherited).

See `CHANGELOG.md` for version history; `NOTICE` for attribution (upstream © WickedLukas; fork implementation by DeepSeek Harness, all design concepts provided by the user).

---

*Original upstream readme (nvidia-tuner 0.6.0, English) is preserved in full inside [README.md → "Original (upstream README, preserved)"](README.md#original-upstream-readme-preserved).*
