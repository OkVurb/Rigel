import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import App from "./App";
import Cursor from "./cursor";
import "./shell.css";

const root = document.getElementById("root");
if (!root) throw new Error("#root missing from index.html");

createRoot(root).render(
  <StrictMode>
    <Cursor />
    <App />
  </StrictMode>,
);
