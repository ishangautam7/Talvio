import { mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { startServer } from "./app";

const env = process.env;

const port = Number(env.PORT ?? 8787);
if (!Number.isInteger(port) || port < 0 || port > 65535) {
  throw new Error(`PORT must be a port number, not "${env.PORT}".`);
}

const publicUrl = (env.PUBLIC_URL ?? `http://localhost:${port}`).replace(
  /\/+$/,
  "",
);
new URL(publicUrl); // throws on a malformed PUBLIC_URL before anything starts

const databasePath = env.DATABASE_PATH ?? "./data/zynveliq.db";
mkdirSync(dirname(databasePath), { recursive: true });

const google =
  env.GOOGLE_CLIENT_ID && env.GOOGLE_CLIENT_SECRET
    ? { clientId: env.GOOGLE_CLIENT_ID, clientSecret: env.GOOGLE_CLIENT_SECRET }
    : null;

const app = startServer({
  port,
  hostname: env.HOST ?? "0.0.0.0",
  databasePath,
  publicUrl,
  google,
  trustProxy: env.TRUST_PROXY === "1",
});
console.log(
  `Zynveliq account server listening on ${app.url} (public URL ${publicUrl}, Google sign-in ${google ? "on" : "off"})`,
);

for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.on(signal, () => {
    void app.stop().then(() => process.exit(0));
  });
}
