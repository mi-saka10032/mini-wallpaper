import { useEffect, useState } from "react";

interface AppLoadingProps {
  /** 是否已完成初始化（true 时触发退出动画） */
  finished: boolean;
  /** 退出动画结束后的回调 */
  onExited?: () => void;
}

/**
 * App 启动 Loading 组件
 * - 透明背景，无其他 UI 元素
 * - 3D Q弹球体动画
 * - 初始化完成后 fade-out 退出
 */
const AppLoading: React.FC<AppLoadingProps> = ({ finished, onExited }) => {
  const [exiting, setExiting] = useState(false);
  const [removed, setRemoved] = useState(false);

  useEffect(() => {
    if (finished) {
      // 触发退出动画
      setExiting(true);
      const timer = setTimeout(() => {
        setRemoved(true);
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
            hsl(var(--primary-hue, 220) 70% 60%) 0%,
            hsl(var(--primary-hue, 220) 80% 45%) 100%
          );
          background: var(--primary);
          box-shadow:
            0 4px 12px oklch(0.5 0.1 250 / 0.3),
            inset 0 -3px 6px oklch(0.3 0.05 250 / 0.2),
            inset 0 3px 6px oklch(0.9 0.02 250 / 0.4);
          animation: bounce3d 0.8s cubic-bezier(0.28, 0.84, 0.42, 1) infinite alternate;
          transform-style: preserve-3d;
        }

        @keyframes bounce3d {
          0% {
            transform: translateY(0) scale(1, 1) rotateX(0deg);
            box-shadow:
              0 4px 12px oklch(0.5 0.1 250 / 0.3),
              inset 0 -3px 6px oklch(0.3 0.05 250 / 0.2),
              inset 0 3px 6px oklch(0.9 0.02 250 / 0.4);
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
              0 2px 6px oklch(0.5 0.1 250 / 0.15),
              inset 0 -2px 4px oklch(0.3 0.05 250 / 0.15),
              inset 0 2px 4px oklch(0.9 0.02 250 / 0.3);
          }
        }
      `}</style>
    </div>
  );
};

export default AppLoading;
