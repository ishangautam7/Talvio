import React, { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Eye, Smartphone } from "lucide-react";
import {
  commands,
  type AccountPhone,
  type CommandCard,
  type ViewingCard,
} from "@/bindings";
import { platformName } from "@/components/settings/account/account";
import { preventBrowserContextMenu } from "@/lib/contextMenu";
import { useSafeWindowDrag } from "@/lib/useSafeWindowDrag";
// The assistant's own card, as the reminder popup uses it.
import "@/assistant/AssistantPanel.css";
import "./ApprovalPopup.css";
import { CommandCardView } from "./CommandCardView";

/**
 * How long the cards must sit still on screen before a click answers them.
 * Until then a click is ignored, so a card that appears, moves or shifts under
 * a click aimed at something else (another window, the card above it) cannot
 * be answered by it. Browsers guard permission prompts the same way.
 */
const ARM_DELAY_MS = 750;

/**
 * The question a phone's first request asks on this computer (see
 * `src-tauri/src/account/approval.rs`): may it dictate and type here? One card
 * per phone asking. The window never takes the keyboard, so the paste that
 * Allow lets through lands where the user's cursor is.
 *
 * The same window says "Phone is viewing" for as long as a phone watches a
 * window here (live view), with Stop, and shows a spoken command's card
 * (`CommandCardView`): what was heard and what happened, with Send and
 * Cancel or the choices when the command waits.
 */
const ApprovalPopup: React.FC = () => {
  const { t } = useTranslation();
  useSafeWindowDrag();
  const [phones, setPhones] = useState<AccountPhone[]>([]);
  const [viewing, setViewing] = useState<ViewingCard | null>(null);
  const [command, setCommand] = useState<CommandCard | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const armedAt = useRef(0);
  const disarm = useCallback(() => {
    armedAt.current = performance.now() + ARM_DELAY_MS;
  }, []);

  // Ask on mount as well: the window is shown in the same breath as the first
  // event, so a webview still loading would miss it.
  useEffect(() => {
    void commands
      .accountPendingApprovals()
      .then(setPhones)
      .catch(() => setPhones([]));
    const unlisten = listen<AccountPhone[]>(
      "account-approval-changed",
      (event) => setPhones(event.payload ?? []),
    );
    void commands
      .accountViewing()
      .then(setViewing)
      .catch(() => setViewing(null));
    const unlistenViewing = listen<ViewingCard | null>(
      "account-viewing-changed",
      (event) => setViewing(event.payload ?? null),
    );
    void commands
      .commandCard()
      .then(setCommand)
      .catch(() => setCommand(null));
    const unlistenCommand = listen<CommandCard | null>(
      "command-card-changed",
      (event) => setCommand(event.payload ?? null),
    );
    return () => {
      void unlisten.then((off) => off());
      void unlistenViewing.then((off) => off());
      void unlistenCommand.then((off) => off());
    };
  }, []);

  // The window takes the height the cards need: a second phone, a long name.
  useEffect(() => {
    const node = rootRef.current;
    if (!node) return;
    const report = () => {
      disarm();
      const height = Math.ceil(node.getBoundingClientRect().height);
      if (height > 0) void commands.fitAccountApproval(height);
    };
    report();
    const observer = new ResizeObserver(report);
    observer.observe(node);
    return () => observer.disconnect();
  }, [phones.length, viewing, command, disarm]);

  // A card added or gone moves the others; the window moves when it is placed
  // again (a reminder taking the corner) or dragged.
  useEffect(() => disarm(), [phones, command?.seq, disarm]);
  useEffect(() => {
    const unlisten = getCurrentWindow().onMoved(disarm);
    return () => {
      void unlisten.then((off) => off());
    };
  }, [disarm]);

  const answer = useCallback((id: string, allow: boolean) => {
    setPhones((current) => current.filter((phone) => phone.id !== id));
    void commands.accountAnswerApproval(id, allow);
  }, []);

  const click = (id: string, allow: boolean) => {
    if (performance.now() >= armedAt.current) answer(id, allow);
  };

  // Esc is Don't allow, where the window can have the keyboard at all.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      for (const phone of phones) answer(phone.id, false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [phones, answer]);

  const stopViewing = () => {
    if (performance.now() >= armedAt.current)
      void commands.accountStopViewing();
  };

  const armed = () => performance.now() >= armedAt.current;

  if (phones.length === 0 && !viewing && !command) return null;

  return (
    <div
      className="assistant-scope approval-scope"
      ref={rootRef}
      onContextMenu={preventBrowserContextMenu}
    >
      {command && <CommandCardView card={command} armed={armed} />}
      {viewing && (
        <section
          className="ask-card"
          aria-labelledby="viewing-title"
          aria-describedby="viewing-body"
        >
          <header className="ask-head" data-tauri-drag-region>
            <Eye className="ask-head-icon" size={12} />
            <p className="ask-question-text" id="viewing-title">
              {t("phoneViewing.title")}
            </p>
          </header>
          <div className="ask-answer-body">
            <p id="viewing-body">
              {t("phoneViewing.body", {
                phone: viewing.phone || t("phoneViewing.unnamedPhone"),
                title: viewing.title,
              })}
            </p>
          </div>
          <div className="approval-actions">
            <button
              type="button"
              className="qa-btn primary"
              onClick={stopViewing}
            >
              {t("phoneViewing.stop")}
            </button>
          </div>
        </section>
      )}
      {phones.map((phone) => (
        <section
          className="ask-card"
          key={phone.id}
          aria-labelledby={`approval-${phone.id}-name`}
          aria-describedby={`approval-${phone.id}-body`}
        >
          <header className="ask-head" data-tauri-drag-region>
            <Smartphone className="ask-head-icon" size={12} />
            <p className="ask-question-text">{t("phoneApproval.title")}</p>
          </header>
          <div className="ask-answer-body">
            <p className="approval-phone" id={`approval-${phone.id}-name`}>
              <strong>{phone.name}</strong>
              <span className="approval-platform">
                {` · ${platformName(phone.platform)}`}
              </span>
            </p>
            <p id={`approval-${phone.id}-body`}>{t("phoneApproval.body")}</p>
          </div>
          <div className="approval-actions">
            <button
              type="button"
              className="qa-btn"
              onClick={() => click(phone.id, false)}
            >
              {t("phoneApproval.deny")}
            </button>
            <button
              type="button"
              className="qa-btn primary"
              onClick={() => click(phone.id, true)}
            >
              {t("phoneApproval.allow")}
            </button>
          </div>
        </section>
      ))}
    </div>
  );
};

export default ApprovalPopup;
