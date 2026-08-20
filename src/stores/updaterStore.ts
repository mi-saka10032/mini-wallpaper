import { create } from "zustand";
import {
  checkUpdate,
  downloadAndInstall,
  getCurrentVersion,
  type DownloadProgress,
  type UpdateInfo,
} from "@/api/updater";

/** 更新流程阶段 */
export type UpdatePhase =
  /** 空闲：尚未检查，或检查失败已静默收敛 */
  | "idle"
  /** 正在检查版本 */
  | "checking"
  /** 已是最新版本 */
  | "latest"
  /** 发现新版本，等待用户决定是否下载 */
  | "available"
  /** 正在下载安装包 */
  | "downloading"
  /** 下载完成，正在安装 / 准备重启 */
  | "installing"
  /** 检查或下载失败 */
  | "error";

interface UpdaterState {
  phase: UpdatePhase;
  /** 当前运行版本号 */
  currentVersion: string;
  /** 可更新到的版本信息；无新版本时为 null */
  update: UpdateInfo | null;
  /** 下载进度；未在下载中时为 null */
  progress: DownloadProgress | null;
  /** 错误信息（仅手动检查时展示，启动静默检查不展示） */
  error: string | null;
  /** 右上角浮窗是否可见 */
  toastVisible: boolean;

  /** 读取并缓存当前版本号 */
  initVersion: () => Promise<void>;
  /**
   * 检查更新
   * @param silent 静默模式：失败不记录 error、发现新版本时自动弹出浮窗
   */
  runCheck: (silent: boolean) => Promise<void>;
  /** 下载并安装当前待更新版本 */
  startInstall: () => Promise<void>;
  /** 关闭右上角浮窗（不影响 phase，设置面板内仍可见更新态） */
  dismissToast: () => void;
}

export const useUpdaterStore = create<UpdaterState>((set, get) => ({
  phase: "idle",
  currentVersion: "",
  update: null,
  progress: null,
  error: null,
  toastVisible: false,

  initVersion: async () => {
    try {
      const version = await getCurrentVersion();
      set({ currentVersion: version });
    } catch (e) {
      console.error("[updater.initVersion]", e);
    }
  },

  runCheck: async (silent: boolean) => {
    // 下载 / 安装进行中时禁止重复检查，避免覆盖 Update 句柄
    const { phase } = get();
    if (phase === "checking" || phase === "downloading" || phase === "installing") return;

    set({ phase: "checking", error: null });
    try {
      const update = await checkUpdate();
      if (update) {
        set({
          phase: "available",
          update,
          currentVersion: update.currentVersion || get().currentVersion,
          // 静默检查发现新版本时主动弹浮窗；手动检查在设置面板内展示，不弹浮窗
          toastVisible: silent,
        });
      } else {
        set({ phase: "latest", update: null });
      }
    } catch (e) {
      // 启动静默检查失败（离线、GitHub 不可达等）不打扰用户，仅留日志
      console.error("[updater.runCheck]", e);
      set({
        phase: silent ? "idle" : "error",
        error: silent ? null : String(e),
      });
    }
  },

  startInstall: async () => {
    const { update } = get();
    if (!update) return;

    set({ phase: "downloading", progress: { downloaded: 0, total: null }, error: null });
    try {
      await downloadAndInstall(update, (progress) => {
        set({ progress });
        // 下载完成但安装器尚未接管的窗口期，切到 installing 提示用户勿关闭
        if (progress.total !== null && progress.downloaded >= progress.total) {
          set({ phase: "installing" });
        }
      });
      set({ phase: "installing" });
    } catch (e) {
      console.error("[updater.startInstall]", e);
      set({ phase: "error", error: String(e), progress: null });
    }
  },

  dismissToast: () => set({ toastVisible: false }),
}));
