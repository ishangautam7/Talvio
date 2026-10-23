import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Monitor, Smartphone } from "lucide-react";
import type { AccountDevice } from "@/bindings";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { formatRelativeTime } from "@/utils/dateFormat";
import { isPhone, platformName } from "./account";

interface DeviceListProps {
  devices: AccountDevice[];
  removing: boolean;
  onRemove: (deviceId: string) => Promise<void>;
  /** A change to whether a phone may use this computer is in flight. */
  approving: boolean;
  onApprove: (deviceId: string, approved: boolean) => Promise<void>;
}

/**
 * Every device signed in to the account, in the server's order: this one
 * first. Each phone also says whether it may dictate and type here, with the
 * control to change that; computers have no such thing.
 */
export const DeviceList: React.FC<DeviceListProps> = ({
  devices,
  removing,
  onRemove,
  approving,
  onApprove,
}) => {
  const { t, i18n } = useTranslation();
  const [confirming, setConfirming] = useState<AccountDevice | null>(null);
  const listRef = useRef<HTMLUListElement>(null);

  // The dialog gives focus back to the row's Remove button, which is gone
  // once the device is: keep focus in the Settings window, on the list.
  useEffect(() => {
    if (!confirming && document.activeElement === document.body) {
      listRef.current?.focus();
    }
  }, [confirming]);

  // The list arrives with the connection, so it is empty only before then.
  if (devices.length === 0) {
    return (
      <p className="px-5 py-4 text-sm text-muted">
        {t("settings.account.devices.empty")}
      </p>
    );
  }

  const detail = (device: AccountDevice) => {
    if (device.current) return t("settings.account.devices.thisComputer");
    if (device.online) return t("settings.account.devices.online");
    return t("settings.account.devices.lastSeen", {
      time: formatRelativeTime(
        String(Math.floor(device.last_seen_at / 1000)),
        i18n.language,
      ),
    });
  };

  return (
    <>
      <ul
        ref={listRef}
        tabIndex={-1}
        aria-label={t("settings.account.devices.title")}
        className="divide-y divide-hairline focus:outline-none"
      >
        {devices.map((device) => {
          const Icon = isPhone(device) ? Smartphone : Monitor;
          const phone = isPhone(device) && !device.current;
          return (
            <li key={device.id} className="flex items-center gap-3 px-5 py-3">
              <div className="relative grid h-9 w-9 shrink-0 place-items-center rounded-lg bg-surface-strong text-muted">
                <Icon className="h-[18px] w-[18px]" aria-hidden="true" />
                {device.online && (
                  <span
                    aria-hidden="true"
                    className="absolute -end-0.5 -bottom-0.5 h-2.5 w-2.5 rounded-full bg-success ring-2 ring-surface"
                  />
                )}
              </div>
              <div className="min-w-0 flex-1">
                <p
                  className="truncate text-sm font-medium text-ink"
                  title={device.name}
                >
                  {device.name}
                </p>
                <p className="truncate text-xs text-muted">
                  {`${platformName(device.platform)} · ${detail(device)}`}
                </p>
                {phone && (
                  <p className="truncate text-xs text-muted">
                    {device.approved
                      ? t("settings.account.devices.allowed")
                      : t("settings.account.devices.notAllowed")}
                  </p>
                )}
              </div>
              {phone &&
                (device.approved ? (
                  <Button
                    variant="ghost"
                    size="sm"
                    disabled={approving}
                    aria-label={t(
                      "settings.account.devices.removeAccessLabel",
                      {
                        name: device.name,
                      },
                    )}
                    onClick={() => void onApprove(device.id, false)}
                  >
                    {t("settings.account.devices.removeAccess")}
                  </Button>
                ) : (
                  <Button
                    variant="secondary"
                    size="sm"
                    disabled={approving}
                    aria-label={t("settings.account.devices.allowLabel", {
                      name: device.name,
                    })}
                    onClick={() => void onApprove(device.id, true)}
                  >
                    {t("settings.account.devices.allow")}
                  </Button>
                ))}
              {!device.current && (
                <Button
                  variant="ghost"
                  size="sm"
                  aria-label={t("settings.account.devices.removeLabel", {
                    name: device.name,
                  })}
                  onClick={() => setConfirming(device)}
                >
                  {t("common.remove")}
                </Button>
              )}
            </li>
          );
        })}
      </ul>

      <Dialog
        open={confirming !== null}
        onClose={() => !removing && setConfirming(null)}
        size="sm"
        title={t("settings.account.devices.removeTitle")}
        description={<span className="break-words">{confirming?.name}</span>}
        footer={
          <>
            <Button
              variant="ghost"
              size="sm"
              className="ms-auto"
              disabled={removing}
              onClick={() => setConfirming(null)}
            >
              {t("common.cancel")}
            </Button>
            <Button
              variant="danger"
              size="sm"
              disabled={removing}
              onClick={() => {
                if (confirming) {
                  void onRemove(confirming.id).finally(() =>
                    setConfirming(null),
                  );
                }
              }}
            >
              {t("settings.account.devices.removeConfirm")}
            </Button>
          </>
        }
      >
        <p className="text-sm leading-relaxed text-body">
          {t("settings.account.devices.removeBody")}
        </p>
      </Dialog>
    </>
  );
};
