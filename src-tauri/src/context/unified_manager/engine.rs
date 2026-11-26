//! 配置快照与差异计算（纯逻辑，无 IO）
//!
//! 架构重整 Phase 1：一个 action 事务提交后，Manager 用本模块比较
//! 「已成功应用的状态（observed）」与「DB 当前状态（desired）」，只派发差异项。
//! 派发成功的部分才推进 observed，失败项保持旧值，下一次 reconcile 自动重放——
//! 这是取消常驻轮询后唯一的重试来源，因此 patch 必须按差异项局部推进。

use std::collections::{BTreeMap, BTreeSet};

/// 与 `DisplayMode::Independent.as_str()` 一致；放在这里是为了让快照层
/// 不反向依赖渲染契约，也避免每处 Default 各写一份字面量。
pub const DEFAULT_DISPLAY_MODE: &str = "independent";

/// `monitor_configs` 表中**影响壁纸窗口**的字段快照。
///
/// 只保留 3 个字段，理由有两类：
/// - `id` / `updated_at`：代理主键与时间戳，变化不代表渲染状态变化，纳入会产出伪差异。
/// - `collection_id` / `play_mode` / `play_interval` / `is_enabled`：只服务轮播定时器，
///   由 TaskRegistry 现有路径消费（`manage_carousel_timer` / `should_start_timer`），
///   且 `carousel.rs` 每次 tick 动态重读 config，不产生任何窗口动作。
///   轮播换图最终写的是 `wallpaper_id`，仍会被本快照捕获。
/// 本次重整的范围是「窗口调整相关副作用」，timer 不在其中，不进快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorSnapshot {
    pub wallpaper_id: Option<i32>,
    pub fit_mode: String,
    pub active: bool,
}

/// 单屏快照里的可比较字段。
///
/// diff 只描述「哪个字段变了」，派发时统一读 desired 的新值，避免为
/// `Option<i32>` / `String` / `bool` 等不同字段类型构造一个值枚举。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MonitorField {
    WallpaperId,
    FitMode,
    Active,
}

/// `app_setting` 白名单里影响渲染的全局字段。
///
/// 其余 key（theme / shortcut_* / 回收站等）不进快照：它们要么无渲染副作用，
/// 要么归 TaskRegistry 自己管理，混进来只会产出无意义的 diff。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalSnapshot {
    pub display_mode: String,
    pub volume: u32,
}

impl Default for GlobalSnapshot {
    fn default() -> Self {
        Self {
            display_mode: DEFAULT_DISPLAY_MODE.to_string(),
            volume: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConfigSnapshot {
    pub monitors: BTreeMap<String, MonitorSnapshot>,
    pub global: GlobalSnapshot,
}

/// reconcile 的作用范围，由 action 事务实际写入的 monitor_id 集合推导。
///
/// 让事实源落在事务上而非 action 硬编码：mirror/extend 下 `wallpaper_id`
/// 看似单屏改动，实际会同步写 N 条 config，scope 自然变成组级。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconcileScope {
    One(String),
    Many(Vec<String>),
    All,
}

impl ReconcileScope {
    /// 事务写了 0 条（纯 app_setting 变更）或含全局键变更 → `All`；
    /// 1 条 → `One`；多条 → `Many`。排序去重保证派发顺序可预测。
    pub fn from_written_monitors(mut ids: Vec<String>, global_changed: bool) -> Self {
        if global_changed || ids.is_empty() {
            return Self::All;
        }
        ids.sort();
        ids.dedup();
        if ids.len() == 1 {
            Self::One(ids.remove(0))
        } else {
            Self::Many(ids)
        }
    }

    fn covers(&self, monitor_id: &str) -> bool {
        match self {
            Self::All => true,
            Self::One(id) => id == monitor_id,
            Self::Many(ids) => ids.iter().any(|id| id == monitor_id),
        }
    }

    /// 只有全局 action 才需要比较 display_mode / volume。
    /// `Many` 一定来自「写 N 条 config 但没改全局键」，故不产出全局差异。
    fn is_global(&self) -> bool {
        matches!(self, Self::All)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigDiff {
    MonitorAdded {
        monitor_id: String,
    },
    MonitorRemoved {
        monitor_id: String,
    },
    /// 同一屏的多个字段变化合并成一项，派发层按 fields 逐项执行。
    MonitorChanged {
        monitor_id: String,
        fields: Vec<MonitorField>,
    },
    /// 只描述模式变化本身。按 independent / mirror / extend 的整段编排由派发层执行，
    /// 这里**不**重复产出 fit_mode 等字段动作，否则同一动作会被派发两次。
    DisplayModeChanged {
        from: String,
        to: String,
    },
    VolumeChanged {
        from: u32,
        to: u32,
    },
}

fn changed_fields(old: &MonitorSnapshot, new: &MonitorSnapshot) -> Vec<MonitorField> {
    use MonitorField::*;
    let mut fields = Vec::new();
    if old.wallpaper_id != new.wallpaper_id {
        fields.push(WallpaperId);
    }
    if old.fit_mode != new.fit_mode {
        fields.push(FitMode);
    }
    if old.active != new.active {
        fields.push(Active);
    }
    fields
}

/// 比较 observed 与 desired，产出 scope 内的全部差异。
///
/// 用 BTreeSet 收集 id 保证输出顺序稳定，便于单测断言与派发可预测；
/// 新增/删除/变更合并成每个 monitor 至多一项，天然支持「一次 diff 覆盖 N 屏」。
pub fn diff_snapshots(
    observed: &ConfigSnapshot,
    desired: &ConfigSnapshot,
    scope: &ReconcileScope,
) -> Vec<ConfigDiff> {
    let mut diffs = Vec::new();

    let mut ids: BTreeSet<String> = BTreeSet::new();
    for id in observed.monitors.keys().chain(desired.monitors.keys()) {
        if scope.covers(id) {
            ids.insert(id.clone());
        }
    }

    for id in ids {
        match (observed.monitors.get(&id), desired.monitors.get(&id)) {
            (None, Some(_)) => diffs.push(ConfigDiff::MonitorAdded { monitor_id: id }),
            (Some(_), None) => diffs.push(ConfigDiff::MonitorRemoved { monitor_id: id }),
            (Some(old), Some(new)) => {
                let fields = changed_fields(old, new);
                if !fields.is_empty() {
                    diffs.push(ConfigDiff::MonitorChanged {
                        monitor_id: id,
                        fields,
                    });
                }
            }
            // id 来自两侧 key 的并集，不可能两边都缺。
            (None, None) => {}
        }
    }

    if scope.is_global() {
        if observed.global.display_mode != desired.global.display_mode {
            diffs.push(ConfigDiff::DisplayModeChanged {
                from: observed.global.display_mode.clone(),
                to: desired.global.display_mode.clone(),
            });
        }
        if observed.global.volume != desired.global.volume {
            diffs.push(ConfigDiff::VolumeChanged {
                from: observed.global.volume,
                to: desired.global.volume,
            });
        }
    }

    diffs
}

/// 派发动作：reconcile 把 diff 翻译成这些动作再交给 Manager 执行。
///
/// 单独抽出来是为了让「diff → 动作」的映射可脱离 Windows 单测：
/// 真正的副作用（renderer / WebView）只在 Manager 侧执行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchAction {
    EnsureTarget {
        monitor_id: String,
        wallpaper_id: Option<i32>,
        fit_mode: String,
    },
    DestroyTarget {
        monitor_id: String,
    },
    UpdateWallpaper {
        monitor_id: String,
        wallpaper_id: i32,
    },
    ClearWallpaper {
        monitor_id: String,
    },
    SetFitMode {
        monitor_id: String,
        fit_mode: String,
    },
    SetDisplayMode {
        mode: String,
    },
    SetVolume {
        volume: u32,
    },
}

/// 把 diff 列表转成派发动作列表。
///
/// 顺序固定：先 set_fit_mode（换图前的样式），再壁纸变更，最后 active 翻转
/// （销毁/创建目标）——与旧 command 内联副作用的顺序一致，避免过渡帧样式跳变。
pub fn plan_dispatch(diffs: &[ConfigDiff], desired: &ConfigSnapshot) -> Vec<DispatchAction> {
    use DispatchAction::*;
    let mut actions = Vec::new();

    for diff in diffs {
        match diff {
            ConfigDiff::MonitorAdded { monitor_id } => {
                let Some(config) = desired.monitors.get(monitor_id) else {
                    continue;
                };
                // 新屏一律走 EnsureTarget：它内部会同时登记几何、按 capability
                // 选 backend 并渲染首帧，不需要再叠加 SetFitMode / UpdateWallpaper。
                actions.push(EnsureTarget {
                    monitor_id: monitor_id.clone(),
                    wallpaper_id: config.wallpaper_id,
                    fit_mode: config.fit_mode.clone(),
                });
            }
            ConfigDiff::MonitorChanged { monitor_id, fields } => {
                let Some(config) = desired.monitors.get(monitor_id) else {
                    continue;
                };
                // 样式先于壁纸：换图时先按新 fit_mode 建目标，避免过渡帧样式跳变。
                if fields.contains(&MonitorField::FitMode) {
                    actions.push(SetFitMode {
                        monitor_id: monitor_id.clone(),
                        fit_mode: config.fit_mode.clone(),
                    });
                }
                if fields.contains(&MonitorField::WallpaperId) {
                    match config.wallpaper_id {
                        Some(wallpaper_id) => actions.push(UpdateWallpaper {
                            monitor_id: monitor_id.clone(),
                            wallpaper_id,
                        }),
                        None => actions.push(ClearWallpaper {
                            monitor_id: monitor_id.clone(),
                        }),
                    }
                }
                if fields.contains(&MonitorField::Active) {
                    if config.active {
                        actions.push(EnsureTarget {
                            monitor_id: monitor_id.clone(),
                            wallpaper_id: config.wallpaper_id,
                            fit_mode: config.fit_mode.clone(),
                        });
                    } else {
                        actions.push(DestroyTarget {
                            monitor_id: monitor_id.clone(),
                        });
                    }
                }
            }
            ConfigDiff::MonitorRemoved { monitor_id } => actions.push(DestroyTarget {
                monitor_id: monitor_id.clone(),
            }),
            ConfigDiff::DisplayModeChanged { to, .. } => {
                actions.push(SetDisplayMode { mode: to.clone() })
            }
            ConfigDiff::VolumeChanged { to, .. } => actions.push(SetVolume { volume: *to }),
        }
    }

    actions
}

/// 只推进「已成功应用」的差异项到 observed。
///
/// 绝不整体覆盖：scoped reconcile 若整体替换快照，会抹掉其它屏已确认的状态；
/// 派发失败项不在 `applied` 里，保持旧值，下一次 diff 会重新产出它（C2 重试语义）。
pub fn patch_observed(
    observed: &mut ConfigSnapshot,
    desired: &ConfigSnapshot,
    applied: &[ConfigDiff],
) {
    for diff in applied {
        match diff {
            ConfigDiff::MonitorAdded { monitor_id }
            | ConfigDiff::MonitorChanged { monitor_id, .. } => {
                if let Some(new) = desired.monitors.get(monitor_id) {
                    observed.monitors.insert(monitor_id.clone(), new.clone());
                }
            }
            ConfigDiff::MonitorRemoved { monitor_id } => {
                observed.monitors.remove(monitor_id);
            }
            ConfigDiff::DisplayModeChanged { to, .. } => {
                observed.global.display_mode = to.clone();
            }
            ConfigDiff::VolumeChanged { to, .. } => {
                observed.global.volume = *to;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(wid: Option<i32>) -> MonitorSnapshot {
        MonitorSnapshot {
            wallpaper_id: wid,
            fit_mode: "cover".to_string(),
            active: true,
        }
    }

    fn snapshot(ids: &[&str], wid: Option<i32>) -> ConfigSnapshot {
        ConfigSnapshot {
            monitors: ids
                .iter()
                .map(|id| (id.to_string(), monitor(wid)))
                .collect(),
            global: GlobalSnapshot::default(),
        }
    }

    #[test]
    fn changed_fields_covers_all_window_fields() {
        let old = monitor(Some(1));
        let mut new = old.clone();
        new.wallpaper_id = Some(2);
        new.fit_mode = "contain".to_string();
        new.active = false;

        assert_eq!(
            changed_fields(&old, &new),
            vec![
                MonitorField::WallpaperId,
                MonitorField::FitMode,
                MonitorField::Active,
            ]
        );
    }

    #[test]
    fn carousel_next_wallpaper_is_captured_via_wallpaper_id() {
        // 轮播 tick 最终写的是 wallpaper_id。timer 字段不进快照，
        // 窗口更新依然会被 diff 捕获，证明 3 字段不会漏掉轮播换图。
        let observed = snapshot(&["DISPLAY1"], Some(1));
        let desired = snapshot(&["DISPLAY1"], Some(2));
        let diffs = diff_snapshots(
            &observed,
            &desired,
            &ReconcileScope::One("DISPLAY1".to_string()),
        );
        assert_eq!(
            diffs,
            vec![ConfigDiff::MonitorChanged {
                monitor_id: "DISPLAY1".to_string(),
                fields: vec![MonitorField::WallpaperId],
            }]
        );
    }

    #[test]
    fn scope_one_ignores_other_monitors_and_global_fields() {
        let mut observed = snapshot(&["DISPLAY1", "DISPLAY2"], Some(1));
        observed.global.volume = 10;
        let mut desired = observed.clone();
        desired.monitors.get_mut("DISPLAY1").unwrap().wallpaper_id = Some(2);
        desired.monitors.get_mut("DISPLAY2").unwrap().wallpaper_id = Some(3);
        desired.global.volume = 80;
        desired.global.display_mode = "extend".to_string();

        let diffs = diff_snapshots(
            &observed,
            &desired,
            &ReconcileScope::One("DISPLAY1".to_string()),
        );
        assert_eq!(
            diffs,
            vec![ConfigDiff::MonitorChanged {
                monitor_id: "DISPLAY1".to_string(),
                fields: vec![MonitorField::WallpaperId],
            }]
        );
    }

    #[test]
    fn batch_scope_reports_all_monitors_in_one_call() {
        let observed = snapshot(&["DISPLAY1", "DISPLAY2", "DISPLAY3"], Some(1));
        let desired = snapshot(&["DISPLAY1", "DISPLAY2", "DISPLAY3"], Some(9));
        let scope = ReconcileScope::Many(vec![
            "DISPLAY1".to_string(),
            "DISPLAY2".to_string(),
            "DISPLAY3".to_string(),
        ]);

        let diffs = diff_snapshots(&observed, &desired, &scope);
        assert_eq!(diffs.len(), 3);
        for diff in &diffs {
            match diff {
                ConfigDiff::MonitorChanged { fields, .. } => {
                    assert_eq!(fields, &vec![MonitorField::WallpaperId]);
                }
                other => panic!("unexpected diff: {:?}", other),
            }
        }
    }

    #[test]
    fn global_action_keeps_mode_change_separate_from_field_actions() {
        let observed = snapshot(&["DISPLAY1", "DISPLAY2"], Some(1));
        let mut desired = observed.clone();
        desired.global.display_mode = "extend".to_string();
        desired.global.volume = 55;
        for config in desired.monitors.values_mut() {
            config.fit_mode = "fill".to_string();
        }

        let diffs = diff_snapshots(&observed, &desired, &ReconcileScope::All);
        // 2 个字段差异 + 1 个模式 + 1 个音量：模式变化不额外产出 set_fit_mode。
        assert_eq!(diffs.len(), 4);
        assert!(diffs.iter().any(|d| matches!(
            d,
            ConfigDiff::DisplayModeChanged { to, .. } if to == "extend"
        )));
        assert!(diffs
            .iter()
            .any(|d| matches!(d, ConfigDiff::VolumeChanged { to: 55, .. })));
        assert_eq!(
            diffs
                .iter()
                .filter(|d| matches!(d, ConfigDiff::MonitorChanged { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn detects_added_and_removed_monitors() {
        let observed = snapshot(&["DISPLAY1"], Some(1));
        let desired = snapshot(&["DISPLAY2"], Some(1));
        let diffs = diff_snapshots(&observed, &desired, &ReconcileScope::All);
        assert_eq!(
            diffs,
            vec![
                ConfigDiff::MonitorRemoved {
                    monitor_id: "DISPLAY1".to_string()
                },
                ConfigDiff::MonitorAdded {
                    monitor_id: "DISPLAY2".to_string()
                },
            ]
        );
    }

    #[test]
    fn patch_only_advances_applied_diffs_so_failed_ones_replay() {
        let observed = snapshot(&["DISPLAY1", "DISPLAY2"], Some(1));
        let desired = snapshot(&["DISPLAY1", "DISPLAY2"], Some(9));
        let scope = ReconcileScope::All;
        let diffs = diff_snapshots(&observed, &desired, &scope);
        assert_eq!(diffs.len(), 2);

        // 模拟 DISPLAY2 派发失败：只把第一项标记为已应用。
        let mut current = observed.clone();
        patch_observed(&mut current, &desired, &diffs[..1]);

        let replay = diff_snapshots(&current, &desired, &scope);
        assert_eq!(
            replay,
            vec![ConfigDiff::MonitorChanged {
                monitor_id: "DISPLAY2".to_string(),
                fields: vec![MonitorField::WallpaperId],
            }]
        );
    }

    #[test]
    fn patch_does_not_touch_monitors_outside_applied_diffs() {
        let observed = snapshot(&["DISPLAY1", "DISPLAY2"], Some(1));
        let mut desired = observed.clone();
        for config in desired.monitors.values_mut() {
            config.wallpaper_id = Some(9);
        }
        let scope = ReconcileScope::One("DISPLAY1".to_string());
        let diffs = diff_snapshots(&observed, &desired, &scope);

        let mut current = observed.clone();
        patch_observed(&mut current, &desired, &diffs);
        assert_eq!(current.monitors["DISPLAY1"].wallpaper_id, Some(9));
        assert_eq!(current.monitors["DISPLAY2"].wallpaper_id, Some(1));
    }

    #[test]
    fn dispatch_plan_maps_wallpaper_and_fit_mode_in_order() {
        let observed = snapshot(&["DISPLAY1"], Some(1));
        let mut desired = observed.clone();
        desired.monitors.get_mut("DISPLAY1").unwrap().wallpaper_id = Some(2);
        desired.monitors.get_mut("DISPLAY1").unwrap().fit_mode = "fill".to_string();
        let diffs = diff_snapshots(&observed, &desired, &ReconcileScope::All);

        assert_eq!(
            plan_dispatch(&diffs, &desired),
            vec![
                DispatchAction::SetFitMode {
                    monitor_id: "DISPLAY1".to_string(),
                    fit_mode: "fill".to_string(),
                },
                DispatchAction::UpdateWallpaper {
                    monitor_id: "DISPLAY1".to_string(),
                    wallpaper_id: 2,
                },
            ]
        );
    }

    #[test]
    fn dispatch_plan_for_added_monitor_is_single_ensure_target() {
        let observed = ConfigSnapshot::default();
        let desired = snapshot(&["DISPLAY1"], Some(7));
        let diffs = diff_snapshots(&observed, &desired, &ReconcileScope::All);
        assert_eq!(
            plan_dispatch(&diffs, &desired),
            vec![DispatchAction::EnsureTarget {
                monitor_id: "DISPLAY1".to_string(),
                wallpaper_id: Some(7),
                fit_mode: "cover".to_string(),
            }]
        );
    }

    #[test]
    fn dispatch_plan_keeps_global_actions_separate() {
        let observed = snapshot(&["DISPLAY1"], Some(1));
        let mut desired = observed.clone();
        desired.global.display_mode = "extend".to_string();
        desired.global.volume = 30;
        let diffs = diff_snapshots(&observed, &desired, &ReconcileScope::All);
        assert_eq!(
            plan_dispatch(&diffs, &desired),
            vec![
                DispatchAction::SetDisplayMode {
                    mode: "extend".to_string()
                },
                DispatchAction::SetVolume { volume: 30 },
            ]
        );
    }

    #[test]
    fn scope_is_derived_from_transaction_writes() {
        assert_eq!(
            ReconcileScope::from_written_monitors(vec![], true),
            ReconcileScope::All
        );
        assert_eq!(
            ReconcileScope::from_written_monitors(vec![], false),
            ReconcileScope::All
        );
        assert_eq!(
            ReconcileScope::from_written_monitors(vec!["DISPLAY2".to_string()], false),
            ReconcileScope::One("DISPLAY2".to_string())
        );
        assert_eq!(
            ReconcileScope::from_written_monitors(
                vec![
                    "DISPLAY3".to_string(),
                    "DISPLAY1".to_string(),
                    "DISPLAY3".to_string()
                ],
                false
            ),
            ReconcileScope::Many(vec!["DISPLAY1".to_string(), "DISPLAY3".to_string()])
        );
    }
}
