import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import { PopupApp } from "./components/Popup";
import { blockPageMenu, inTauri } from "./platform";
import "./index.css";

// The same bundle serves the main window and the notification pop-up window.
const isPopup = inTauri() && getCurrentWindow().label === "popup";
blockPageMenu(document);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {isPopup ? <PopupApp /> : <App />}
  </React.StrictMode>,
);
