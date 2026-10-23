import React from "react";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "@/components/ui/SettingContainer";
import { SettingsGroup } from "@/components/ui/SettingsGroup";

interface ServerSettingProps {
  /** The server in use, as discovery found it; empty while it has found none. */
  serverUrl: string;
}

/**
 * Which account server this app signs in to, read-only: the address always
 * comes from Talvio's discovery address (`src-tauri/src/account/discovery.rs`)
 * and is never chosen or typed here.
 */
export const ServerSetting: React.FC<ServerSettingProps> = ({ serverUrl }) => {
  const { t } = useTranslation();
  const host = serverUrl.replace(/^https?:\/\//, "");

  return (
    <SettingsGroup>
      <SettingContainer
        title={t("settings.account.server.title")}
        grouped
        details={
          <p className="text-sm break-all text-muted">
            {host
              ? t("settings.account.server.using", { host })
              : t("settings.account.server.notFound")}
          </p>
        }
      >
        {null}
      </SettingContainer>
    </SettingsGroup>
  );
};
