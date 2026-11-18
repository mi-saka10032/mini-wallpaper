import { StrictMode } from "react";
import ReactDOM from "react-dom/client";
import ActionToast from "./ActionToast";
import "./toast.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <ActionToast />
  </StrictMode>,
);
