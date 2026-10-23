import { describe, expect, test } from "bun:test";
import { join } from "node:path";
import {
  DEFAULT_DATABASE_PATH,
  resolveDatabasePath,
} from "../src/databasePath";

const existing =
  (...paths: string[]) =>
  (path: string) =>
    paths.includes(path);

describe("database path", () => {
  const oldDefault = join("./data", "zynveliq.db");

  test("a fresh deployment gets talvio.db", () => {
    expect(resolveDatabasePath(undefined, existing())).toEqual({
      path: DEFAULT_DATABASE_PATH,
      legacy: false,
    });
  });

  test("a deployment from before the rename keeps its zynveliq.db", () => {
    expect(resolveDatabasePath(undefined, existing(oldDefault))).toEqual({
      path: oldDefault,
      legacy: true,
    });
    expect(
      resolveDatabasePath(
        "/data/talvio.db",
        existing(join("/data", "zynveliq.db")),
      ),
    ).toEqual({ path: join("/data", "zynveliq.db"), legacy: true });
  });

  test("an existing talvio.db wins over an old file beside it", () => {
    expect(
      resolveDatabasePath(
        undefined,
        existing(DEFAULT_DATABASE_PATH, oldDefault),
      ),
    ).toEqual({ path: DEFAULT_DATABASE_PATH, legacy: false });
  });

  test("any other configured path is used as given", () => {
    expect(
      resolveDatabasePath(
        "/srv/accounts.db",
        existing(join("/srv", "zynveliq.db")),
      ),
    ).toEqual({ path: "/srv/accounts.db", legacy: false });
  });
});
