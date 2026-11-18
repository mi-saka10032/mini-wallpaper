import { StrictMode } from "react";
import ReactDOM from "react-dom/client";
import WallpaperRenderer from "./WallpaperRenderer";
import "./wallpaper.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <WallpaperRenderer />
  </StrictMode>,
);
