//! 热量意图律决策表（fork 新增，纯逻辑零依赖可测）。
//! 输入三信号（T 温度 / u 利用率=意图 / d̂ 两拍差分=势能）+ 上一拍 duty（唯一状态量）；
//! 输出本拍 duty。每拍全表重判、不平滑、不滞回（逐拍实时公理）；帧噪=一拍小偏、下拍自纠。
//! 与 gfd v1.8（箱扇域）共用同一决策表——两域唯一差异=参数域（见 card_defaults）。

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IntentCfg {
    pub free_lo: u32, // 自由区下沿（以下无散热义务）
    pub warn: u32,    // 预测性起势预警线
    pub free_hi: u32, // 自由区上沿（以上快降许可）
    pub floor: u32,   // 保底档（卡扇域=驱动锁 30，永不关死 A2）
    pub load_line: u32, // u 意图分界（≥ 即"满载意图"）
    pub slope_up: u32,  // 升斜率限 %/拍
    pub slope_down: u32, // 降斜率限 %/拍（中段退档用；收兵直落=floor 例外：撤干预后降温由物理曲线自动供给
}

impl IntentCfg {
    pub fn card_defaults() -> Self {
        // 卡扇域实例：地板=驱动锁 30，卡内直接执行器，响应可较箱扇域果断
        Self { free_lo: 50, warn: 65, free_hi: 70, floor: 30, load_line: 10, slope_up: 4, slope_down: 10 }
    }
}

/// 八格决策状态码（进日志，每拍动作可回放质询）
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum State {
    FastCool,    // ① u高 T≥hi：快降许可（唯一允许急降）
    Rise,        // ② u高 warn~hi d̂>0：预测性起势（提前量，缓升）
    HoldHigh,    // ②b u高 warn~hi d̂≤0：保持（不猛吹抢降温）
    Chase,       // ③ u高 lo~warn d̂>0：微升追平涨势
    Hold,        // ③b u高 lo~warn d̂≤0：保持（"活但升得慢→不需要过快降温"）
    IdleLow,     // ⑦ u高 T<lo：无义务保持
    Retreat,     // ④⑤⑥ u低：撤干预（直落 floor；"慢"由物理供给）
    Rest,        // ⑧ 待机
}

/// 实现注记：①格 boost=min(2·slope+超线量, slope)=恒 slope：卡域快降=受控连续爬升（积分器本质），"比例"语义由跨拍连续性承载；
/// slope_down 现生效域=非收兵降档（当前表内近空集）——两样均为设计定案（留观察期数据裁决）。
/// d̂ 单位：°C/拍（=°C/s 沿 -p 节拍；两拍差即得，无滤波）。0.3°C/拍 视为"不升"（温度整数读数的最小可辨升温的一半，防 ±1 量化噪声驱动 Chase）。
const DRIFT_EPS: i32 = 0; // 整数域：d̂>0 即升（量化噪声由 slope 与 5s 锚带自然钝化）

/// 决策核心：返回 (新 duty, 状态码)。duty_prev 初值=调用方读到的驱动 AUTO 现值。
pub fn decide(
    cfg: &IntentCfg,
    t_now: u32,
    t_prev: Option<u32>,
    u: u32,
    duty_prev: u32,
    duty_max: u32,
) -> (u32, State) {
    let busy = u >= cfg.load_line;
    let d_hat: i32 = t_prev.map(|p| t_now as i32 - p as i32).unwrap_or(0);
    let rising = d_hat > DRIFT_EPS;

    // 第一层：8 格定"意图动作"（raw），第二层统一收口（钳位+斜率）
    let (raw, state) = if busy {
        if t_now >= cfg.free_hi {
            // ① 快降许可：比例上量（超线几度多给档），封顶=4 倍升斜率
            let boost = (cfg.slope_up * 2 + (t_now - cfg.free_hi).min(cfg.slope_up * 4)).min(cfg.slope_up);
            (duty_prev.saturating_add(boost), State::FastCool)
        } else if t_now >= cfg.warn {
            if rising {
                // ② 起势：u 份额加权提前量（100% 满载给满斜率；半载半斜率）
                let gain = (cfg.slope_up * u / 100).max(1);
                (duty_prev.saturating_add(gain), State::Rise)
            } else {
                (duty_prev, State::HoldHigh)
            }
        } else if t_now >= cfg.free_lo {
            if rising {
                (duty_prev.saturating_add((cfg.slope_up / 2).max(1)), State::Chase)
            } else {
                (duty_prev, State::Hold)
            }
        } else {
            (duty_prev, State::IdleLow)
        }
    } else if t_now >= cfg.free_lo {
        (cfg.floor, State::Retreat) // ④⑤⑥ 撤干预直落地板（"慢"归物理）
    } else {
        (cfg.floor, State::Rest) // ⑧
    };

    // 收口层：包络钳位；升向斜率限；降向——收兵/待机（floor 直落）=斜率限的地板例外，
    // 其余降档（如帧噪后的回稳不在此表内）按 slope_down 限速。
    let mut duty = raw.clamp(cfg.floor, duty_max);
    if duty > duty_prev + cfg.slope_up { duty = duty_prev + cfg.slope_up; }
    let floor_drop = matches!(state, State::Retreat | State::Rest);
    if !floor_drop && duty + cfg.slope_down < duty_prev { duty = duty_prev - cfg.slope_down; }
    (duty, state)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cfg() -> IntentCfg { IntentCfg::card_defaults() }

    #[test]
    fn fast_cool_climbs_by_proportion() {
        // ① 72°C 满载 → 快降：封顶=升斜率（连续拍爬到位，止涨而非瞬降）
        let (d, s) = decide(&cfg(), 72, Some(71), 100, 30, 100);
        assert_eq!(s, State::FastCool);
        assert_eq!(d, 34); // 30+slope_up*1（收口斜率钳）
    }
    #[test]
    fn rise_gives_early_ramp() {
        // ② 68°C×满载×升温 → 提前量缓升（不待破 70；u 加权，封顶=slope_up）
        let (d, s) = decide(&cfg(), 68, Some(67), 100, 30, 100);
        assert_eq!(s, State::Rise);
        assert_eq!(d, 30 + 4); // u=100：gain 封顶=slope_up
        let (d2, _) = decide(&cfg(), 68, Some(67), 50, 30, 100);
        assert_eq!(d2, 30 + 2); // 半载半斜率（比例语义）
        let (d3, s3) = decide(&cfg(), 68, Some(67), 5, 30, 100); // u<load_line=非意图帧
        assert_eq!(s3, State::Retreat); // 正确语义：无意图不加热（收兵格）
        assert_eq!(d3, cfg().floor);
        let (d4, _) = decide(&cfg(), 68, Some(67), 15, 30, 100); // 刚跨分界的低份额
        assert_eq!(d4, 31); // gain=max(4·15/100=0,1)=1（缓升不停摆）
    }
    #[test]
    fn free_zone_hot_but_steady_holds() {
        // ③/②b "合理转速保持，不抢降温"（设计条款：u=100 意图在场，等温/降温均保持）
        assert_eq!(decide(&cfg(), 66, Some(66), 100, 45, 100).1, State::HoldHigh);
        assert_eq!(decide(&cfg(), 66, Some(67), 100, 45, 100).1, State::HoldHigh); // 降温方向更不抢
        assert_eq!(decide(&cfg(), 55, Some(55), 100, 40, 100).1, State::Hold);
    }
    #[test]
    fn chase_only_when_rising_slow() {
        let (d, s) = decide(&cfg(), 55, Some(54), 100, 30, 100);
        assert_eq!(s, State::Chase);
        assert_eq!(d, 32); // slope_up/2
    }
    #[test]
    fn idle_load_below_zone_holds() {
        // ⑦ 满载但凉（活不到热）：保持、不加无谓风
        let (d, s) = decide(&cfg(), 45, Some(45), 100, 35, 100);
        assert_eq!(s, State::IdleLow);
        assert_eq!(d, 35);
    }
    #[test]
    fn retreat_drops_to_floor_immediately() {
        // ④⑤⑥ 收兵即落（物理接管降温；地板例外不受 slope_down 限速）
        let (d, s) = decide(&cfg(), 68, Some(69), 0, 60, 100);
        assert_eq!(s, State::Retreat);
        assert_eq!(d, cfg().floor);
        let (d8, s8) = decide(&cfg(), 40, Some(41), 0, 30, 100);
        assert_eq!(s8, State::Rest);
        assert_eq!(d8, cfg().floor);
    }
    #[test]
    fn never_exceeds_max_and_frame_noise_self_heals() {
        // 钳位：连升不越 100；帧噪（单帧 u=0）→ 一拍落 floor、下拍可回（无宏观害）
        let mut d = 90u32;
        for _ in 0..40 { d = decide(&cfg(), 85, Some(84), 100, d, 100).0; }
        assert!(d <= 100);
        let (dn, _) = decide(&cfg(), 71, Some(72), 0, 100, 100); // 单帧 u 落
        assert_eq!(dn, cfg().floor);
        let (back, _) = decide(&cfg(), 72, Some(71), 100, dn, 100); // 下拍回拉
        assert!(back > dn);
    }
    #[test]
    fn envelope_floor_and_zone_constants_hold() {
        let c = cfg();
        assert!(c.free_lo < c.warn && c.warn < c.free_hi);
        assert!(c.slope_up > 0 && c.slope_down > 0);
    }
    #[test]
    fn rest_never_wakes_without_intent() {
        // A1 守恒：u=0 时无论温度多高必 Rest/Retreat 且落地板——"满载必升、空载必降"的机器化守卫
        for t in [55u32, 66, 71, 85] {
            let (d, s) = decide(&cfg(), t, Some(t), 0, 50, 100);
            assert!(matches!(s, State::Retreat | State::Rest), "u=0 态强制收兵: t={t} s={s:?}");
            assert!(d <= cfg().free_lo.max(cfg().floor), "空载回地板: d={d}");
        }
    }
}
