import { StrictMode } from "react";
import ReactDOM from "react-dom/client";
import { AppShell } from "./App";
import "@/i18n";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <AppShell />
  </StrictMode>,
);
