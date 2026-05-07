import { useEffect, useState, type FC } from "react";
import style from "@/styles/AppLoading.module.css";
import { cn } from "@/lib/utils";

interface AppLoadingProps {
  /** 是否已完成初始化（true 时触发退出动画） */

  finished: boolean;

  /** 退出动画结束后的回调 */

  onExited?: () => void;
}

/**

 * App 启动 Loading 组件

 * - 窗口完全透明，仅显示 loading 动画元素

 * - 蓝色水波纹光圈从圆心向外扩散（类似水滴涟漪）

 * - 初始化完成后 fade-out 退出，恢复 body 背景

 */

const AppLoading: FC<AppLoadingProps> = ({ finished, onExited }) => {
  const [exiting, setExiting] = useState(false);

  const [removed, setRemoved] = useState(false);

  useEffect(() => {
    document.body.classList.add("loading-transparent");

    return () => {
      document.body.classList.remove("loading-transparent");
    };
  }, []);

  useEffect(() => {
    if (finished) {
      setExiting(true);

      const timer = setTimeout(() => {
        setRemoved(true);

        document.body.classList.remove("loading-transparent");

        onExited?.();
      }, 600);

      return () => clearTimeout(timer);
    }
  }, [finished, onExited]);

  if (removed) return null;

  return (
    <div
      className={`fixed inset-0 z-[9999] flex items-center justify-center transition-opacity duration-600 ${
        exiting ? "opacity-0" : "opacity-100"
      }`}
      style={{ background: "transparent" }}
    >
      {/* 水波纹光圈容器 */}

      <div className="relative w-120 h-120 flex justify-center items-center">
        {/* 中心蓝色发光点 */}

        <div className={cn(`absolute w-6 h-6 rounded-[50%]`, style.rippleCenter)} />

        {/* 多层蓝色扩散波纹环 */}

        {[0, 1, 2, 3, 4, 5].map((i) => (
          <div key={i} className={style.rippleWave} style={{ animationDelay: `${i * 0.4}s` }} />
        ))}
      </div>
    </div>
  );
};

export default AppLoading;
