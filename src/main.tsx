import React from "react";
import ReactDOM from "react-dom/client";
import "./styles.css";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Popover } from "@/components/popover/Popover";
import { SettingsWindow } from "@/components/settings/Settings";

// The system theme decides light/dark (the design ships both palettes).
const media = window.matchMedia("(prefers-color-scheme: dark)");
const applyTheme = () => document.documentElement.classList.toggle("dark", media.matches);
applyTheme();
media.addEventListener("change", applyTheme);

const which = new URLSearchParams(window.location.search).get("w");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <TooltipProvider>{which === "settings" ? <SettingsWindow /> : <Popover />}</TooltipProvider>
  </React.StrictMode>
);
