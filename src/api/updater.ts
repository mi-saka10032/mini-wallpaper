import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getVersion } from "@tauri-apps/api/app";

/**
 * Updater 薄封装层
 *
 * 直接复用 tauri-plugin-updater 的能力：插件内部已完成
 * 「拉取 latest.json → 版本比对 → 下载安装包 → minisign 签名校验 →
 * 静默拉起 NSIS 安装器 → 覆盖安装」全流程，无需自行管理下载路径。
 *
 * 本层只负责：错误静默化、进度回调归一化、以及暴露给上层的最小 API。
 */

/** 下载进度快照 */
export interface DownloadProgress {
  /** 已下载字节数 */
  downloaded: number;
  /** 总字节数；服务端未返回 Content-Length 时为 null */
  total: number | null;
}

/** 检查结果：null 表示已是最新版本 */
export interface UpdateInfo {
  /** 可更新到的新版本号 */
  version: string;
  /** 当前运行版本号 */
  currentVersion: string;
  /** 更新说明（release notes），可能为空 */
  notes: string;
  /** 发布日期原始字符串，可能为空 */
  date: string;
  /** 插件返回的 Update 句柄，下载安装时复用，避免二次 check */
  handle: Update;
}

/** 获取当前应用版本号（取自 tauri.conf.json 的 version 字段） */
export async function getCurrentVersion(): Promise<string> {
  return getVersion();
}

/**
 * 检查更新
 *
 * @returns 有新版本时返回 UpdateInfo，已是最新返回 null
 * @throws 网络失败 / endpoint 不可达 / 签名公钥不匹配时抛出
 */
export async function checkUpdate(): Promise<UpdateInfo | null> {
  const update = await check();
  // 插件在「已是最新」时返回 null；部分版本返回 available=false 的对象，这里一并兜住
  if (!update || update.available === false) return null;

  return {
    version: update.version,
    currentVersion: update.currentVersion,
    notes: update.body ?? "",
    date: update.date ?? "",
    handle: update,
  };
}

/**
 * 下载并安装更新，随后重启应用
 *
 * 全过程由插件托管：下载到临时目录 → 校验签名 → 运行安装器覆盖安装。
 * 用户无需手动查找文件或点击安装包。
 *
 * @param update 由 checkUpdate 返回的更新信息
 * @param onProgress 下载进度回调
 */
export async function downloadAndInstall(
  update: UpdateInfo,
  onProgress?: (progress: DownloadProgress) => void,
): Promise<void> {
  let downloaded = 0;
  let total: number | null = null;

  await update.handle.downloadAndInstall((event) => {
    switch (event.event) {
      case "Started":
        total = event.data.contentLength ?? null;
        downloaded = 0;
        onProgress?.({ downloaded, total });
        break;
      case "Progress":
        downloaded += event.data.chunkLength;
        onProgress?.({ downloaded, total });
        break;
      case "Finished":
        onProgress?.({ downloaded: total ?? downloaded, total });
        break;
    }
  });

  // Windows 的 NSIS 安装器在 passive 模式下会自行结束当前进程并在安装完成后重启，
  // 此时再调用 relaunch 属于多余操作，且可能抛错把已成功的更新误判为失败。
  // 非 Windows 平台需显式 relaunch；即便如此也单独兜住异常，
  // 避免「安装已完成、仅重启失败」被上层渲染成 error 态。
  const isWindows = navigator.userAgent.includes("Windows");
  if (isWindows) return;

  try {
    await relaunch();
  } catch (e) {
    console.error("[updater.relaunch]", e);
  }
}
