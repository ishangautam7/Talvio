/**
 * Carries a Zynveliq install's browser storage over to Talvio's names (the app
 * was renamed; docs/talvio-brand.md §3). Every `zynveliq.*` key and
 * `zynveliq-theme` is copied to its `talvio.*` / `talvio-theme` name unless the
 * new one already exists, so a value Talvio wrote is never overwritten. The
 * onboarding-completed flag is the one that matters most: without it every
 * existing user would be walked through setup again.
 *
 * It runs once per origin (all windows share one), on import, and every entry
 * point imports it before anything else so no module reads storage first. The
 * old keys are left in place: they cost a few bytes, and a downgrade still
 * finds them.
 */

const OLD_PREFIX = "zynveliq.";
const NEW_PREFIX = "talvio.";
const RENAMED: Readonly<Record<string, string>> = {
  "zynveliq-theme": "talvio-theme",
};
/** Set once the copy has run, so later launches skip the scan. */
export const MIGRATED_MARKER = "talvio.storageMigrated";

/** The Talvio name for an old key, or null for a key that is not ours. */
export function renamedKey(key: string): string | null {
  if (key in RENAMED) return RENAMED[key];
  if (key.startsWith(OLD_PREFIX)) {
    return NEW_PREFIX + key.slice(OLD_PREFIX.length);
  }
  return null;
}

/**
 * Copy the old keys forward. Returns how many were copied. Never throws:
 * storage can be missing or refuse access (private window, blocked site data),
 * and a failed copy must not stop a window from starting.
 */
export function migrateLegacyStorage(storage: Storage | undefined): number {
  if (!storage) return 0;
  try {
    if (storage.getItem(MIGRATED_MARKER) !== null) return 0;
  } catch {
    return 0;
  }

  let keys: string[] = [];
  try {
    for (let i = 0; i < storage.length; i++) {
      const key = storage.key(i);
      if (key !== null) keys.push(key);
    }
  } catch {
    keys = [];
  }

  let copied = 0;
  for (const oldKey of keys) {
    const newKey = renamedKey(oldKey);
    if (!newKey) continue;
    try {
      if (storage.getItem(newKey) !== null) continue;
      const value = storage.getItem(oldKey);
      if (value === null) continue;
      storage.setItem(newKey, value);
      copied++;
    } catch {
      // Quota or access error on one key: carry on with the rest.
    }
  }

  try {
    storage.setItem(MIGRATED_MARKER, "1");
  } catch {
    // Not marked: the next start tries again, which is harmless.
  }
  return copied;
}

function browserStorage(): Storage | undefined {
  try {
    return typeof localStorage === "undefined" ? undefined : localStorage;
  } catch {
    return undefined;
  }
}

migrateLegacyStorage(browserStorage());
