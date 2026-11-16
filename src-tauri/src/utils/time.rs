/// 统一的时间戳字符串格式（`YYYY-MM-DD HH:MM:SS`）。
///
/// 供 `ActiveModelBehavior::before_save` 与各 service 共用，确保同一套口径。
/// 用 `Local` 而非 `Utc`：与历史数据、smart_rule 的本地时间比较保持一致。
pub fn now_str() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}
