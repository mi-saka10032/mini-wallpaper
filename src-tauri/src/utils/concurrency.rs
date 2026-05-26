/// 根据当前 CPU 逻辑核数动态计算导入并发数
///
/// 策略：核数 × 75%，下限 2，上限 8
/// - 75% 留余量给 UI 渲染线程、Tauri IPC、SQLite 后台线程
/// - 下限 2 保证最低吞吐
/// - 上限 8 防止高核数机器磁盘 I/O 排队 + 内存峰值过高
pub fn import_concurrency() -> usize {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    (cores * 3 / 4).max(2).min(8)
}
