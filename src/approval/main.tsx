import React from "react";
import ReactDOM from "react-dom/client";
import ApprovalPopup from "./ApprovalPopup";
import "@/i18n";
import { suppressCaretBrowsing } from "@/lib/caretBrowsing";
import { suppressBrowserContextMenu } from "@/lib/contextMenu";

suppressCaretBrowsing();
suppressBrowserContextMenu();

// Its own window, so it loads its own font, the same family as everywhere else.
import "@fontsource-variable/inter";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ApprovalPopup />
  </React.StrictMode>,
);
