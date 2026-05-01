import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  FolderPlus,
  Monitor,
  Star,
  Trash2,
  Unlink,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import type { Wallpaper } from "@/api/config";
import { getCollectionWallpapers } from "@/api/collection";
import { useCollectionStore } from "@/stores/collectionStore";
import { useMonitorConfigStore } from "@/stores/monitorConfigStore";
import { useSettingStore, SETTING_KEYS } from "@/stores/settingStore";
import { cn } from "@/lib/utils";

// ============ Context 类型定义 ============

interface ContextMenuState {
  /** 打开右键菜单 */
  openContextMenu: (
    wallpaper: Wallpaper,
    event: React.MouseEvent,
    options: {
      activeId: number;
      isCollectionView: boolean;
      onDelete: (id: number) => void;
      onAddToCollection: (wallpaperId: number, collectionId: number) => void;
    },
  ) => void;
}

const WallpaperCardContextMenuContext = createContext<ContextMenuState | null>(null);

/** Hook：获取全局右键菜单的 openContextMenu 方法 */
export function useWallpaperCardContextMenu() {
  const ctx = useContext(WallpaperCardContextMenuContext);
  if (!ctx) {
    throw new Error("useWallpaperCardContextMenu must be used within WallpaperCardContextMenuProvider");
  }
  return ctx;
}

// ============ 菜单项数据 ============

interface MenuOpenPayload {
  wallpaper: Wallpaper;
  position: { x: number; y: number };
  activeId: number;
  isCollectionView: boolean;
  onDelete: (id: number) => void;
  onAddToCollection: (wallpaperId: number, collectionId: number) => void;
}

// ============ Provider 组件 ============

export const WallpaperCardContextMenuProvider: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => {
  const [open, setOpen] = useState(false);
  const [payload, setPayload] = useState<MenuOpenPayload | null>(null);
  const [subMenu, setSubMenu] = useState<"setAs" | "addTo" | null>(null);

  const menuRef = useRef<HTMLDivElement>(null);
  const subMenuRef = useRef<HTMLDivElement>(null);

  // 从 store 按需获取数据（P1 优化：仅在菜单打开时才消费）
  const collections = useCollectionStore((s) => s.collections);
  const configs = useMonitorConfigStore((s) => s.configs);
  const upsert = useMonitorConfigStore((s) => s.upsert);
  const upsertAll = useMonitorConfigStore((s) => s.upsertAll);
  const displayMode = useSettingStore((s) => s.settings[SETTING_KEYS.DISPLAY_MODE] ?? "independent");

  const activeConfigs = useMemo(() => configs.filter((c) => c.active), [configs]);

  const { t } = useTranslation();

  // 打开菜单
  const openContextMenu = useCallback(
    (
      wallpaper: Wallpaper,
      event: React.MouseEvent,
      options: {
        activeId: number;
        isCollectionView: boolean;
        onDelete: (id: number) => void;
        onAddToCollection: (wallpaperId: number, collectionId: number) => void;
      },
    ) => {
      event.preventDefault();
      event.stopPropagation();
      setPayload({
        wallpaper,
        position: { x: event.clientX, y: event.clientY },
        activeId: options.activeId,
        isCollectionView: options.isCollectionView,
        onDelete: options.onDelete,
        onAddToCollection: options.onAddToCollection,
      });
      setSubMenu(null);
      setOpen(true);
    },
    [],
  );

  // 关闭菜单
  const closeMenu = useCallback(() => {
    setOpen(false);
    setSubMenu(null);
  }, []);

  // 点击外部关闭
  useEffect(() => {
    if (!open) return;

    const handleClickOutside = (e: MouseEvent) => {
      if (
        menuRef.current && !menuRef.current.contains(e.target as Node) &&
        (!subMenuRef.current || !subMenuRef.current.contains(e.target as Node))
      ) {
        closeMenu();
      }
    };

    const handleScroll = () => closeMenu();
    const handleContextMenuOutside = (e: MouseEvent) => {
      // 如果右键点击在菜单外部，关闭当前菜单
      if (
        menuRef.current && !menuRef.current.contains(e.target as Node) &&
        (!subMenuRef.current || !subMenuRef.current.contains(e.target as Node))
      ) {
        closeMenu();
      }
    };

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeMenu();
    };

    // 延迟绑定，避免当前右键事件立即触发关闭
    const timer = setTimeout(() => {
      document.addEventListener("mousedown", handleClickOutside);
      document.addEventListener("contextmenu", handleContextMenuOutside);
      document.addEventListener("scroll", handleScroll, true);
      document.addEventListener("keydown", handleKeyDown);
    }, 0);

    return () => {
      clearTimeout(timer);
      document.removeEventListener("mousedown", handleClickOutside);
      document.removeEventListener("contextmenu", handleContextMenuOutside);
      document.removeEventListener("scroll", handleScroll, true);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [open, closeMenu]);

  // 设置为壁纸的处理逻辑
  const handleSetAsWallpaper = useCallback(
    async (monitorId: string) => {
      if (!payload) return;
      const { wallpaper, activeId } = payload;
      const isSyncMode = displayMode === "mirror" || displayMode === "extend";
      const targetConfig = configs.find((c) => c.monitor_id === monitorId);
      const collectionId = activeId > 0 ? activeId : null;

      if (activeId === 0) {
        if (!targetConfig?.collection_id) {
          if (isSyncMode) await upsertAll({ wallpaperId: wallpaper.id });
          else await upsert({ monitorId, wallpaperId: wallpaper.id });
        } else {
          try {
            const wallpapersInCollection = await getCollectionWallpapers(targetConfig.collection_id);
            const isInCollection = wallpapersInCollection.some((w) => w.id === wallpaper.id);
            if (isInCollection) {
              if (isSyncMode) await upsertAll({ wallpaperId: wallpaper.id });
              else await upsert({ monitorId, wallpaperId: wallpaper.id });
            } else {
              if (isSyncMode) await upsertAll({ wallpaperId: wallpaper.id, clearCollection: true, isEnabled: false });
              else await upsert({ monitorId, wallpaperId: wallpaper.id, clearCollection: true, isEnabled: false });
            }
          } catch {
            if (isSyncMode) await upsertAll({ wallpaperId: wallpaper.id, clearCollection: true, isEnabled: false });
            else await upsert({ monitorId, wallpaperId: wallpaper.id, clearCollection: true, isEnabled: false });
          }
        }
      } else {
        if (!targetConfig?.collection_id) {
          if (isSyncMode) await upsertAll({ wallpaperId: wallpaper.id, collectionId });
          else await upsert({ monitorId, wallpaperId: wallpaper.id, collectionId });
        } else if (targetConfig.collection_id === collectionId) {
          if (isSyncMode) await upsertAll({ wallpaperId: wallpaper.id });
          else await upsert({ monitorId, wallpaperId: wallpaper.id });
        } else {
          if (isSyncMode) await upsertAll({ wallpaperId: wallpaper.id, collectionId });
          else await upsert({ monitorId, wallpaperId: wallpaper.id, collectionId });
        }
      }
      closeMenu();
    },
    [payload, configs, displayMode, upsert, upsertAll, closeMenu],
  );

  // 计算菜单位置（确保不超出视口，且在鼠标右侧弹出）
  const menuStyle = useMemo((): React.CSSProperties => {
    if (!payload) return { display: "none" };
    const { x, y } = payload.position;
    const menuWidth = 200;
    const menuHeight = 160;
    const viewportW = window.innerWidth;
    const viewportH = window.innerHeight;

    // 优先在鼠标右侧弹出
    let left = x + 2;
    let top = y;

    // 如果右侧空间不够，则在左侧弹出
    if (left + menuWidth > viewportW) {
      left = x - menuWidth - 2;
    }
    // 如果底部空间不够，向上偏移
    if (top + menuHeight > viewportH) {
      top = viewportH - menuHeight - 8;
    }
    if (top < 8) top = 8;

    return {
      position: "fixed",
      left: `${left}px`,
      top: `${top}px`,
      zIndex: 9999,
    };
  }, [payload]);

  // 子菜单位置（在主菜单右侧），直接计算避免 useMemo 对 mutable ref 的依赖问题
  const getSubMenuStyle = useCallback((): React.CSSProperties => {
    if (!menuRef.current) return { display: "none" };
    const rect = menuRef.current.getBoundingClientRect();
    const viewportW = window.innerWidth;
    const viewportH = window.innerHeight;
    const subWidth = 220;
    const subHeight = 200;

    // 优先在主菜单右侧
    let left = rect.right + 2;
    let top = rect.top;

    if (left + subWidth > viewportW) {
      left = rect.left - subWidth - 2;
    }
    if (top + subHeight > viewportH) {
      top = viewportH - subHeight - 8;
    }
    if (top < 8) top = 8;

    return {
      position: "fixed",
      left: `${left}px`,
      top: `${top}px`,
      zIndex: 10000,
    };
  }, []);

  const contextValue = useMemo(() => ({ openContextMenu }), [openContextMenu]);

  return (
    <WallpaperCardContextMenuContext.Provider value={contextValue}>
      {children}

      {/* 全局单例右键菜单 */}
      {open && payload && (
        <>
          {/* 主菜单 */}
          <div
            ref={menuRef}
            style={menuStyle}
            className="min-w-[10rem] overflow-hidden rounded-md border bg-popover p-1 text-popover-foreground shadow-md animate-in fade-in-0 zoom-in-95"
          >
            {/* 设置为壁纸 */}
            <button
              type="button"
              className={cn(
                "relative flex w-full cursor-default items-center gap-2 rounded-sm px-2 py-1.5 text-sm outline-none select-none",
                "hover:bg-accent hover:text-accent-foreground",
                activeConfigs.length === 0 && "pointer-events-none opacity-50",
              )}
              onMouseEnter={() => setSubMenu("setAs")}
            >
              <Monitor className="size-4 text-muted-foreground" />
              {t("main.setAs")}
              <span className="ml-auto text-muted-foreground">›</span>
            </button>

            {/* 添加到收藏夹（仅全部壁纸视图） */}
            {!payload.isCollectionView && (
              <button
                type="button"
                className={cn(
                  "relative flex w-full cursor-default items-center gap-2 rounded-sm px-2 py-1.5 text-sm outline-none select-none",
                  "hover:bg-accent hover:text-accent-foreground",
                  collections.length === 0 && "pointer-events-none opacity-50",
                )}
                onMouseEnter={() => setSubMenu("addTo")}
              >
                <FolderPlus className="size-4 text-muted-foreground" />
                {t("main.addTo")}
                <span className="ml-auto text-muted-foreground">›</span>
              </button>
            )}

            {/* 删除/移除 */}
            <button
              type="button"
              className={cn(
                "relative flex w-full cursor-default items-center gap-2 rounded-sm px-2 py-1.5 text-sm outline-none select-none",
                "hover:bg-accent hover:text-accent-foreground",
                !payload.isCollectionView && "text-destructive hover:bg-destructive/10 hover:text-destructive",
              )}
              onMouseEnter={() => setSubMenu(null)}
              onClick={() => {
                payload.onDelete(payload.wallpaper.id);
                closeMenu();
              }}
            >
              {payload.isCollectionView ? (
                <>
                  <Unlink className="size-4 text-muted-foreground" />
                  {t("main.removeFromCollection")}
                </>
              ) : (
                <>
                  <Trash2 className="size-4 text-destructive" />
                  {t("main.delete")}
                </>
              )}
            </button>
          </div>

          {/* 子菜单：设置为壁纸 */}
          {subMenu === "setAs" && (
            <div
              ref={subMenuRef}
              style={getSubMenuStyle()}
              className="min-w-[10rem] max-w-[16rem] overflow-hidden rounded-md border bg-popover p-1 text-popover-foreground shadow-lg animate-in fade-in-0 zoom-in-95"
            >
              {activeConfigs.map((config) => {
                const isCurrent = config.wallpaper_id === payload.wallpaper.id;
                return (
                  <button
                    key={config.monitor_id}
                    type="button"
                    disabled={isCurrent}
                    className={cn(
                      "relative flex w-full cursor-default items-center gap-2 rounded-sm px-2 py-1.5 text-sm outline-none select-none",
                      "hover:bg-accent hover:text-accent-foreground",
                      isCurrent && "pointer-events-none opacity-50",
                    )}
                    onClick={() => !isCurrent && handleSetAsWallpaper(config.monitor_id)}
                  >
                    <Monitor className="size-4 shrink-0 text-muted-foreground" />
                    <span className="max-w-40 truncate">
                      {t("main.wallpaperOf", { name: config.monitor_id })}
                    </span>
                    {isCurrent && (
                      <span className="ml-auto pl-2 text-xs text-foreground/50">
                        {t("main.currentWallpaper")}
                      </span>
                    )}
                  </button>
                );
              })}
            </div>
          )}

          {/* 子菜单：添加到收藏夹 */}
          {subMenu === "addTo" && !payload.isCollectionView && (
            <div
              ref={subMenuRef}
              style={getSubMenuStyle()}
              className="min-w-[10rem] max-w-[14rem] overflow-hidden rounded-md border bg-popover p-1 text-popover-foreground shadow-lg animate-in fade-in-0 zoom-in-95"
            >
              {collections.map((col) => (
                <button
                  key={col.id}
                  type="button"
                  className="relative flex w-full cursor-default items-center gap-2 rounded-sm px-2 py-1.5 text-sm outline-none select-none hover:bg-accent hover:text-accent-foreground"
                  title={col.name}
                  onClick={() => {
                    payload.onAddToCollection(payload.wallpaper.id, col.id);
                    closeMenu();
                  }}
                >
                  <Star className="size-4 shrink-0 text-muted-foreground" />
                  <span className="max-w-32 truncate">{col.name}</span>
                </button>
              ))}
            </div>
          )}
        </>
      )}
    </WallpaperCardContextMenuContext.Provider>
  );
};
