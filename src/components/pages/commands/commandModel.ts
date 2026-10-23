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
 * The model to pick with a provider chosen for commands: the one the
 * assistant already runs on it, else the one cleanup does, else none (the
 * model field is left as it is). A model set up for something else on the
 * same provider is one its key is known to reach.
 */
export function suggestedRouterModel(
  settings: AppSettings | null | undefined,
  providerId: string,
): string | null {
  const assistant = settings?.assistant_models?.[providerId]?.trim();
  if (assistant) return assistant;
  const cleanup = settings?.post_process_models?.[providerId]?.trim();
  return cleanup || null;
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
