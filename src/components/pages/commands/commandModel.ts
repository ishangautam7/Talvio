import { FREE_PROVIDER_ID } from "@/lib/freeCredit";
import type {
  AppSettings,
  CommandModelInUse,
  PostProcessProvider,
} from "@/bindings";

/**
 * Pure helpers for the Commands page's "Runs on" row, kept out of the
 * component so they can be tested without React or Tauri.
 */

/** Providers a command cannot route through: the built-in engine and Apple
 *  Intelligence take no tool calls (`command_mode::llm::usable`). */
const NOT_A_ROUTER = new Set(["builtin", "apple_intelligence"]);

/** The providers a command can run on: every HTTP provider but those. */
export function routerProviders(
  settings: AppSettings | null | undefined,
): PostProcessProvider[] {
  return (settings?.post_process_providers ?? []).filter(
    (provider) =>
      !NOT_A_ROUTER.has(provider.id) &&
      provider.id !== FREE_PROVIDER_ID &&
      provider.base_url.trim().toLowerCase().startsWith("http"),
  );
}

/**
 * A small, fast model that calls tools well, for providers where we know
 * one; the id is the provider's own spelling. Only a starting point: the
 * picker lists what the provider really offers.
 */
export const ROUTER_MODEL_DEFAULTS: Record<string, string> = {
  anthropic: "claude-haiku-5-5",
  openrouter: "anthropic/claude-haiku-4.5",
  openai: "gpt-5-mini",
  gemini: "gemini-2.5-flash",
};

/**
 * The model to pick with a provider chosen for commands: the one the
 * assistant already runs on it, else the one cleanup does, else a known fast
 * one for that provider, else none (the model is left as it is). A model set
 * up for something else on the same provider is one its key is known to
 * reach.
 */
export function suggestedRouterModel(
  settings: AppSettings | null | undefined,
  providerId: string,
): string | null {
  const assistant = settings?.assistant_models?.[providerId]?.trim();
  if (assistant) return assistant;
  const cleanup = settings?.post_process_models?.[providerId]?.trim();
  return cleanup || ROUTER_MODEL_DEFAULTS[providerId] || null;
}

/**
 * Whether the saved model plainly belongs to another provider: the shipped
 * default is an Anthropic id, which no other provider serves under that name,
 * so a store that switched provider without switching model is fixed.
 */
export function modelBelongsElsewhere(
  providerId: string,
  model: string,
): boolean {
  const value = model.trim();
  return (
    providerId !== "anthropic" && value === ROUTER_MODEL_DEFAULTS.anthropic
  );
}

/** What the line under "Runs on" says, as a translation key and its values. */
export function modelInUseLine(
  inUse: CommandModelInUse | null,
  chosenLabel: string,
): { key: string; values: Record<string, string> } | null {
  if (!inUse) return null;
  const values = {
    provider: inUse.provider_label,
    model: inUse.model,
    chosen: chosenLabel,
  };
  if (!inUse.ready) return { key: "commandsPage.model.needsKey", values };
  if (inUse.borrowed) return { key: "commandsPage.model.borrowed", values };
  return { key: "commandsPage.model.using", values };
}

/** Whether a key is saved for `providerId` (in AI cleanup, the assistant or
 *  here: the three share one set of keys). */
export function hasSavedKey(
  settings: AppSettings | null | undefined,
  providerId: string,
): boolean {
  return Boolean(settings?.post_process_api_keys?.[providerId]?.trim());
}

/**
 * A provider to move commands to when the chosen one needs a key nobody
 * entered: the assistant's provider, else AI cleanup's, else any other with a
 * saved key, as long as there is a model to run on it. `null` when the chosen
 * provider is fine as it is, or nothing better is set up, so a person is
 * never moved off a provider they gave a key.
 */
export function providerWithSavedKey(
  settings: AppSettings | null | undefined,
  requiresKey: (providerId: string) => boolean,
): string | null {
  const chosen = settings?.command_router_provider_id ?? "";
  if (chosen && (!requiresKey(chosen) || hasSavedKey(settings, chosen))) {
    return null;
  }
  const ids = routerProviders(settings).map((provider) => provider.id);
  const candidates = [
    settings?.assistant_provider_id,
    settings?.post_process_provider_id,
    ...ids,
  ];
  for (const id of candidates) {
    if (!id || id === chosen || !ids.includes(id)) continue;
    if (!hasSavedKey(settings, id)) continue;
    if (suggestedRouterModel(settings, id)) return id;
  }
  return null;
}

/** The picker's order: providers with a saved key first, the rest as listed. */
export function keyedFirst<T extends { id: string }>(
  settings: AppSettings | null | undefined,
  providers: T[],
): T[] {
  const keyed = providers.filter((p) => hasSavedKey(settings, p.id));
  const rest = providers.filter((p) => !hasSavedKey(settings, p.id));
  return [...keyed, ...rest];
}
