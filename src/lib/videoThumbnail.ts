import { convertFileSrc } from "@tauri-apps/api/core";

/** 缩略图最大宽度 */
const MAX_WIDTH = 480;

/** 视频加载超时时间（ms） */
const TIMEOUT = 15_000;

/**
 * 通过 canvas 抽取视频首帧，等比缩放（最大宽度 480px），输出 WebP 格式的 Uint8Array。
 *
 * @param filePath 本地视频文件绝对路径（Tauri asset 协议转换）
 * @returns WebP 格式的字节数组；加载/解码失败时返回 null
 */
export async function extractVideoThumbnail(
  filePath: string,
): Promise<Uint8Array | null> {
  const videoUrl = convertFileSrc(filePath);

  return new Promise<Uint8Array | null>((resolve) => {
    const video = document.createElement("video");
    video.crossOrigin = "anonymous";
    video.preload = "auto";
    video.muted = true;

    let settled = false;

    const cleanup = () => {
      video.removeEventListener("loadedmetadata", onLoadedMetadata);
      video.removeEventListener("canplay", onCanPlay);
      video.removeEventListener("error", onError);
      clearTimeout(timeoutId);
      try {
        video.pause();
      } catch {
        /* noop */
      }
      video.src = "";
      video.load();
      video.remove();
    };

    const settle = (value: Uint8Array | null) => {
      if (settled) return;
      settled = true;
      cleanup();
      resolve(value);
    };

    const timeoutId = setTimeout(() => {
      console.warn(`[videoThumbnail] timeout: ${filePath}`);
      settle(null);
    }, TIMEOUT);

    const onLoadedMetadata = () => {
      try {
        video.currentTime = 0;
      } catch {
        settle(null);
      }
    };

    const onCanPlay = () => {
      try {
        const ow = video.videoWidth;
        const oh = video.videoHeight;
        if (ow === 0 || oh === 0) {
          settle(null);
          return;
        }

        // 等比缩放：宽度超过 MAX_WIDTH 时按比例缩小
        let tw = ow;
        let th = oh;
        if (ow > MAX_WIDTH) {
          tw = MAX_WIDTH;
          th = Math.round((oh / ow) * MAX_WIDTH);
        }

        const canvas = document.createElement("canvas");
        canvas.width = tw;
        canvas.height = th;
        const ctx = canvas.getContext("2d");
        if (!ctx) {
          settle(null);
          return;
        }

        ctx.drawImage(video, 0, 0, tw, th);

        canvas.toBlob(
          (blob) => {
            if (!blob) {
              // WebP 不支持时回退 JPEG
              canvas.toBlob(
                (jpegBlob) => {
                  if (!jpegBlob) {
                    settle(null);
                    return;
                  }
                  jpegBlob.arrayBuffer().then(
                    (buf) => settle(new Uint8Array(buf)),
                    () => settle(null),
                  );
                },
                "image/jpeg",
                0.85,
              );
              return;
            }
            blob.arrayBuffer().then(
              (buf) => settle(new Uint8Array(buf)),
              () => settle(null),
            );
          },
          "image/webp",
          0.85,
        );
      } catch {
        settle(null);
      }
    };

    const onError = () => {
      console.warn(`[videoThumbnail] load error: ${filePath}`);
      settle(null);
    };

    video.addEventListener("loadedmetadata", onLoadedMetadata);
    video.addEventListener("canplay", onCanPlay);
    video.addEventListener("error", onError);

    video.src = videoUrl;
    video.load();
  });
}

/**
 * 分批生成视频缩略图。
 *
 * @param items 待处理列表，每项包含壁纸 ID 和文件路径
 * @param batchSize 每批并发数量（默认 10）
 * @param onBatch 每批完成后的回调，接收 { wallpaperId, data } 数组（data 为 null 表示失败）
 */
export async function batchExtractVideoThumbnails(
  items: { wallpaperId: number; filePath: string }[],
  onBatch: (
    results: { wallpaperId: number; data: Uint8Array | null }[],
  ) => Promise<void>,
  batchSize = 10,
): Promise<void> {
  for (let i = 0; i < items.length; i += batchSize) {
    const batch = items.slice(i, i + batchSize);

    const results = await Promise.allSettled(
      batch.map(async (item) => {
        const data = await extractVideoThumbnail(item.filePath);
        return { wallpaperId: item.wallpaperId, data };
      }),
    );

    const batchResults = results.map((r, idx) =>
      r.status === "fulfilled"
        ? r.value
        : { wallpaperId: batch[idx].wallpaperId, data: null },
    );

    await onBatch(batchResults);
  }
}
