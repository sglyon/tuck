import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import DictationOverlay from "./components/DictationOverlay";
import "./styles.css";
ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    {new URLSearchParams(window.location.search).has("dictation-overlay") ? <DictationOverlay /> : <App />}
  </React.StrictMode>,
);
