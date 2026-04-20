import { getCurrentWindow } from "@tauri-apps/api/window";
import { exit } from "@tauri-apps/plugin-process";
import { Minus, Square, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useSettingStore, SETTING_KEYS } from "@/stores/settingStore";

const appWindow = getCurrentWindow();

const WindowControls: React.FC = () => {
  const [isMaximized, setIsMaximized] = useState(false);
  const closeToTray = useSettingStore(
    (s) => s.settings[SETTING_KEYS.CLOSE_TO_TRAY],
  );

  useEffect(() => {
    const checkMaximized = async () => {
      setIsMaximized(await appWindow.isMaximized());
    };
    checkMaximized();

    const unlisten = appWindow.onResized(async () => {
      setIsMaximized(await appWindow.isMaximized());
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const handleMinimize = useCallback(() => {
    appWindow.minimize();
  }, []);

  const handleMaximize = useCallback(async () => {
    if (await appWindow.isMaximized()) {
      appWindow.unmaximize();
    } else {
      appWindow.maximize();
    }
  }, []);

  const handleClose = useCallback(() => {
    if (closeToTray === "true") {
      appWindow.hide();
    } else {
      exit(0);
    }
  }, [closeToTray]);

  return (
    <div className="flex items-center">
      <button
        onClick={handleMinimize}
        className="flex h-8 w-10 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
      >
        <Minus className="size-4" />
      </button>
      <button
        onClick={handleMaximize}
        className="flex h-8 w-10 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
      >
        {isMaximized ? (
          <svg
            className="size-3.5"
            viewBox="0 0 12 12"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.2"
          >
            <rect x="2.5" y="0.5" width="9" height="9" rx="1" />
            <rect x="0.5" y="2.5" width="9" height="9" rx="1" fill="var(--background)" />
          </svg>
        ) : (
          <Square className="size-3.5" />
        )}
      </button>
      <button
        onClick={handleClose}
        className="flex h-8 w-10 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:bg-destructive hover:text-white"
      >
        <X className="size-4" />
      </button>
    </div>
  );
};

export default WindowControls;
