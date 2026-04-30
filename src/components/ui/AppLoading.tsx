import { useEffect, useState } from "react";

interface AppLoadingProps {
  /** 是否已完成初始化（true 时触发退出动画） */
  finished: boolean;
  /** 退出动画结束后的回调 */
  onExited?: () => void;
}

/**
 * App 启动 Loading 组件
 * - 窗口完全透明，仅显示 loading 动画元素
 * - 3D Q弹球体动画
 * - 初始化完成后 fade-out 退出，恢复 body 背景
 */
const AppLoading: React.FC<AppLoadingProps> = ({ finished, onExited }) => {
  const [exiting, setExiting] = useState(false);
  const [removed, setRemoved] = useState(false);

  // 挂载时让 body 透明，卸载/退出时恢复
  useEffect(() => {
    document.body.classList.add("loading-transparent");
    return () => {
      document.body.classList.remove("loading-transparent");
    };
  }, []);

  useEffect(() => {
    if (finished) {
      // 触发退出动画
      setExiting(true);
      const timer = setTimeout(() => {
        setRemoved(true);
        // 先恢复 body 背景，再通知外层
        document.body.classList.remove("loading-transparent");
        onExited?.();
      }, 500); // 退出动画持续 500ms
      return () => clearTimeout(timer);
    }
  }, [finished, onExited]);

  if (removed) return null;

  return (
    <div
      className={`fixed inset-0 z-[9999] flex items-center justify-center transition-opacity duration-500 ${
        exiting ? "opacity-0" : "opacity-100"
      }`}
      style={{ background: "transparent" }}
    >
      <div className="flex items-end gap-2">
        {[0, 1, 2].map((i) => (
          <div
            key={i}
            className="loading-ball"
            style={{
              animationDelay: `${i * 0.15}s`,
            }}
          />
        ))}
      </div>

      <style>{`
        .loading-ball {
          width: 14px;
          height: 14px;
          border-radius: 50%;
          background: linear-gradient(
            135deg,
            hsl(220 80% 65%) 0%,
            hsl(260 75% 55%) 100%
          );
          box-shadow:
            0 4px 14px hsla(240, 70%, 50%, 0.4),
            0 0 20px hsla(240, 70%, 60%, 0.2),
            inset 0 -3px 6px hsla(240, 50%, 30%, 0.3),
            inset 0 3px 6px hsla(240, 50%, 90%, 0.4);
          animation: bounce3d 0.8s cubic-bezier(0.28, 0.84, 0.42, 1) infinite alternate;
          transform-style: preserve-3d;
        }

        @keyframes bounce3d {
          0% {
            transform: translateY(0) scale(1, 1) rotateX(0deg);
            box-shadow:
              0 4px 14px hsla(240, 70%, 50%, 0.4),
              0 0 20px hsla(240, 70%, 60%, 0.2),
              inset 0 -3px 6px hsla(240, 50%, 30%, 0.3),
              inset 0 3px 6px hsla(240, 50%, 90%, 0.4);
          }
          30% {
            transform: translateY(-20px) scale(0.9, 1.1) rotateX(20deg);
          }
          50% {
            transform: translateY(-28px) scale(0.85, 1.15) rotateX(35deg);
          }
          70% {
            transform: translateY(-20px) scale(0.9, 1.1) rotateX(20deg);
          }
          100% {
            transform: translateY(0) scale(1.15, 0.85) rotateX(0deg);
            box-shadow:
              0 2px 8px hsla(240, 70%, 50%, 0.2),
              0 0 12px hsla(240, 70%, 60%, 0.1),
              inset 0 -2px 4px hsla(240, 50%, 30%, 0.2),
              inset 0 2px 4px hsla(240, 50%, 90%, 0.3);
          }
        }
      `}</style>
    </div>
  );
};

export default AppLoading;