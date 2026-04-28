use serde::Deserialize;
use std::ops::Deref;

/// 自动校验的包装类型
///
/// 在 Tauri command 参数反序列化阶段自动触发 garde 校验，
/// 校验失败时直接返回反序列化错误，无需在 command 函数体内手动调用 validate()。
///
/// # 用法
/// ```rust
/// #[tauri::command]
/// pub async fn create_collection(
///     db: State<'_, DatabaseConnection>,
///     req: Validated<CreateCollectionRequest>,
/// ) -> Result<collection::Model, String> {
///     // 方式1：通过 Deref 直接访问字段（只读）
///     let name = &req.name;
///
///     // 方式2：获取所有权后使用
///     let req = req.into_inner();
///     service::create(db.inner(), req.name).await
/// }
/// ```
pub struct Validated<T>(pub T);

impl<T> Validated<T> {
    /// 消费 wrapper，返回内部已校验的值
    pub fn into_inner(self) -> T {
        self.0
    }
}

/// 通过 Deref 实现 `req.field_name` 直接访问内部字段（只读）
impl<T> Deref for Validated<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de, T> Deserialize<'de> for Validated<T>
where
    T: Deserialize<'de> + garde::Validate,
    <T as garde::Validate>::Context: Default,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let inner = T::deserialize(deserializer)?;
        inner
            .validate()
            .map_err(|e| serde::de::Error::custom(e.to_string()))?;
        Ok(Validated(inner))
    }
}
