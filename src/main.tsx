import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import Overlay from "./Overlay";
import "./styles.css";

let windowLabel = "";
try {
  windowLabel = getCurrentWindow().label;
} catch {
  windowLabel = "";
}

const isOverlay =
  windowLabel === "overlay" ||
  new URLSearchParams(window.location.search).get("window") === "overlay";

document.documentElement.classList.toggle("overlay-doc", isOverlay);

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>{isOverlay ? <Overlay /> : <App />}</React.StrictMode>,
);
