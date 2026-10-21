import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";
import { applyTheme, readCachedTheme } from "./utils/theme";

// Before the first render, not inside a component: an effect runs after React
// has already painted, which would show the default theme for a frame and then
// swap it. Every window goes through this file, so Spotlight and the timer
// alert are themed by the same line.
applyTheme(readCachedTheme());

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
