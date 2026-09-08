import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { RouterProvider } from "@tanstack/react-router";
import "@fontsource/inter/latin-400.css";
import "@fontsource/geist-mono/latin-400.css";
import "@fontsource/geist-mono/latin-600.css";
import "@fontsource/space-grotesk/latin-700.css";
import "./index.css";
import { router } from "./router";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <RouterProvider router={router} />
  </StrictMode>,
);
