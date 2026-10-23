import React from "react";
import { useTranslation } from "react-i18next";
import { Smartphone } from "lucide-react";
import {
  commands,
  type AccountConnection,
  type AccountStatus,
  type AccountUser,
} from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { Button } from "@/components/ui/Button";
import { InfoTip } from "@/components/ui/InfoTip";
import { SettingsGroup } from "@/components/ui/SettingsGroup";
import { ToggleSwitch } from "@/components/ui/ToggleSwitch";
import { avatarLetter, displayName, isPhone } from "./account";
import { DeviceList } from "./DeviceList";
import { useAccountAction } from "./useAccountAction";

const CONNECTION_DOT: Record<AccountConnection, string> = {
  online: "bg-success",
  connecting: "bg-warning motion-safe:animate-pulse",
  offline: "bg-muted-soft",
};

interface SignedInProps {
  status: AccountStatus;
  user: AccountUser;
}

/** Signed in: who, whether the phone can reach this computer, key sync, and the devices. */
export const SignedIn: React.FC<SignedInProps> = ({ status, user }) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const { busy, run } = useAccountAction();
  const name = displayName(user);
  const remoteInput =
    getSetting("remote_input_enabled") ?? status.remote_input_enabled;
  const remoteWindows = getSetting("remote_windows_enabled") ?? false;
  const syncKeys = getSetting("account_sync_secrets") ?? true;

  return (
    <>
      <section className="flex flex-wrap items-center gap-x-4 gap-y-3 rounded-2xl border border-hairline bg-surface px-5 py-4 elev-card">
        <div
          aria-hidden="true"
          className="grid h-11 w-11 shrink-0 place-items-center rounded-full bg-accent/12 font-display text-lg text-accent"
        >
          {avatarLetter(user)}
        </div>
        <div className="min-w-[10rem] flex-1">
          <p
            className="truncate text-[0.9375rem] font-medium text-ink"
            title={name}
          >
            {name}
          </p>
          {name !== user.email && (
            <p className="truncate text-sm text-muted" title={user.email}>
              {user.email}
            </p>
          )}
          <p className="mt-1 flex items-center gap-1.5 text-[0.8125rem] text-muted">
            <span
              aria-hidden="true"
              className={`h-2 w-2 shrink-0 rounded-full ${CONNECTION_DOT[status.connection]}`}
            />
            {t(`settings.account.connection.${status.connection}`)}
            {status.connection === "offline" && (
              <InfoTip text={t("settings.account.connection.offlineHint")} />
            )}
          </p>
        </div>
        <Button
          variant="secondary"
          size="sm"
          disabled={busy !== null}
          onClick={() => void run("sign_out", () => commands.accountSignOut())}
        >
          {busy === "sign_out"
            ? t("settings.account.signingOut")
            : t("settings.account.signOut")}
        </Button>
      </section>

      <SettingsGroup title={t("settings.account.phone.title")}>
        <ToggleSwitch
          checked={remoteInput}
          onChange={(enabled) =>
            void updateSetting("remote_input_enabled", enabled)
          }
          isUpdating={isUpdating("remote_input_enabled")}
          label={t("settings.account.phone.remoteInput")}
          description={t("settings.account.phone.remoteInputHint")}
          grouped
        />
        <ToggleSwitch
          checked={remoteInput && remoteWindows}
          onChange={(enabled) =>
            void updateSetting("remote_windows_enabled", enabled)
          }
          isUpdating={isUpdating("remote_windows_enabled")}
          disabled={!remoteInput}
          label={t("settings.account.phone.remoteWindows")}
          description={t("settings.account.phone.remoteWindowsHint")}
          grouped
        />
        {!status.devices.some(isPhone) && (
          <div className="flex items-center gap-3 px-5 py-3.5">
            <Smartphone
              className="h-4 w-4 shrink-0 text-muted"
              aria-hidden="true"
            />
            <p className="min-w-0 flex-1 text-sm text-body break-words">
              {t("settings.account.phone.install", { email: user.email })}
            </p>
          </div>
        )}
      </SettingsGroup>

      <SettingsGroup title={t("settings.account.keys.title")}>
        <ToggleSwitch
          checked={syncKeys}
          onChange={(enabled) =>
            void updateSetting("account_sync_secrets", enabled)
          }
          isUpdating={isUpdating("account_sync_secrets")}
          label={t("settings.account.keys.sync")}
          description={t("settings.account.keys.syncHint")}
          grouped
        />
      </SettingsGroup>

      <SettingsGroup title={t("settings.account.devices.title")}>
        <DeviceList
          devices={status.devices}
          removing={busy === "remove"}
          onRemove={(deviceId) =>
            run("remove", () => commands.accountRemoveDevice(deviceId))
          }
          approving={busy === "approve"}
          onApprove={(deviceId, approved) =>
            run("approve", () =>
              commands.accountSetDeviceApproved(deviceId, approved),
            )
          }
        />
      </SettingsGroup>
    </>
  );
};
