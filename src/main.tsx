import React from "react";
import ReactDOM from "react-dom/client";
import { currentWindowLabel } from "./lib/ipc";
import { PLATFORM } from "./lib/platform";
import "./styles/tokens.css";

const label = currentWindowLabel();
document.documentElement.dataset.platform = PLATFORM;

async function boot() {
  const root = ReactDOM.createRoot(document.getElementById("root")!);
  if (label === "bar") {
    document.documentElement.classList.add("window-bar");
    const { default: BarApp } = await import("./bar/BarApp");
    root.render(<React.StrictMode><BarApp /></React.StrictMode>);
  } else {
    document.documentElement.classList.add("window-main");
    const { default: HubApp } = await import("./hub/HubApp");
    root.render(<React.StrictMode><HubApp /></React.StrictMode>);
  }
}
boot();
