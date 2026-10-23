import React, { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type CommandModelInUse } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { Page, PageHeader } from "@/components/ui/Page";
import { Switch } from "@/components/ui/Switch";
import { Input } from "@/components/ui/Input";
import { SettingsGroup } from "@/components/ui/SettingsGroup";
import { SettingContainer } from "@/components/ui/SettingContainer";
import { ToggleSwitch } from "@/components/ui/ToggleSwitch";
import { LogoSelect, type LogoSelectOption } from "@/components/ui/LogoSelect";
import { ProviderTile } from "@/components/icons/ProviderLogos";
import { LLM_REQUIRES_KEY } from "@/components/shell/ProviderSetup";
import { useNavigation } from "@/components/shell/navigation";
import { PushToTalk } from "@/components/settings/PushToTalk";
import { RowAction, ShortcutRow } from "@/components/settings/ShortcutRow";
import { shortcutInfo } from "@/components/settings/shortcutInfo";
import { CommandTestBox } from "@/components/settings/debug/CommandTestBox";
import {
  modelInUseLine,
  routerProviders,
  suggestedRouterModel,
} from "./commands/commandModel";

/** What the examples list shows, in order. */
const EXAMPLES = ["youtube", "song", "volume", "switch"] as const;

/**
 * Which provider and model route a command, and which one really does: the
 * chosen provider without a key hands commands to the assistant's model
 * (`command_mode::llm::effective`), and the line under the row says so.
 */
const CommandModelRow: React.FC = () => {
  const { t } = useTranslation();
  const { settings, updateSetting, refreshSettings } = useSettings();
  const { openModelSlot } = useNavigation();
  const providerId = settings?.command_router_provider_id ?? "";
  const storedModel = settings?.command_router_model ?? "";
  const [model, setModel] = useState(storedModel);
  const [inUse, setInUse] = useState<CommandModelInUse | null>(null);

  useEffect(() => setModel(storedModel), [storedModel]);

  // Asked again whenever a setting that decides it may have changed: the
  // chosen provider or model, a key, or the assistant's own choice.
  useEffect(() => {
    let live = true;
    void commands
      .commandModelInUse()
      .then((value) => live && setInUse(value))
      .catch(() => live && setInUse(null));
    return () => {
      live = false;
    };
  }, [settings]);

  const providers = useMemo(() => routerProviders(settings), [settings]);
  const options = useMemo<LogoSelectOption[]>(
    () =>
      providers.map((provider) => ({
        value: provider.id,
        label: provider.label,
        hint:
          LLM_REQUIRES_KEY.has(provider.id) &&
          !settings?.post_process_api_keys?.[provider.id]?.trim()
            ? t("pickers.needsKey")
            : undefined,
        icon: <ProviderTile id={provider.id} kind="llm" size="sm" />,
      })),
    [providers, settings, t],
  );

  const chooseProvider = async (id: string) => {
    if (id === providerId) return;
    await updateSetting("command_router_provider_id", id);
    const suggested = suggestedRouterModel(settings, id);
    if (suggested) await updateSetting("command_router_model", suggested);
    void refreshSettings();
  };

  const commitModel = async () => {
    if (model === storedModel) return;
    await updateSetting("command_router_model", model);
    // Read back what was saved: an emptied model goes back to the default.
    void refreshSettings();
  };

  const chosenLabel =
    providers.find((provider) => provider.id === providerId)?.label ??
    providerId;
  const line = modelInUseLine(inUse, chosenLabel);

  return (
    <SettingContainer
      title={t("commandsPage.model.title")}
      description={t("commandsPage.model.info")}
      grouped={true}
      details={
        line && (
          <div className="flex flex-wrap items-center gap-2 text-[0.8125rem] text-muted">
            <span>{t(line.key, line.values)}</span>
            {!inUse?.ready && (
              <RowAction onClick={() => openModelSlot("assistant")}>
                {t("commandsPage.model.setUp")}
              </RowAction>
            )}
          </div>
        )
      }
    >
      <div className="flex flex-wrap items-center justify-end gap-2">
        <LogoSelect
          options={options}
          value={providerId || null}
          onChange={(next) => void chooseProvider(next)}
          placeholder={t("pickers.choose")}
          className="w-[12rem]"
          ariaLabel={t("commandsPage.model.provider")}
        />
        <Input
          type="text"
          variant="compact"
          className="w-[12rem]"
          value={model}
          aria-label={t("commandsPage.model.model")}
          placeholder={t("settings.debug.commandMode.model.placeholder")}
          spellCheck={false}
          onChange={(e) => setModel(e.target.value)}
          onBlur={() => void commitModel()}
          onKeyDown={(e) => {
            if (e.key === "Enter") void commitModel();
          }}
        />
      </div>
    </SettingContainer>
  );
};

/** A few things to say, so the feature is not a blank box. */
const Examples: React.FC = () => {
  const { t } = useTranslation();
  return (
    <SettingsGroup title={t("commandsPage.examples.title")}>
      <ul className="divide-y divide-hairline">
        {EXAMPLES.map((example) => (
          <li key={example} className="px-5 py-3 text-sm text-ink">
            {t(`commandsPage.examples.items.${example}`)}
          </li>
        ))}
      </ul>
    </SettingsGroup>
  );
};

/**
 * Commands: say what you want done ("open YouTube", "play Believer") and
 * Talvio does it on this computer. The switch, the keys, what runs commands,
 * a box to try one by typing, and a few things to say. No banner: a plain
 * shortcut row holds the keys as well as one would.
 */
export const CommandsPage: React.FC = () => {
  const { t } = useTranslation();
  const { settings, updateSetting, isUpdating } = useSettings();
  const enabled = settings?.command_mode_enabled ?? false;

  return (
    <Page>
      <PageHeader
        title={t("nav.commands")}
        description={t("commandsPage.description")}
        actions={
          <div className="inline-flex items-center gap-2.5 text-sm font-medium text-ink">
            <span aria-hidden="true">
              {enabled ? t("common.on") : t("common.off")}
            </span>
            <Switch
              checked={enabled}
              onChange={(value) =>
                void updateSetting("command_mode_enabled", value)
              }
              disabled={isUpdating("command_mode_enabled")}
              label={t("commandsPage.enable")}
            />
          </div>
        }
      />

      {!enabled ? (
        <>
          <p className="rounded-2xl border border-hairline bg-surface-muted px-5 py-4 text-sm text-body">
            {t("commandsPage.offNotice")}
          </p>
          <div className="mt-8">
            <Examples />
          </div>
        </>
      ) : (
        <section className="space-y-8">
          <SettingsGroup>
            <ShortcutRow
              shortcutId="command"
              title={t("commandsPage.shortcut.title")}
              info={shortcutInfo(t, settings, "command")}
            />
            <PushToTalk grouped={true} setting="command_push_to_talk" />
            <CommandModelRow />
            <ToggleSwitch
              checked={settings?.command_dry_run ?? false}
              onChange={(value) => void updateSetting("command_dry_run", value)}
              isUpdating={isUpdating("command_dry_run")}
              label={t("commandsPage.dryRun.title")}
              description={t("commandsPage.dryRun.info")}
              grouped={true}
            />
          </SettingsGroup>
          <SettingsGroup title={t("commandsPage.test.group")}>
            <CommandTestBox
              title={t("commandsPage.test.title")}
              description={t("commandsPage.test.info")}
            />
          </SettingsGroup>
          <Examples />
        </section>
      )}
    </Page>
  );
};
