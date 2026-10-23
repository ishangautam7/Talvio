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
