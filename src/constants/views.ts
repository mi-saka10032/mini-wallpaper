/**
 * 侧栏视图标识（activeId）约定
 *
 * `activeId` 是单一数值，用于表达当前主内容区展示哪个视图：
 * - `0`  ：全部壁纸（本地壁纸库）
 * - `> 0`：对应 id 的收藏夹（手动 / 智能）
 * - 负数 ：内置的特殊视图，用哨兵常量表达，避免与收藏夹 id 冲突
 *
 * 新增特殊视图时在此追加常量，不要在组件里散落魔法数字。
 */

/** 全部壁纸（本地壁纸库） */
export const LIBRARY_VIEW_ID = 0;

/** 显示器设置面板 */
export const SETTINGS_VIEW_ID = -1;

/** 回收站 */
export const TRASH_VIEW_ID = -2;
