import { mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { adminEmailsFromEnv } from "./admin";
import { startServer } from "./app";
import { resolveDatabasePath } from "./databasePath";
import { feedbackGithubFromEnv } from "./feedback";
import { describeFree, freeFromEnv } from "./free/config";
import { turnFromEnv } from "./turn";

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

const { path: databasePath, legacy } = resolveDatabasePath(env.DATABASE_PATH);
if (legacy) {
  console.log(
    `Using the existing database ${databasePath} from before the rename to Talvio`,
  );
}
mkdirSync(dirname(databasePath), { recursive: true });

const google =
  env.GOOGLE_CLIENT_ID && env.GOOGLE_CLIENT_SECRET
    ? { clientId: env.GOOGLE_CLIENT_ID, clientSecret: env.GOOGLE_CLIENT_SECRET }
    : null;

const turn = turnFromEnv(env);
const feedbackGithub = feedbackGithubFromEnv(env);
const adminToken = env.ADMIN_TOKEN?.trim() || null;
if (adminToken && adminToken.length < 24) {
  throw new Error("ADMIN_TOKEN must be at least 24 characters.");
}
const adminEmails = adminEmailsFromEnv(env.ADMIN_EMAILS);
const free = freeFromEnv(env);

const app = startServer({
  port,
  hostname: env.HOST ?? "0.0.0.0",
  databasePath,
  publicUrl,
  google,
  trustProxy: env.TRUST_PROXY === "1",
  secretsKey: env.TALVIO_SECRETS_KEY,
  turn,
  adminToken,
  adminEmails,
  feedbackGithub,
  free,
});
console.log(
  `Talvio account server listening on ${app.url} (public URL ${publicUrl}, Google sign-in ${google ? "on" : "off"}, TURN ${turn ? turn.kind : "off"}, free credit ${describeFree(free)}, admin dashboard ${adminEmails.length ? `${adminEmails.length} account(s)` : "off"}, admin token ${adminToken ? "on" : "off"}, feedback issues ${feedbackGithub ? feedbackGithub.repo : "off"})`,
);

for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.on(signal, () => {
    void app.stop().then(() => process.exit(0));
  });
}
