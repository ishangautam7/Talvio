import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { commands, type AccountStatus } from "@/bindings";
import { DeleteAccount } from "./DeleteAccount";
import { ServerSetting } from "./ServerSetting";
import { SignedIn } from "./SignedIn";
import { SignInForm } from "./SignInForm";

/** How often to ask again whether the server offers Google, while unknown. */
const CONFIG_RETRY_MS = 31_000;

/**
 * Settings → Account: sign in here and in the phone app with one account, and
 * the phone can dictate and type on this computer. Rust owns the session, the
 * connection and the device list (`src-tauri/src/account/`); this tab shows
 * the `AccountStatus` it publishes and calls the account commands.
 */
export const AccountSettings: React.FC = () => {
  const { t } = useTranslation();
  const [status, setStatus] = useState<AccountStatus | null>(null);

  useEffect(() => {
    let active = true;
    const unlisten = listen<AccountStatus>("account-status-changed", (event) =>
      setStatus(event.payload),
    );
    commands
      .accountGetStatus()
      // An event that landed while this was in flight is the newer status.
      .then((current) => active && setStatus((seen) => seen ?? current))
      .catch((error) => console.error("Failed to load account status:", error));
    return () => {
      active = false;
      void unlisten.then((stop) => stop());
    };
  }, []);

  // A server that did not answer `/v1/config` (down, or the network still
  // waking) is asked again while this tab is open; the answer is an event.
  const configUnknown =
    status !== null && !status.signed_in && status.google_available === null;
  useEffect(() => {
    if (!configUnknown) return;
    const timer = window.setInterval(
      () => void commands.accountGetStatus().catch(() => {}),
      CONFIG_RETRY_MS,
    );
    return () => window.clearInterval(timer);
  }, [configUnknown]);

  if (!status) {
    return <p className="px-1 text-sm text-muted">{t("common.loading")}</p>;
  }

  const user = status.signed_in ? status.user : null;

  return (
    <>
      {user ? (
        <SignedIn status={status} user={user} />
      ) : (
        <SignInForm
          googleAvailable={status.google_available === true}
          googlePending={status.google_pending}
        />
      )}
      <ServerSetting serverUrl={status.server_url} />
      {user && <DeleteAccount email={user.email} />}
    </>
  );
};
