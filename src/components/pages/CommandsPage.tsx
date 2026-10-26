import React, { useEffect, useId, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { commands, type CommandModelInUse, type ModelChoice } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { OffNotice } from "@/components/ui/OffNotice";
import { Page, PageHeader } from "@/components/ui/Page";
import { Switch } from "@/components/ui/Switch";
import { Input } from "@/components/ui/Input";
import { SettingsGroup } from "@/components/ui/SettingsGroup";
import { SettingContainer } from "@/components/ui/SettingContainer";
import { ToggleSwitch } from "@/components/ui/ToggleSwitch";
import { LogoSelect, type LogoSelectOption } from "@/components/ui/LogoSelect";
import { ProviderTile } from "@/components/icons/ProviderLogos";
import {
  ChoiceField,
  type Choice,
  errorText,
  KeyLink,
  LLM_KEY_PAGES,
  LLM_REQUIRES_KEY,
  named,
  resolveChoice,
  settle,
} from "@/components/shell/ProviderSetup";
import { useNavigation } from "@/components/shell/navigation";
import { PushToTalk } from "@/components/settings/PushToTalk";
import { RowAction, ShortcutRow } from "@/components/settings/ShortcutRow";
import { shortcutInfo } from "@/components/settings/shortcutInfo";
import { CommandTestBox } from "@/components/settings/debug/CommandTestBox";
import {
  keyedFirst,
  modelBelongsElsewhere,
  modelInUseLine,
  providerWithSavedKey,
  routerProviders,
  suggestedRouterModel,
} from "./commands/commandModel";

/** What the examples list shows, in order. */
const EXAMPLES = ["youtube", "song", "volume", "switch"] as const;

/**
 * Which provider and model route a command, and which one really does: the
 * chosen provider without a key hands commands to a provider set up elsewhere (the assistant's, then AI cleanup's), and the page moves to it
 * (`command_mode::llm::effective`), and the line under the row says so.
 * Under it, the provider's key when it needs one (shared with AI cleanup and
 * the assistant, so entering it here sets it up there too) and the model,
 * picked from what the provider lists or typed.
 */
const CommandModelRow: React.FC = () => {
  const { t } = useTranslation();
  const {
    settings,
    updateSetting,
    refreshSettings,
    refreshPostProcessReadiness,
  } = useSettings();
  const { openModelSlot } = useNavigation();
  const providerId = settings?.command_router_provider_id ?? "";
  const storedModel = settings?.command_router_model ?? "";
  const storedKey = settings?.post_process_api_keys?.[providerId]?.trim() ?? "";
  const needsKey = LLM_REQUIRES_KEY.has(providerId);
  const [model, setModel] = useState(storedModel);
  const [key, setKey] = useState(storedKey);
  const [inUse, setInUse] = useState<CommandModelInUse | null>(null);
  const [models, setModels] = useState<ModelChoice[]>([]);
  const [loading, setLoading] = useState(false);
  const [listError, setListError] = useState<string | null>(null);
  const keyId = useId();
  const modelId = useId();

  useEffect(() => setModel(storedModel), [storedModel]);
  useEffect(() => setKey(storedKey), [storedKey, providerId]);

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

  const providers = useMemo(
    () => keyedFirst(settings, routerProviders(settings)),
    [settings],
  );
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

  /** The provider's models, once there is a key to ask with. */
  const loadModels = async (id = providerId) => {
    if (!id) return;
    setLoading(true);
    setListError(null);
    try {
      const list = await settle(commands.fetchPostProcessModels(id));
      setModels(list);
      if (list.length === 0) setListError(t("pickers.setup.noModels"));
    } catch (error) {
      setModels([]);
      setListError(errorText(error));
    } finally {
      setLoading(false);
    }
  };

  // A new provider: its list, if it can be asked.
  useEffect(() => {
    setModels([]);
    setListError(null);
    if (providerId && (!needsKey || storedKey)) void loadModels(providerId);
  }, [providerId]);

  // A store still on the Anthropic default under another provider gets that
  // provider's own model instead.
  useEffect(() => {
    if (!providerId || !modelBelongsElsewhere(providerId, storedModel)) return;
    const suggested = suggestedRouterModel(settings, providerId);
    if (suggested && suggested !== storedModel) {
      void updateSetting("command_router_model", suggested);
    }
  }, [providerId, storedModel]);

  const chooseProvider = async (id: string) => {
    if (id === providerId) return;
    await updateSetting("command_router_provider_id", id);
    const suggested = suggestedRouterModel(settings, id);
    if (suggested) await updateSetting("command_router_model", suggested);
    void refreshSettings();
  };

  // A provider nobody gave a key (the shipped Anthropic default, say) moves to
  // one set up elsewhere, the assistant's first, so commands run on a key
  // already entered instead of asking for another. Once, when the page first
  // has settings: a keyless provider picked here afterwards is someone about
  // to type its key, and must not be switched away under them.
  const adoptChecked = useRef(false);
  useEffect(() => {
    if (!settings || adoptChecked.current) return;
    adoptChecked.current = true;
    const adopt = providerWithSavedKey(settings, (id) =>
      LLM_REQUIRES_KEY.has(id),
    );
    if (adopt) void chooseProvider(adopt);
  }, [settings]);

  const choices: Choice[] = models.map((item) => ({
    value: item.id,
    label: named(item.id, item.label),
  }));

  const commitModel = async (next: string) => {
    const chosen = resolveChoice(next, choices);
    setModel(chosen);
    if (chosen === storedModel) return;
    await updateSetting("command_router_model", chosen);
    // Read back what was saved: an emptied model goes back to the default.
    void refreshSettings();
  };

  const commitKey = async () => {
    const next = key.trim();
    if (next === storedKey) return;
    try {
      await settle(commands.changePostProcessApiKeySetting(providerId, next));
      await refreshSettings();
      void refreshPostProcessReadiness();
      if (next) void loadModels(providerId);
    } catch (error) {
      toast.error(errorText(error));
    }
  };

  const chosenLabel =
    providers.find((provider) => provider.id === providerId)?.label ??
    providerId;
  const line = modelInUseLine(inUse, chosenLabel);
  const keyPage = LLM_KEY_PAGES[providerId];

  return (
    <SettingContainer
      title={t("commandsPage.model.title")}
      description={t("commandsPage.model.info")}
      grouped={true}
      details={
        <div className="space-y-4">
          {line && (
            <div className="flex flex-wrap items-center gap-2 text-[0.8125rem] text-muted">
              <span>{t(line.key, line.values)}</span>
              {!inUse?.ready && (
                <RowAction onClick={() => openModelSlot("assistant")}>
                  {t("commandsPage.model.setUp")}
                </RowAction>
              )}
            </div>
          )}
          {providerId && (
            <div className="grid max-w-xl gap-4">
              {needsKey && (
                <div className="space-y-1.5">
                  <label
                    htmlFor={keyId}
                    className="block text-[0.8125rem] font-medium text-ink"
                  >
                    {t("commandsPage.model.key", { provider: chosenLabel })}
                  </label>
                  <Input
                    id={keyId}
                    type="password"
                    value={key}
                    onChange={(event) => setKey(event.target.value)}
                    onBlur={() => void commitKey()}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") void commitKey();
                    }}
                    placeholder={t("pickers.setup.keyPlaceholder")}
                    autoComplete="off"
                    spellCheck={false}
                    className="h-10 w-full"
                  />
                  <div className="flex flex-wrap items-center gap-3 text-xs leading-snug text-muted">
                    <span>{t("commandsPage.model.keyShared")}</span>
                    {keyPage && <KeyLink url={keyPage} />}
                  </div>
                </div>
              )}
              <ChoiceField
                id={modelId}
                label={t("commandsPage.model.model")}
                value={model}
                onChange={(next) =>
                  // A pick from the list is final; typing waits for the blur.
                  choices.some((choice) => choice.value === next)
                    ? void commitModel(next)
                    : setModel(next)
                }
                onCommit={(next) => void commitModel(next)}
                choices={choices}
                placeholder={t("settings.debug.commandMode.model.placeholder")}
                loadLabel={t("pickers.setup.loadModels")}
                loading={loading}
                onLoad={() => void loadModels()}
                error={listError}
              />
            </div>
          )}
        </div>
      }
    >
      <LogoSelect
        options={options}
        value={providerId || null}
        onChange={(next) => void chooseProvider(next)}
        placeholder={t("pickers.choose")}
        className="w-[12rem]"
        ariaLabel={t("commandsPage.model.provider")}
      />
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
        feature="commands"
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
          <OffNotice>{t("commandsPage.offNotice")}</OffNotice>
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
              checked={settings?.command_confirm_irreversible ?? false}
              onChange={(value) =>
                void updateSetting("command_confirm_irreversible", value)
              }
              isUpdating={isUpdating("command_confirm_irreversible")}
              label={t("commandsPage.confirmIrreversible.title")}
              description={t("commandsPage.confirmIrreversible.info")}
              grouped={true}
            />
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
