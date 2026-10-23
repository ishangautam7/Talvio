import React, { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "@/hooks/useSettings";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Segmented } from "@/components/ui/Segmented";
import { SettingContainer } from "@/components/ui/SettingContainer";
import { SettingsGroup } from "@/components/ui/SettingsGroup";
import { accountErrorKey, normalizeServerUrl } from "./account";

type Mode = "automatic" | "custom";

interface ServerSettingProps {
  /** The server in use; empty while Automatic has found none. */
  serverUrl: string;
  /** Automatic finds the server; Custom is one the user typed. */
  automatic: boolean;
  /** Signed in: a session belongs to one server, so the choice is fixed. */
  locked: boolean;
  /** A sign-in is waiting on this server, so it cannot change under it. */
  busy: boolean;
  /** Whether the row holds a choice that is not saved. */
  onUnsavedChange: (unsaved: boolean) => void;
}

/**
 * Which account server this app signs in to: Automatic (the one Zynveliq's
 * discovery address names, since its host name changes), or a Custom one for
 * people who run their own.
 */
export const ServerSetting: React.FC<ServerSettingProps> = ({
  serverUrl,
  automatic,
  locked,
  busy,
  onUnsavedChange,
}) => {
  const { t } = useTranslation();
  const { updateSetting, isUpdating } = useSettings();
  const errorId = useId();
  const [mode, setMode] = useState<Mode>(automatic ? "automatic" : "custom");
  const [draft, setDraft] = useState(automatic ? "" : serverUrl);
  const [error, setError] = useState<string | null>(null);
  const saving = isUpdating("account_server_url");
  const unsaved =
    !locked && mode === "custom" && (automatic || draft.trim() !== serverUrl);

  // Signing in or out, or a saved change, shows the server in use, never an
  // edit that was not saved. Automatic's server moving on its own leaves a
  // Custom draft alone.
  const customUrl = automatic ? null : serverUrl;
  useEffect(() => {
    setMode(customUrl === null ? "automatic" : "custom");
    setDraft(customUrl ?? "");
    setError(null);
  }, [customUrl, locked]);

  useEffect(() => onUnsavedChange(unsaved), [unsaved, onUnsavedChange]);

  const save = async (url: string): Promise<boolean> => {
    setError(null);
    if (await updateSetting("account_server_url", url)) return true;
    setError("common.saveFailed");
    return false;
  };

  const choose = (next: Mode) => {
    setMode(next);
    setError(null);
    if (next === "custom") {
      // Start from the server in use, so a small change is a small edit.
      setDraft(serverUrl);
    } else if (!automatic) {
      void save("").then((saved) => {
        if (!saved) setMode("custom");
      });
    }
  };

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    const url = normalizeServerUrl(draft);
    if (!url) {
      setError(accountErrorKey("invalid_url"));
      return;
    }
    setDraft(url);
    if (automatic || url !== serverUrl) await save(url);
  };

  const host = serverUrl.replace(/^https?:\/\//, "");
  const errorLine = error && (
    <p id={errorId} role="alert" className="mt-2 text-sm text-error">
      {t(error)}
    </p>
  );

  return (
    <SettingsGroup>
      <SettingContainer
        title={t("settings.account.server.title")}
        description={t(
          locked
            ? "settings.account.server.locked"
            : "settings.account.server.hint",
        )}
        descriptionMode={locked ? "caption" : "tooltip"}
        grouped
        details={
          mode === "automatic" ? (
            <>
              <p className="text-sm break-all text-muted">
                {host
                  ? t("settings.account.server.using", { host })
                  : t("settings.account.server.notFound")}
              </p>
              {errorLine}
            </>
          ) : (
            <>
              <form className="flex gap-2" onSubmit={submit} noValidate>
                <Input
                  type="url"
                  inputMode="url"
                  autoComplete="off"
                  spellCheck={false}
                  aria-label={t("settings.account.server.title")}
                  aria-invalid={error ? true : undefined}
                  aria-describedby={error ? errorId : undefined}
                  className="min-w-0 flex-1"
                  title={draft}
                  value={draft}
                  disabled={locked || busy || saving}
                  onChange={(event) => {
                    setDraft(event.target.value);
                    setError(null);
                  }}
                />
                {unsaved && (
                  <Button
                    type="submit"
                    variant="secondary"
                    disabled={busy || saving}
                  >
                    {t("common.save")}
                  </Button>
                )}
              </form>
              {errorLine}
            </>
          )
        }
      >
        <Segmented
          size="sm"
          label={t("settings.account.server.title")}
          value={mode}
          onChange={choose}
          disabled={locked || busy || saving}
          options={[
            {
              value: "automatic",
              label: t("settings.account.server.automatic"),
            },
            { value: "custom", label: t("settings.account.server.custom") },
          ]}
        />
      </SettingContainer>
    </SettingsGroup>
  );
};
