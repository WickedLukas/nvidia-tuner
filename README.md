# NVIDIA-TUNER (gfd)

> 语言索引：**中文（主）** | [English](README.en.md)

# NVIDIA-TUNER (gfd)

**Linux 下的 NVIDIA 显卡命令行调校工具**：超频、降压、限功耗，风扇控制支持两种模式——上游线性风扇曲线，与本 fork 新增的**热量意图律**。基于 NVML，X11 / Wayland 通吃。通常以 systemd 常驻服务运行（见下"部署"）。

## 热量意图律（本 fork 核心，`--fan-mode intent`）

三条输入、一张八格决策表、逐拍重判：

| 输入 | 角色 | 公理 |
|---|---|---|
| 利用率 u | **意图** | 满载温度必升，空载温度必降 |
| 温度 T | 状态 | 70°C 以内无需强力散热 |
| 温度导数 dT | 势能 | 预警区+升温=提前加风（起势） |

目标函数 = **保持温度不急剧变化**（热冲击对焊点的伤害大于适度高温）。因此：升载用斜率限连续加风压峰值；撤载**撤销干预直落地板档**——"降温慢"不靠程序演，交还给物理（温差衰减的自然对流自动把降温曲线拉缓）。地板档永不关死：热量必散尽，只存在"多快散完"。

```bash
nvidia-tuner --fan-mode intent --pairs 40:30,85:90          # --pairs 仍必填（范围预检/回退位）
# 业务调参（全有缺省）：--intent-free-lo/--intent-warn/--intent-free-hi
#                      --intent-floor/--intent-load-line/--intent-slope-up/--intent-slope-down
nvidia-tuner --fan-mode curve --pairs 40:30,60:45,70:65,85:90   # 上游行为，一键回退
```

每拍决策日志 `t=68 u=100 duty=34 state=Rise`——动作全可回放质询；连续失败自动交还驱动 AUTO（上游 fail-safe 面完整继承，坏配置在启动校验即拒）。

## 功能一览

* 设置核心/显存频率偏移（超频/降压）。
* 锁定最高核心频率（配合偏移实现降压）。
* 设置功耗墙。
* 风扇控制：线性曲线（温度区间自动插值 + 可选温度滞回）**或热量意图律**（见上）。
* `--show`：只读打印当前设置与允许范围。
* 权限自举（sudo/doas/pkexec）；全部改动在退出时**重置为默认**而非启动前值（上游纪律，本 fork 全部新功能遵守）。

## 部署（systemd）

```ini
[Unit]
Description=nvidia-tuner
After=multi-user.target

[Service]
Type=simple
ExecStart=/usr/local/sbin/nvidia-tuner --fan-mode intent --pairs 40:30,60:45,70:65,85:90
Restart=always

[Install]
WantedBy=multi-user.target
```

## 下载与构建

**免环境直取**：Releases 页提供单个全静态二进制 `nvidia-tuner-linux-x86_64-static`（任意 x86_64 Linux 通吃，不挑发行版/glibc），附 sha256；`chmod +x` 即用。

## 构建

```bash
cargo build --release
```

要求：Linux、NVIDIA 驱动 ≥ 520。`--show` 外的一切调校参数都会改真实硬件设置。

---

# Original (upstream README, preserved)

A simple Rust CLI tool for overclocking, undervolting and controlling the fan of NVIDIA GPUs on Linux. Using the NVML library it equally supports X11 and Wayland.

## Features

* Set core clock offset.
* Set memory clock offset.
* Lock the maximum core clock for undervolting.
* Set power limit.
* Fan control using a custom linear fan curve.
* Use temperature hysteresis to prevent the fan from spinning up and down too frequently.
* Use `--show` to get the current settings and allowed ranges without changing anything.

The fan curve is defined by temperature and fan speed pairs, in the format `temp1:fan1,temp2:fan2,...`. Between these points the fan speed is linearly interpolated. Temperatures must be strictly increasing, fan speeds must be non-decreasing, all values must be within the limits supported by your GPU.

## Usage

Run `nvidia-tuner --help` or see the options table above. The tool re-execs itself through sudo, doas or pkexec if not run as root, and resets all settings it changed to their defaults on exit.

---

## 实现署名

本 fork 的编码实现由 **DeepSeek Harness（dsh）** 完成，**用户提供全部设计思路与控制概念**（热量意图律六公理与八格表的判词均出自人工裁定）；上游 nvidia-tuner 由 Lukas（WickedLukas）开发，MIT 许可，版权与致谢见 `LICENSE` 与 `NOTICE`。
