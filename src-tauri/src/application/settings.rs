//! 应用设置用例：写库 + 设置变更副作用。

use anyhow::Result;

use super::AppServices;
use crate::dto::app_setting_dto::SetSettingRequest;
use crate::services::app_setting_service;

pub async fn set_setting(svc: &AppServices<'_>, req: &SetSettingRequest) -> Result<()> {
    // 跨字段校验属于用例入口，不留给数据层。
    req.validate_value_format().map_err(anyhow::Error::msg)?;
    app_setting_service::set(&svc.ctx.db, &req.key, &req.value).await?;

    let mut sched = svc.scheduler.lock().await;
    sched
        .apply_setting_side_effect(&req.key, &req.value, req.monitor_id.as_deref())
        .await;
    Ok(())
}
