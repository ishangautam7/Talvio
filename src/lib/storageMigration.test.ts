import { describe, expect, test } from "bun:test";
import {
  MIGRATED_MARKER,
  migrateLegacyStorage,
  renamedKey,
} from "./storageMigration";

/** A Storage backed by a Map, optionally failing on chosen operations. */
const fakeStorage = (
  initial: Record<string, string> = {},
  fail: {
    get?: boolean;
    set?: (key: string) => boolean;
    length?: boolean;
  } = {},
): Storage & { values: Map<string, string> } => {
  const values = new Map(Object.entries(initial));
  return {
    values,
    get length() {
      if (fail.length) throw new Error("denied");
      return values.size;
    },
    key: (i: number) => [...values.keys()][i] ?? null,
    getItem: (key: string) => {
      if (fail.get) throw new Error("denied");
      return values.get(key) ?? null;
    },
    setItem: (key: string, value: string) => {
      if (fail.set?.(key)) throw new Error("quota");
      values.set(key, value);
    },
    removeItem: (key: string) => void values.delete(key),
    clear: () => values.clear(),
  };
};

describe("renamedKey", () => {
  test("maps the old prefix and the theme key", () => {
    expect(renamedKey("zynveliq.onboarding.completed")).toBe(
      "talvio.onboarding.completed",
    );
    expect(renamedKey("zynveliq.kokoro.ready.fp32")).toBe(
      "talvio.kokoro.ready.fp32",
    );
    expect(renamedKey("zynveliq-theme")).toBe("talvio-theme");
  });

  test("leaves other keys alone", () => {
    expect(renamedKey("talvio.onboarding.completed")).toBeNull();
    expect(renamedKey("zynveliq")).toBeNull();
    expect(renamedKey("i18nextLng")).toBeNull();
  });
});

describe("migrateLegacyStorage", () => {
  test("copies every old key to its new name and keeps the old ones", () => {
    const storage = fakeStorage({
      "zynveliq.onboarding.completed": "3",
      "zynveliq.notice.dismissed.x": "1",
      "zynveliq-theme": "dark",
      i18nextLng: "en",
    });
    expect(migrateLegacyStorage(storage)).toBe(3);
    expect(storage.values.get("talvio.onboarding.completed")).toBe("3");
    expect(storage.values.get("talvio.notice.dismissed.x")).toBe("1");
    expect(storage.values.get("talvio-theme")).toBe("dark");
    expect(storage.values.get("zynveliq.onboarding.completed")).toBe("3");
    expect(storage.values.has("talvio.i18nextLng")).toBe(false);
    expect(storage.values.get(MIGRATED_MARKER)).toBe("1");
  });

  test("never overwrites a value already under the new name", () => {
    const storage = fakeStorage({
      "zynveliq-theme": "dark",
      "talvio-theme": "light",
    });
    expect(migrateLegacyStorage(storage)).toBe(0);
    expect(storage.values.get("talvio-theme")).toBe("light");
  });

  test("runs once per origin", () => {
    const storage = fakeStorage({ "zynveliq.feedback.email": "a@b.c" });
    migrateLegacyStorage(storage);
    storage.values.delete("talvio.feedback.email");
    expect(migrateLegacyStorage(storage)).toBe(0);
    expect(storage.values.has("talvio.feedback.email")).toBe(false);
  });

  test("survives storage that throws", () => {
    expect(migrateLegacyStorage(undefined)).toBe(0);
    expect(
      migrateLegacyStorage(
        fakeStorage({ "zynveliq-theme": "x" }, { get: true }),
      ),
    ).toBe(0);
    expect(
      migrateLegacyStorage(
        fakeStorage({ "zynveliq-theme": "x" }, { length: true }),
      ),
    ).toBe(0);
  });

  test("a key that fails to copy does not stop the others", () => {
    const storage = fakeStorage(
      { "zynveliq.a": "1", "zynveliq.b": "2" },
      { set: (key) => key === "talvio.a" },
    );
    expect(migrateLegacyStorage(storage)).toBe(1);
    expect(storage.values.get("talvio.b")).toBe("2");
  });
});
