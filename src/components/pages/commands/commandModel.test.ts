import { describe, expect, test } from "bun:test";
import type { AppSettings, CommandModelInUse } from "@/bindings";
import {
  keyedFirst,
  modelBelongsElsewhere,
  providerWithSavedKey,
  modelInUseLine,
  routerProviders,
  suggestedRouterModel,
} from "./commandModel";

const provider = (id: string, base_url = `https://${id}.example/v1`) => ({
  id,
  label: id,
  base_url,
});

const settings = (extra: Partial<AppSettings> = {}) =>
  ({
    post_process_providers: [
      provider("builtin", "http://127.0.0.1:11435/v1"),
      provider("apple_intelligence", "apple-intelligence://local"),
      provider("anthropic"),
      provider("openai"),
    ],
    assistant_models: {},
    post_process_models: {},
    ...extra,
  }) as AppSettings;

const inUse = (extra: Partial<CommandModelInUse> = {}): CommandModelInUse => ({
  provider_id: "anthropic",
  provider_label: "Anthropic",
  model: "claude-haiku-5-5",
  borrowed: false,
  ready: true,
  ...extra,
});

describe("routerProviders", () => {
  test("lists HTTP providers, never the built-in engine or Apple", () => {
    expect(routerProviders(settings()).map((p) => p.id)).toEqual([
      "anthropic",
      "openai",
    ]);
    expect(routerProviders(null)).toEqual([]);
  });
});

describe("suggestedRouterModel", () => {
  test("prefers the assistant's model on that provider, then cleanup's", () => {
    const s = settings({
      assistant_models: { openai: " gpt-5-mini " },
      post_process_models: { openai: "gpt-5-nano", anthropic: "claude-x" },
    });
    expect(suggestedRouterModel(s, "openai")).toBe("gpt-5-mini");
    expect(suggestedRouterModel(s, "anthropic")).toBe("claude-x");
    expect(suggestedRouterModel(s, "groq")).toBeNull();
  });

  test("falls back to a known fast model for the provider", () => {
    expect(suggestedRouterModel(settings(), "openrouter")).toBe(
      "anthropic/claude-haiku-4.5",
    );
    expect(suggestedRouterModel(settings(), "anthropic")).toBe(
      "claude-haiku-5-5",
    );
  });
});

describe("modelBelongsElsewhere", () => {
  test("only the Anthropic default under another provider", () => {
    expect(modelBelongsElsewhere("openrouter", "claude-haiku-5-5")).toBe(true);
    expect(modelBelongsElsewhere("anthropic", "claude-haiku-5-5")).toBe(false);
    expect(
      modelBelongsElsewhere("openrouter", "anthropic/claude-haiku-4.5"),
    ).toBe(false);
  });
});

describe("modelInUseLine", () => {
  test("says which model runs, and why when it is the assistant's", () => {
    expect(modelInUseLine(inUse(), "Anthropic")?.key).toBe(
      "commandsPage.model.using",
    );
    const borrowed = modelInUseLine(
      inUse({
        borrowed: true,
        provider_label: "Azure OpenAI",
        model: "gpt-5.6-luna",
      }),
      "Anthropic",
    );
    expect(borrowed).toEqual({
      key: "commandsPage.model.borrowed",
      values: {
        provider: "Azure OpenAI",
        model: "gpt-5.6-luna",
        chosen: "Anthropic",
      },
    });
    expect(modelInUseLine(inUse({ ready: false }), "Anthropic")?.key).toBe(
      "commandsPage.model.needsKey",
    );
    expect(modelInUseLine(null, "Anthropic")).toBeNull();
  });
});

describe("providerWithSavedKey", () => {
  const requiresKey = (id: string) => id !== "custom";
  const keyed = (extra: Partial<AppSettings>) =>
    settings({
      post_process_providers: [
        provider("anthropic"),
        provider("openai"),
        provider("azure_openai"),
        provider("custom"),
      ],
      command_router_provider_id: "anthropic",
      ...extra,
    });

  test("moves a keyless choice to the assistant's provider", () => {
    const s = keyed({
      post_process_api_keys: { azure_openai: "k", openai: "k" },
      assistant_provider_id: "azure_openai",
      assistant_models: { azure_openai: "gpt-5.6-luna" },
      post_process_provider_id: "openai",
      post_process_models: { openai: "gpt-5-mini" },
    });
    expect(providerWithSavedKey(s, requiresKey)).toBe("azure_openai");
  });

  test("falls back to cleanup's provider, then any with a key", () => {
    const cleanup = keyed({
      post_process_api_keys: { openai: "k" },
      assistant_provider_id: "azure_openai",
      post_process_provider_id: "openai",
    });
    expect(providerWithSavedKey(cleanup, requiresKey)).toBe("openai");
    const any = keyed({
      post_process_api_keys: { custom: "k" },
      post_process_models: { custom: "openai/gpt-oss-20b" },
    });
    expect(providerWithSavedKey(any, requiresKey)).toBe("custom");
  });

  test("leaves a provider that has its key, or needs none", () => {
    const own = keyed({
      post_process_api_keys: { anthropic: "k", openai: "k" },
    });
    expect(providerWithSavedKey(own, requiresKey)).toBeNull();
    const none = keyed({
      command_router_provider_id: "custom",
      post_process_api_keys: { openai: "k" },
    });
    expect(providerWithSavedKey(none, requiresKey)).toBeNull();
  });

  test("never moves to a provider with no model to run", () => {
    const s = keyed({ post_process_api_keys: { azure_openai: "k" } });
    expect(providerWithSavedKey(s, requiresKey)).toBeNull();
  });
});

describe("keyedFirst", () => {
  test("puts providers with a saved key first, keeping order", () => {
    const s = settings({ post_process_api_keys: { openai: "k", c: "k" } });
    const order = keyedFirst(s, [
      { id: "a" },
      { id: "openai" },
      { id: "b" },
      { id: "c" },
    ]).map((p) => p.id);
    expect(order).toEqual(["openai", "c", "a", "b"]);
  });
});
