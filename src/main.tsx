import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";
import { loadPlatform } from "./utils/platform";
import { applyTheme, readCachedTheme } from "./utils/theme";

// Before the first render, not inside a component: an effect runs after React
// has already painted, which would show the default theme for a frame and then
// swap it. Every window goes through this file, so Spotlight and the timer
// alert are themed by the same line.
applyTheme(readCachedTheme());

// Awaited before the first render so no component has to cope with not yet
// knowing which platform it is on. It is one IPC round trip against a
// synchronous Rust function, and it cannot reject — a failure leaves the
// fallback in place — so there is nothing here to stall startup.
loadPlatform().finally(() => {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>
  );
});
