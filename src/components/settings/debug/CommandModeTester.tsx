import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { FileText } from "lucide-react";
import { commands } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { SettingContainer } from "@/components/ui/SettingContainer";
import { ToggleSwitch } from "@/components/ui/ToggleSwitch";
import { CommandTestBox } from "./CommandTestBox";

type TextSettingKey =
  | "command_router_provider_id"
  | "command_router_model"
  | "command_google_account"
  | "command_search_url"
  | "command_agent_model";

type NumberSettingKey =
  | "command_agent_max_steps"
  | "command_agent_max_cost_usd";

/**
 * A text setting saved when the field is left or Enter is pressed, not on
 * every keystroke: the backend normalises the value (an emptied model goes
 * back to the default), and saving half-typed ids would route commands
 * through them in between.
 */
const CommandTextSetting: React.FC<{
  settingKey: TextSettingKey;
  title: string;
  description: string;
  placeholder: string;
}> = ({ settingKey, title, description, placeholder }) => {
  const { settings, updateSetting, refreshSettings } = useSettings();
  const stored = settings?.[settingKey] ?? "";
  const [value, setValue] = useState(stored);

  useEffect(() => setValue(stored), [stored]);

  const commit = async () => {
    if (value === stored) return;
    await updateSetting(settingKey, value);
    // Read back what was saved, since the backend may have normalised it.
    void refreshSettings();
  };

  return (
    <SettingContainer title={title} description={description} grouped={true}>
      <Input
        type="text"
        variant="compact"
        className="w-[260px]"
        value={value}
        placeholder={placeholder}
        spellCheck={false}
        onChange={(e) => setValue(e.target.value)}
        onBlur={() => void commit()}
        onKeyDown={(e) => {
          if (e.key === "Enter") void commit();
        }}
      />
    </SettingContainer>
  );
};

/**
 * A number setting, saved like `CommandTextSetting` when the field is left
 * or Enter is pressed. The backend clamps it, so the field shows what was
 * actually saved once it is read back.
 */
const CommandNumberSetting: React.FC<{
  settingKey: NumberSettingKey;
  title: string;
  description: string;
  min: number;
  max: number;
  step: number;
}> = ({ settingKey, title, description, min, max, step }) => {
  const { settings, updateSetting, refreshSettings } = useSettings();
  const stored = settings?.[settingKey];
  const shown = stored === undefined ? "" : String(stored);
  const [value, setValue] = useState(shown);

  useEffect(() => setValue(shown), [shown]);

  const commit = async () => {
    const parsed = Number(value);
    if (value.trim() === "" || !Number.isFinite(parsed)) {
      setValue(shown);
      return;
    }
    if (parsed === stored) return;
    await updateSetting(settingKey, parsed);
    void refreshSettings();
  };

  return (
    <SettingContainer title={title} description={description} grouped={true}>
      <Input
        type="number"
        variant="compact"
        className="w-[120px]"
        value={value}
        min={min}
        max={max}
        step={step}
        onChange={(e) => setValue(e.target.value)}
        onBlur={() => void commit()}
        onKeyDown={(e) => {
          if (e.key === "Enter") void commit();
        }}
      />
    </SettingContainer>
  );
};

/**
 * Settings → Debug → Command mode: the test box (`CommandTestBox`, also on
 * the Commands page) and every command-mode setting, the ones the Commands
 * page leaves out included (the agent's limits, the Google account, the
 * search page, contacts).
 */
export const CommandModeTester: React.FC = () => {
  const { t } = useTranslation();
  const { settings, updateSetting, isUpdating } = useSettings();
  const savedDryRun = settings?.command_dry_run ?? false;
  const openContacts = async () => {
    const result = await commands.commandOpenContactsFile();
    if (result.status !== "ok") {
      toast.error(
        t("settings.debug.commandMode.contacts.openFailed", {
          error: result.error,
        }),
      );
    }
  };

  return (
    <>
      <ToggleSwitch
        checked={settings?.command_mode_enabled ?? false}
        onChange={(enabled) =>
          void updateSetting("command_mode_enabled", enabled)
        }
        isUpdating={isUpdating("command_mode_enabled")}
        label={t("settings.debug.commandMode.enabled.title")}
        description={t("settings.debug.commandMode.enabled.description")}
        grouped={true}
      />
      <CommandTestBox
        title={t("settings.debug.commandMode.input.title")}
        description={t("settings.debug.commandMode.input.description")}
      />
      <ToggleSwitch
        checked={savedDryRun}
        onChange={(enabled) => void updateSetting("command_dry_run", enabled)}
        isUpdating={isUpdating("command_dry_run")}
        label={t("settings.debug.commandMode.dryRunSetting.title")}
        description={t("settings.debug.commandMode.dryRunSetting.description")}
        grouped={true}
      />
      <CommandTextSetting
        settingKey="command_router_provider_id"
        title={t("settings.debug.commandMode.provider.title")}
        description={t("settings.debug.commandMode.provider.description")}
        placeholder={t("settings.debug.commandMode.provider.placeholder")}
      />
      <CommandTextSetting
        settingKey="command_router_model"
        title={t("settings.debug.commandMode.model.title")}
        description={t("settings.debug.commandMode.model.description")}
        placeholder={t("settings.debug.commandMode.model.placeholder")}
      />
      <ToggleSwitch
        checked={settings?.command_auto_escalate ?? true}
        onChange={(enabled) =>
          void updateSetting("command_auto_escalate", enabled)
        }
        isUpdating={isUpdating("command_auto_escalate")}
        label={t("settings.debug.commandMode.autoEscalate.title")}
        description={t("settings.debug.commandMode.autoEscalate.description")}
        grouped={true}
      />
      <CommandTextSetting
        settingKey="command_agent_model"
        title={t("settings.debug.commandMode.agentModel.title")}
        description={t("settings.debug.commandMode.agentModel.description")}
        placeholder={t("settings.debug.commandMode.agentModel.placeholder")}
      />
      <CommandNumberSetting
        settingKey="command_agent_max_steps"
        title={t("settings.debug.commandMode.agentMaxSteps.title")}
        description={t("settings.debug.commandMode.agentMaxSteps.description")}
        min={1}
        max={60}
        step={1}
      />
      <CommandNumberSetting
        settingKey="command_agent_max_cost_usd"
        title={t("settings.debug.commandMode.agentMaxCost.title")}
        description={t("settings.debug.commandMode.agentMaxCost.description")}
        min={0.01}
        max={20}
        step={0.05}
      />
      <CommandTextSetting
        settingKey="command_google_account"
        title={t("settings.debug.commandMode.googleAccount.title")}
        description={t("settings.debug.commandMode.googleAccount.description")}
        placeholder={t("settings.debug.commandMode.googleAccount.placeholder")}
      />
      <CommandTextSetting
        settingKey="command_search_url"
        title={t("settings.debug.commandMode.searchUrl.title")}
        description={t("settings.debug.commandMode.searchUrl.description")}
        placeholder={t("settings.debug.commandMode.searchUrl.placeholder")}
      />
      <SettingContainer
        title={t("settings.debug.commandMode.contacts.title")}
        description={t("settings.debug.commandMode.contacts.description")}
        grouped={true}
      >
        <Button
          variant="secondary"
          size="sm"
          onClick={() => void openContacts()}
        >
          <FileText className="h-3.5 w-3.5" aria-hidden="true" />
          {t("settings.debug.commandMode.contacts.open")}
        </Button>
      </SettingContainer>
    </>
  );
};
