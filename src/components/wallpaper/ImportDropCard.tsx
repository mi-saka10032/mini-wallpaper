import { Upload } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import { useWallpaperStore } from "@/stores/wallpaperStore";

/**
 * 导入拖拽卡片组件
 * 仅在本地壁纸栏（activeId === 0）的默认状态下显示
 * 支持 HTML5 拖拽文件导入壁纸
 */
const ImportDropCard: React.FC = () => {
  const { t } = useTranslation();
  const importByPaths = useWallpaperStore((s) => s.importByPaths);
  const fetchSupportedExtensions = useWallpaperStore((s) => s.fetchSupportedExtensions);
  const [isDragOver, setIsDragOver] = useState(false);
  const [supportedExts, setSupportedExts] = useState<string[]>([]);
  const dragCounterRef = useRef(0);

  // 预加载支持的扩展名
  useEffect(() => {
    fetchSupportedExtensions().then(setSupportedExts);
  }, [fetchSupportedExtensions]);

  const handleDragEnter = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    dragCounterRef.current += 1;
    if (e.dataTransfer.types.includes("Files")) {
      setIsDragOver(true);
    }
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    dragCounterRef.current -= 1;
    if (dragCounterRef.current === 0) {
      setIsDragOver(false);
    }
  }, []);

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    e.dataTransfer.dropEffect = "copy";
  }, []);

  const handleDrop = useCallback(
    async (e: React.DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      setIsDragOver(false);
      dragCounterRef.current = 0;

      const files = Array.from(e.dataTransfer.files);
      if (files.length === 0) return;

      // 过滤支持的文件格式
      const extSet = new Set(supportedExts);
      const validPaths = files
        .filter((f) => {
          const ext = f.name.split(".").pop()?.toLowerCase() ?? "";
          return extSet.has(ext);
        })
        .map((f) => f.path);

      if (validPaths.length > 0) {
        await importByPaths(validPaths);
      }
    },
    [importByPaths, supportedExts],
  );

  return (
    <div
      className={cn(
        "group relative flex cursor-default items-center justify-center overflow-hidden rounded-lg border-2 border-dashed transition-all duration-200",
        isDragOver
          ? "border-primary bg-primary/10 shadow-lg shadow-primary/5"
          : "border-muted-foreground/25 bg-muted/10 hover:border-muted-foreground/40 hover:bg-muted/20",
      )}
      onDragEnter={handleDragEnter}
      onDragLeave={handleDragLeave}
      onDragOver={handleDragOver}
      onDrop={handleDrop}
    >
      <div className="aspect-video w-full" />
      <div className="absolute inset-0 flex flex-col items-center justify-center gap-2 px-3">
        <div
          className={cn(
            "flex size-10 items-center justify-center rounded-full transition-all duration-200",
            isDragOver
              ? "bg-primary/20 text-primary scale-110"
              : "bg-muted-foreground/10 text-muted-foreground/50 group-hover:bg-muted-foreground/15 group-hover:text-muted-foreground/70",
          )}
        >
          <Upload
            className={cn(
              "size-5 transition-transform duration-200",
              isDragOver && "animate-bounce",
            )}
          />
        </div>
        <div className="text-center">
          <p
            className={cn(
              "text-xs font-medium transition-colors duration-200",
              isDragOver ? "text-primary" : "text-muted-foreground/60 group-hover:text-muted-foreground/80",
            )}
          >
            {isDragOver ? t("main.releaseToImport") : t("main.dropToImport")}
          </p>
          <p className="mt-0.5 text-[10px] text-muted-foreground/40">
            {t("main.supportedFormats")}
          </p>
        </div>
      </div>
    </div>
  );
};

export default ImportDropCard;
