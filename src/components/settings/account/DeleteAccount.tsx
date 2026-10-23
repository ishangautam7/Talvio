import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { useAccountAction } from "./useAccountAction";

interface DeleteAccountProps {
  email: string;
}

/** The last thing on the tab, behind a confirmation that says what goes. */
export const DeleteAccount: React.FC<DeleteAccountProps> = ({ email }) => {
  const { t } = useTranslation();
  const { busy, run } = useAccountAction();
  const [confirming, setConfirming] = useState(false);
  const deleting = busy !== null;

  return (
    <div className="flex justify-end">
      <Button
        variant="danger-ghost"
        size="sm"
        onClick={() => setConfirming(true)}
      >
        {t("settings.account.delete.button")}
      </Button>

      <Dialog
        open={confirming}
        onClose={() => !deleting && setConfirming(false)}
        size="sm"
        title={t("settings.account.delete.title")}
        description={<span className="break-words">{email}</span>}
        footer={
          <>
            <Button
              variant="ghost"
              size="sm"
              className="ms-auto"
              disabled={deleting}
              onClick={() => setConfirming(false)}
            >
              {t("common.cancel")}
            </Button>
            <Button
              variant="danger"
              size="sm"
              disabled={deleting}
              onClick={() =>
                void run("delete", () => commands.accountDelete()).finally(() =>
                  setConfirming(false),
                )
              }
            >
              {deleting
                ? t("settings.account.delete.deleting")
                : t("settings.account.delete.button")}
            </Button>
          </>
        }
      >
        <p className="text-sm leading-relaxed text-body">
          {t("settings.account.delete.body")}
        </p>
      </Dialog>
    </div>
  );
};
