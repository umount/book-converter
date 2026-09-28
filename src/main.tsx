import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";
import { installFocusIndicators } from "./shared/ui/focusIndicators";

const removeFocusIndicators = installFocusIndicators();
if (import.meta.hot) import.meta.hot.dispose(removeFocusIndicators);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
