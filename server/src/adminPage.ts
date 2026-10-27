import { createHash } from "node:crypto";

/**
 * The owner's dashboard page, served at `/admin` (README.md, "Admin
 * dashboard"). It is static: everything it shows comes from the
 * `/v1/admin/...` JSON routes, called with the admin sign-in an `ADMIN_EMAILS`
 * account gets from its password or Google, kept only in this tab's
 * sessionStorage. Without that sign-in the page holds no data at all. Every value is put on the page as text (`textContent`), never as
 * markup, since emails, names and feedback are typed by other people.
 */

const STYLE = `
:root {
  color-scheme: light;
  --canvas: #f5f5f3; --card: #ffffff; --ink: #18181b; --muted: #6b6b70;
  --faint: #9a9aa0; --hairline: #e6e6e3; --fill: #f0f0ee;
  --accent: #c1381f; --accent-fill: #c1381f; --on-accent: #ffffff;
  --shadow: 0 1px 2px rgba(0,0,0,.04);
}
@media (prefers-color-scheme: dark) {
  :root {
    color-scheme: dark;
    --canvas: #0f0f11; --card: #18181b; --ink: #f2f2f0; --muted: #a1a1a8;
    --faint: #75757c; --hairline: #2a2a2f; --fill: #222226;
    --accent: #ff6a52; --accent-fill: #ff6a52; --on-accent: #18181b;
    --shadow: none;
  }
}
* { box-sizing: border-box; }
html, body { margin: 0; }
body {
  background: var(--canvas); color: var(--ink);
  font: 14px/1.45 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
  -webkit-font-smoothing: antialiased;
}
button, input, select { font: inherit; color: inherit; }
.wrap { max-width: 1120px; margin: 0 auto; padding: 20px 16px 48px; }
header { display: flex; align-items: center; gap: 8px 12px; margin-bottom: 20px; flex-wrap: wrap; }
header .stamp { order: 3; flex-basis: 100%; }
@media (min-width: 721px) { header .stamp { order: 0; flex-basis: auto; } }
header h1 { font-size: 20px; font-weight: 600; margin: 0; flex: 1; letter-spacing: -.01em; }
header h1 span { color: var(--muted); font-weight: 500; }
.stamp { color: var(--faint); font-size: 12px; }
.card {
  background: var(--card); border: 1px solid var(--hairline); border-radius: 12px;
  box-shadow: var(--shadow); padding: 16px; min-width: 0;
}
h2 { font-size: 14px; font-weight: 600; margin: 0 0 12px; }
.sub { color: var(--muted); font-size: 12px; }
.spacer { height: 14px; }
h2.gap { margin-top: 18px; }
.grid-gap { margin-bottom: 12px; }
.grid { display: grid; gap: 12px; margin-bottom: 12px; }
.grid > *, .row > * { min-width: 0; }
.kpis { grid-template-columns: repeat(auto-fit, minmax(min(150px, 100%), 1fr)); }
.two { grid-template-columns: repeat(auto-fit, minmax(min(320px, 100%), 1fr)); }
.kpi .label { color: var(--muted); font-size: 12px; }
.kpi .value { font-size: 24px; font-weight: 600; letter-spacing: -.02em; margin: 2px 0;
  font-variant-numeric: tabular-nums; }
.kpi .detail { color: var(--muted); font-size: 12px; }
.btn {
  border: 1px solid var(--hairline); background: var(--card); border-radius: 8px;
  padding: 6px 12px; cursor: pointer;
}
.btn:hover { background: var(--fill); }
.btn.primary { background: var(--accent); border-color: var(--accent); color: var(--on-accent); }
.btn.primary:hover { filter: brightness(1.05); }
.btn:focus-visible, input:focus-visible, select:focus-visible, .row:focus-visible, .bar:focus-visible {
  outline: 2px solid var(--accent-fill); outline-offset: 2px;
}
.control {
  border: 1px solid var(--hairline); background: var(--card); border-radius: 8px;
  padding: 7px 10px; min-width: 0;
}
.login { max-width: 400px; margin: 12vh auto 0; }
.login form { display: flex; flex-direction: column; gap: 10px; }
.error { color: var(--accent); font-size: 13px; min-height: 1em; margin: 10px 0 0; }
.login h2 { margin-bottom: 4px; }
.login .sub { margin: 0 0 4px; }
.login > .sub:first-of-type { margin-bottom: 14px; }
.or { text-align: center; margin: 10px 0 !important; }
.btn.wide { width: 100%; padding: 8px 12px; }
.btn:disabled { opacity: .6; cursor: default; }
.hidden { display: none !important; }
table { width: 100%; border-collapse: collapse; font-variant-numeric: tabular-nums; }
th { text-align: left; font-weight: 500; color: var(--muted); font-size: 12px; padding: 0 8px 6px 0; }
td { padding: 7px 8px 7px 0; border-top: 1px solid var(--hairline); vertical-align: top; }
td.num, th.num { text-align: right; padding-right: 0; padding-left: 8px; }
.scroll { overflow-x: auto; }
/* Chart */
.chart { position: relative; height: 170px; display: flex; flex-direction: column; }
.plot { position: relative; flex: 1; border-bottom: 1px solid var(--hairline); }
.gridline { position: absolute; left: 0; right: 0; border-top: 1px dashed var(--hairline); }
.gridlabel { position: absolute; right: 0; transform: translateY(-110%); font-size: 11px; color: var(--faint); }
.bars { position: absolute; inset: 0; display: flex; align-items: flex-end; gap: 2px; }
.bar { flex: 1; height: 100%; display: flex; align-items: flex-end; cursor: default; outline: none; border-radius: 4px 4px 0 0; }
.bar i { display: block; width: 100%; background: var(--accent-fill); border-radius: 4px 4px 0 0; min-height: 0; }
.bar.zero i { background: transparent; }
.bar:hover i, .bar:focus i { filter: brightness(.85); }
.xaxis { display: flex; justify-content: space-between; font-size: 11px; color: var(--faint); padding-top: 4px; }
.tip {
  position: absolute; top: 0; pointer-events: none; background: var(--ink); color: var(--card);
  border-radius: 8px; padding: 6px 8px; font-size: 12px; white-space: nowrap; transform: translateX(-50%);
  z-index: 2;
}
.tip b { display: block; font-weight: 600; }
/* Users */
.toolbar { display: flex; gap: 8px; flex-wrap: wrap; margin-bottom: 8px; }
.toolbar input { flex: 1 1 220px; }
.rows { border-top: 1px solid var(--hairline); }
.row {
  display: grid; grid-template-columns: minmax(0, 2.2fr) 1fr 1fr 1fr 1.3fr; gap: 12px;
  padding: 10px 6px; border-bottom: 1px solid var(--hairline); cursor: pointer; align-items: center;
  border-radius: 6px;
}
.row:hover { background: var(--fill); }
.row.head { cursor: default; color: var(--muted); font-size: 12px; padding-top: 0; padding-bottom: 6px; }
.row.head:hover { background: none; }
.email { font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.name { color: var(--muted); font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.meter { height: 4px; background: var(--fill); border-radius: 2px; overflow: hidden; margin-top: 4px; }
.meter i { display: block; height: 100%; background: var(--accent-fill); border-radius: 2px; }
.tags { display: flex; gap: 4px; flex-wrap: wrap; }
.tag { font-size: 11px; border: 1px solid var(--hairline); border-radius: 6px; padding: 0 6px; color: var(--muted); }
.cell-label { display: none; color: var(--faint); font-size: 11px; margin-right: 4px; }
.more { margin-top: 10px; display: flex; justify-content: space-between; align-items: center; gap: 8px; }
@media (max-width: 720px) {
  .row { grid-template-columns: 1fr 1fr; gap: 4px 12px; }
  .row > :first-child { grid-column: 1 / -1; }
  .row.head { display: none; }
  .cell-label { display: inline; }
}
/* Detail */
dialog {
  border: 1px solid var(--hairline); border-radius: 16px; padding: 0; width: min(720px, calc(100vw - 32px));
  max-height: calc(100vh - 48px); background: var(--card); color: var(--ink);
}
dialog::backdrop { background: rgba(0,0,0,.35); }
.dlg-head { display: flex; align-items: flex-start; gap: 12px; padding: 16px; border-bottom: 1px solid var(--hairline);
  position: sticky; top: 0; background: var(--card); }
.dlg-head div { flex: 1; min-width: 0; }
.dlg-head h2 { margin: 0; overflow-wrap: anywhere; }
.dlg-body { padding: 16px; display: grid; gap: 16px; }
.facts { display: grid; grid-template-columns: repeat(auto-fit, minmax(140px, 1fr)); gap: 8px 16px; }
.facts dt { color: var(--muted); font-size: 12px; }
.facts dd { margin: 0; font-variant-numeric: tabular-nums; }
.dot { display: inline-block; width: 7px; height: 7px; border-radius: 50%; background: var(--faint); margin-right: 6px; vertical-align: 1px; }
.dot.on { background: #2f9e44; }
/* Feedback */
.fb { padding: 10px 0; border-top: 1px solid var(--hairline); }
.fb p { margin: 4px 0; white-space: pre-wrap; overflow-wrap: anywhere; }
.fb .meta { color: var(--muted); font-size: 12px; }
.fb img { max-width: 100%; border: 1px solid var(--hairline); border-radius: 8px; margin-top: 6px; display: block; }
.fb .btn { margin-top: 4px; margin-right: 4px; font-size: 12px; padding: 2px 8px; }
code { font: 12px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; background: var(--fill); padding: 1px 5px; border-radius: 4px; overflow-wrap: anywhere; }
`;

const SCRIPT = `
"use strict";
const KEY = "talvio.admin.session";
const VERIFIER = "talvio.admin.verifier";
const SERVICE = {
  chat: "Chat (assistant, cleanup, commands)",
  transcribe: "Speech to text",
  speech: "Text to speech",
};
const SHORT = { chat: "Chat", transcribe: "STT", speech: "TTS" };
const $ = (id) => document.getElementById(id);

function el(tag, props, ...children) {
  const node = document.createElement(tag);
  if (props) for (const [k, v] of Object.entries(props)) {
    if (v === undefined || v === null) continue;
    if (k === "class") node.className = v;
    else if (k === "text") node.textContent = v;
    else if (k.startsWith("on")) node.addEventListener(k.slice(2), v);
    else node.setAttribute(k, String(v));
  }
  for (const child of children.flat()) {
    if (child === null || child === undefined || child === false) continue;
    node.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return node;
}

let token = null;
try { token = sessionStorage.getItem(KEY); } catch {}

function saveToken(value) {
  token = value;
  try { value ? sessionStorage.setItem(KEY, value) : sessionStorage.removeItem(KEY); } catch {}
}

class AuthError extends Error {}

async function api(path) {
  const res = await fetch(path, { headers: { authorization: "Bearer " + token }, cache: "no-store" });
  if (res.status === 401) throw new AuthError("Your sign-in has ended. Sign in again.");
  if (res.status === 404 && path.startsWith("/v1/admin/overview")) throw new AuthError("The dashboard is off on this server.");
  if (res.status === 429) throw new AuthError(tooMany(res));
  if (!res.ok) {
    let message = "The server answered " + res.status + ".";
    try { message = (await res.json()).error.message || message; } catch {}
    throw new Error(message);
  }
  return res;
}
const getJson = async (path) => (await api(path)).json();

function tooMany(res) {
  const wait = Number(res.headers.get("retry-after")) || 60;
  return "Too many attempts. Try again in " + Math.ceil(wait / 60) + " min.";
}

// ── Signing in ──
const SIGN_IN_ERRORS = {
  invalid_credentials: "Wrong email or password.",
  invalid_email: "Enter an email address.",
  not_admin: "This account is not an admin of this server.",
  invalid_code: "That sign-in expired. Try again.",
  access_denied: "Google sign-in was cancelled.",
  account_conflict: "That Google account cannot sign in to this address.",
  google_failed: "Google sign-in did not work. Try again.",
  google_unavailable: "Google sign-in is not set up on this server.",
};

async function signIn(path, body) {
  const res = await fetch(path, {
    method: "POST", cache: "no-store",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  if (res.status === 429) throw new AuthError(tooMany(res));
  let answer = {};
  try { answer = await res.json(); } catch {}
  if (!res.ok) {
    const code = answer.error && answer.error.code;
    throw new AuthError(SIGN_IN_ERRORS[code] || (answer.error && answer.error.message) || "Could not sign in.");
  }
  saveToken(answer.token);
  await loadAll();
}

const b64url = (bytes) => btoa(String.fromCharCode(...bytes)).replace(/[+]/g, "-").replace(/[/]/g, "_").replace(/=+$/, "");

async function googleSignIn() {
  const verifier = b64url(crypto.getRandomValues(new Uint8Array(32)));
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier)));
  try { sessionStorage.setItem(VERIFIER, verifier); } catch {}
  location.assign("/v1/admin/google/start?" + new URLSearchParams({
    code_challenge: b64url(digest), code_challenge_method: "S256",
  }));
}

async function offerGoogle() {
  if (!(window.crypto && crypto.subtle)) return; // PKCE needs a secure page (https or localhost)
  try {
    const config = await (await fetch("/v1/config", { cache: "no-store" })).json();
    $("google").classList.toggle("hidden", !config.google);
    $("or").classList.toggle("hidden", !config.google);
  } catch {}
}

/** Back from Google: ?code= to redeem, or ?error=. */
async function returnFromGoogle() {
  const params = new URLSearchParams(location.search);
  const code = params.get("code"), error = params.get("error");
  if (!code && !error) return false;
  history.replaceState(null, "", location.pathname);
  let verifier = null;
  try { verifier = sessionStorage.getItem(VERIFIER); sessionStorage.removeItem(VERIFIER); } catch {}
  if (error || !verifier) {
    showLogin(SIGN_IN_ERRORS[error] || SIGN_IN_ERRORS.google_failed);
    return true;
  }
  await signIn("/v1/admin/google/exchange", { code, codeVerifier: verifier }).catch(fail);
  return true;
}

const nf = new Intl.NumberFormat();
const fmtN = (n) => nf.format(n);
function usd(v) {
  if (!v) return "$0";
  const digits = v >= 1 ? 2 : v >= 0.01 ? 3 : 4;
  return "$" + v.toFixed(digits);
}
const dateFmt = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });
const dayFmt = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", timeZone: "UTC" });
const full = (ms) => (ms ? dateFmt.format(new Date(ms)) : "never");
function ago(ms) {
  if (!ms) return "never";
  const s = Math.max(0, (Date.now() - ms) / 1000);
  if (s < 60) return "just now";
  if (s < 3600) return Math.floor(s / 60) + " min ago";
  if (s < 86400) return Math.floor(s / 3600) + " h ago";
  if (s < 86400 * 30) return Math.floor(s / 86400) + " d ago";
  return new Date(ms).toLocaleDateString();
}
const when = (ms) => el("span", { title: full(ms), text: ago(ms) });

function showLogin(message) {
  $("app").classList.add("hidden");
  $("signout").classList.add("hidden");
  $("login").classList.remove("hidden");
  $("login-error").textContent = message || "";
  $("password").value = "";
  ($("email").value ? $("password") : $("email")).focus();
}

function fail(error) {
  if (error instanceof AuthError) {
    saveToken(null);
    showLogin(error.message);
  } else {
    $("app-error").textContent = error.message || String(error);
  }
}

// ── Overview ──
function kpi(label, value, detail) {
  return el("div", { class: "card kpi" },
    el("div", { class: "label", text: label }),
    el("div", { class: "value", text: value }),
    el("div", { class: "detail", text: detail }));
}

function chart(days) {
  const max = Math.max(1, ...days.map((d) => d.requests));
  const step = niceStep(max);
  const top = Math.ceil(max / step) * step;
  const plot = el("div", { class: "plot" });
  for (let v = step; v <= top; v += step) {
    const line = el("div", { class: "gridline" }, el("span", { class: "gridlabel", text: fmtN(v) }));
    line.style.bottom = (v / top) * 100 + "%";
    plot.append(line);
  }
  const tip = el("div", { class: "tip hidden", role: "status" });
  const bars = el("div", { class: "bars" });
  days.forEach((d, i) => {
    const fill = el("i");
    fill.style.height = (d.requests / top) * 100 + "%";
    const label = dayFmt.format(new Date(d.day + "T00:00:00Z"));
    const bar = el("div", {
      class: "bar" + (d.requests ? "" : " zero"), tabindex: 0,
      "aria-label": label + ": " + d.requests + " requests, " + usd(d.costUsd) + ", " + d.signups + " sign-ups",
    }, fill);
    const show = () => {
      tip.replaceChildren(el("b", { text: label }),
        fmtN(d.requests) + " requests · " + usd(d.costUsd),
        el("br"), d.signups + " sign-up" + (d.signups === 1 ? "" : "s"));
      tip.classList.remove("hidden");
      const x = ((i + 0.5) / days.length) * 100;
      tip.style.left = "clamp(70px, " + x + "%, calc(100% - 70px))";
    };
    bar.addEventListener("mouseenter", show);
    bar.addEventListener("focus", show);
    bar.addEventListener("mouseleave", () => tip.classList.add("hidden"));
    bar.addEventListener("blur", () => tip.classList.add("hidden"));
    bars.append(bar);
  });
  plot.append(bars);
  const first = days[0], last = days[days.length - 1], mid = days[Math.floor(days.length / 2)];
  const axis = el("div", { class: "xaxis" },
    ...[first, mid, last].map((d) => el("span", { text: dayFmt.format(new Date(d.day + "T00:00:00Z")) })));
  return el("div", { class: "chart" }, tip, plot, axis);
}

function niceStep(max) {
  const raw = max / 3;
  const pow = Math.pow(10, Math.floor(Math.log10(raw)));
  for (const m of [1, 2, 5, 10]) if (m * pow >= raw) return Math.max(1, m * pow);
  return Math.max(1, 10 * pow);
}

function renderOverview(o) {
  $("stamp").textContent = "Updated " + dateFmt.format(new Date(o.generatedAt));
  const c = o.credit;
  $("kpis").replaceChildren(
    kpi("Users", fmtN(o.users.total), "+" + fmtN(o.users.new24h) + " in 24 h · +" + fmtN(o.users.new7d) + " in 7 days"),
    kpi("Active users", fmtN(o.users.active7d), fmtN(o.users.active24h) + " in 24 h · seen in 7 days"),
    kpi("Signed-in devices", fmtN(o.devices.total), fmtN(o.devices.desktop) + " desktop · " + fmtN(o.devices.mobile) + " mobile · " + fmtN(o.devices.online) + " online"),
    kpi("Free credit spent", usd(c.spentUsd), fmtN(c.requests) + " requests · " + fmtN(c.accountsWithUsage) + " accounts · " + fmtN(c.accountsExhausted) + " used up"),
    kpi("Feedback", fmtN(o.feedback.total), fmtN(o.feedback.last7d) + " in 7 days"),
  );

  const total30 = o.days.reduce((s, d) => s + d.requests, 0);
  const cost30 = o.days.reduce((s, d) => s + d.costUsd, 0);
  $("daily").replaceChildren(
    el("h2", { text: "Free credit requests per day" }),
    el("div", { class: "sub", text: "Last 30 days (UTC) · " + fmtN(total30) + " requests · " + usd(cost30) }),
    el("div", { class: "spacer" }), chart(o.days));

  const services = c.byService.length
    ? el("div", { class: "scroll" }, el("table", null,
        el("thead", null, el("tr", null, el("th", { text: "Service" }), el("th", { class: "num", text: "Requests" }),
          el("th", { class: "num", text: "Spent" }), el("th", { class: "num", text: "Share" }))),
        el("tbody", null, c.byService.map((s) => el("tr", null,
          el("td", { text: SERVICE[s.service] || s.service }),
          el("td", { class: "num", text: fmtN(s.requests) }),
          el("td", { class: "num", text: usd(s.costUsd) }),
          el("td", { class: "num", text: c.spentUsd ? Math.round((s.costUsd / c.spentUsd) * 100) + "%" : "–" }))))))
    : el("p", { class: "sub", text: "No free credit used yet." });
  const models = c.byModel.length
    ? el("div", { class: "scroll" }, el("table", null,
        el("thead", null, el("tr", null, el("th", { text: "Model" }), el("th", { text: "Service" }),
          el("th", { class: "num", text: "Requests" }), el("th", { class: "num", text: "Spent" }))),
        el("tbody", null, c.byModel.map((m) => el("tr", null,
          el("td", null, el("code", { text: m.model })),
          el("td", { text: SHORT[m.service] || m.service }),
          el("td", { class: "num", text: fmtN(m.requests) }),
          el("td", { class: "num", text: usd(m.costUsd) }))))))
    : null;
  $("services").replaceChildren(
    el("h2", { text: "Credit by service" }),
    el("div", { class: "sub", text: (c.enabled ? "Free credit is on" : "Free credit is off on this server") + " · " + usd(c.limitUsd) + " per account" }),
    el("div", { class: "spacer" }), services,
    models && el("h2", { class: "gap", text: "By model" }), models);

  $("feedback-count").textContent = fmtN(o.feedback.total) + " entries · " + fmtN(o.feedback.last7d) + " in the last 7 days";
  $("feedback-url").textContent = o.feedback.url;
}

// ── Users ──
const users = { q: "", sort: "new", offset: 0, list: [], total: 0, seq: 0 };

async function loadUsers(append) {
  const seq = ++users.seq;
  const offset = append ? users.list.length : 0;
  const params = new URLSearchParams({ limit: "50", offset: String(offset), sort: users.sort });
  if (users.q) params.set("q", users.q);
  const page = await getJson("/v1/admin/users?" + params);
  if (seq !== users.seq) return;
  users.list = append ? users.list.concat(page.users) : page.users;
  users.total = page.total;
  renderUsers(page.next !== null);
}

function cell(label, ...content) {
  return el("div", null, el("span", { class: "cell-label", text: label }), ...content);
}

function renderUsers(hasMore) {
  const head = el("div", { class: "row head" },
    el("div", { text: "Account" }), el("div", { text: "Joined" }), el("div", { text: "Last seen" }),
    el("div", { text: "Devices" }), el("div", { text: "Free credit" }));
  const rows = users.list.map((u) => {
    const pct = u.credit.limitUsd ? Math.min(100, (u.credit.usedUsd / u.credit.limitUsd) * 100) : 0;
    const meter = el("div", { class: "meter" }, el("i"));
    meter.firstChild.style.width = pct + "%";
    const row = el("div", { class: "row", tabindex: 0, role: "button", "aria-label": "Open " + u.email },
      el("div", null, el("div", { class: "email", text: u.email }),
        el("div", { class: "name", text: u.name || "" })),
      cell("Joined", when(u.createdAt)),
      cell("Seen", when(u.lastSeenAt)),
      cell("Devices", u.devices.total
        ? u.devices.desktop + " desktop · " + u.devices.mobile + " mobile"
        : "none"),
      el("div", null,
        el("div", null, usd(u.credit.usedUsd) + " of " + usd(u.credit.limitUsd)),
        meter,
        u.services.length ? el("div", { class: "tags" }, u.services.map((s) => el("span", { class: "tag", text: SHORT[s] || s }))) : null));
    const open = () => openUser(u.id).catch(fail);
    row.addEventListener("click", open);
    row.addEventListener("keydown", (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); open(); } });
    return row;
  });
  $("user-rows").replaceChildren(head, ...(rows.length ? rows : [el("p", { class: "sub", text: users.q ? "No account matches." : "No accounts yet." })]));
  $("user-count").textContent = "Showing " + fmtN(users.list.length) + " of " + fmtN(users.total);
  $("user-more").classList.toggle("hidden", !hasMore);
}

async function openUser(id) {
  const d = await getJson("/v1/admin/users/" + encodeURIComponent(id));
  const u = d.user;
  const sign = [u.hasPassword && "password", u.googleLinked && "Google"].filter(Boolean).join(" + ") || "–";
  const facts = el("dl", { class: "facts" },
    ...[
      ["Joined", full(u.createdAt)],
      ["Last seen", full(u.lastSeenAt)],
      ["Sign-in", sign + (u.emailVerified ? " · verified" : "")],
      ["Credit used", usd(d.credit.usedUsd) + " of " + usd(d.credit.limitUsd)],
      ["Credit left", usd(d.credit.remainingUsd)],
      ["Requests", fmtN(d.credit.requests)],
      ["Feedback sent", fmtN(d.feedback)],
    ].map(([k, v]) => el("div", null, el("dt", { text: k }), el("dd", { text: v }))));

  const devices = d.devices.length
    ? el("div", { class: "scroll" }, el("table", null,
        el("thead", null, el("tr", null, el("th", { text: "Device" }), el("th", { text: "Kind" }),
          el("th", { text: "App" }), el("th", { text: "Last seen" }))),
        el("tbody", null, d.devices.map((dev) => el("tr", null,
          el("td", null, el("span", { class: "dot" + (dev.online ? " on" : ""), title: dev.online ? "Online" : "Offline" }), dev.name),
          el("td", { text: dev.kind + " · " + dev.platform }),
          el("td", { text: dev.appVersion || "–" }),
          el("td", null, dev.online ? "online now" : when(dev.lastSeenAt)))))))
    : el("p", { class: "sub", text: "No signed-in devices." });

  const services = d.services.length
    ? el("table", null,
        el("thead", null, el("tr", null, el("th", { text: "Service" }), el("th", { class: "num", text: "Requests" }),
          el("th", { class: "num", text: "Spent" }), el("th", { class: "num", text: "Last used" }))),
        el("tbody", null, d.services.map((s) => el("tr", null,
          el("td", { text: SERVICE[s.service] || s.service }),
          el("td", { class: "num", text: fmtN(s.requests) }),
          el("td", { class: "num", text: usd(s.costUsd) }),
          el("td", { class: "num" }, when(s.lastUsedAt))))))
    : el("p", { class: "sub", text: "Has not used the free credit." });

  const recent = d.recentRequests.length
    ? el("div", { class: "scroll" }, el("table", null,
        el("thead", null, el("tr", null, el("th", { text: "When" }), el("th", { text: "Service" }),
          el("th", { text: "Model" }), el("th", { class: "num", text: "Size" }), el("th", { class: "num", text: "Cost" }))),
        el("tbody", null, d.recentRequests.map((r) => el("tr", null,
          el("td", null, when(r.at)),
          el("td", { text: SHORT[r.service] || r.service }),
          el("td", null, el("code", { text: r.model })),
          el("td", { class: "num", text: fmtN(Math.round(r.units * 10) / 10) + " " + r.unit }),
          el("td", { class: "num", text: usd(r.costUsd) }))))))
    : null;

  $("detail").replaceChildren(
    el("div", { class: "dlg-head" },
      el("div", null, el("h2", { text: u.email }), el("div", { class: "sub", text: (u.name ? u.name + " · " : "") + u.id })),
      el("button", { class: "btn", type: "button", text: "Close", onclick: () => $("detail").close() })),
    el("div", { class: "dlg-body" },
      facts,
      el("section", null, el("h2", { text: "Devices" }), devices),
      el("section", null, el("h2", { text: "Services used" }), services),
      recent && el("section", null, el("h2", { text: "Recent requests" }), recent)));
  $("detail").showModal();
}

// ── Feedback ──
let feedbackNext = null;
async function loadFeedback(append) {
  const params = new URLSearchParams({ limit: "10" });
  if (append && feedbackNext !== null) params.set("before", String(feedbackNext));
  const page = await getJson("/v1/admin/feedback?" + params);
  feedbackNext = page.next;
  const items = page.feedback.map((f) => {
    const shots = f.screenshots.map((s, n) => el("button", {
      class: "btn", type: "button", text: "Screenshot " + (n + 1),
      onclick: async (e) => {
        const button = e.currentTarget;
        button.disabled = true;
        try {
          const blob = await (await api(new URL(s.url).pathname)).blob();
          const img = el("img", { alt: "Screenshot " + (n + 1) + " from this feedback" });
          img.src = URL.createObjectURL(blob);
          button.replaceWith(img);
        } catch (error) { button.disabled = false; fail(error); }
      },
    }));
    const who = [f.kind, f.user && f.user.email, f.email && "reply to " + f.email, f.device && f.device.name]
      .filter(Boolean).join(" · ");
    return el("div", { class: "fb" },
      el("div", { class: "meta" }, when(f.createdAt), who ? " · " + who : ""),
      el("p", { text: f.message }),
      f.system ? el("div", { class: "meta", text: f.system }) : null,
      shots.length ? el("div", null, shots) : null);
  });
  const list = $("feedback-list");
  if (!append) list.replaceChildren();
  list.append(...items);
  if (!append && !items.length) list.append(el("p", { class: "sub", text: "No feedback yet." }));
  $("feedback-more").classList.toggle("hidden", feedbackNext === null);
}

// ── Wiring ──
async function loadAll() {
  $("app-error").textContent = "";
  const o = await getJson("/v1/admin/overview");
  $("login").classList.add("hidden");
  $("app").classList.remove("hidden");
  $("signout").classList.remove("hidden");
  renderOverview(o);
  await Promise.all([loadUsers(false), loadFeedback(false)]);
}

$("login-form").addEventListener("submit", (e) => {
  e.preventDefault();
  const button = $("login-submit");
  button.disabled = true;
  signIn("/v1/admin/login", { email: $("email").value.trim(), password: $("password").value })
    .catch(fail).finally(() => { button.disabled = false; });
});
$("google").addEventListener("click", () => googleSignIn().catch(fail));
$("signout").addEventListener("click", async () => {
  const ending = token;
  saveToken(null);
  try { await fetch("/v1/admin/logout", { method: "POST", headers: { authorization: "Bearer " + ending } }); } catch {}
  location.reload();
});
$("refresh").addEventListener("click", () => loadAll().catch(fail));
let searchTimer;
$("search").addEventListener("input", (e) => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(() => { users.q = e.target.value.trim(); loadUsers(false).catch(fail); }, 250);
});
$("sort").addEventListener("change", (e) => { users.sort = e.target.value; loadUsers(false).catch(fail); });
$("user-more").addEventListener("click", () => loadUsers(true).catch(fail));
$("feedback-more").addEventListener("click", () => loadFeedback(true).catch(fail));
$("detail").addEventListener("click", (e) => { if (e.target === $("detail")) $("detail").close(); });

offerGoogle();
returnFromGoogle().then((handled) => {
  if (handled) return;
  if (token) loadAll().catch(fail); else showLogin();
});
`;

const BODY = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex, nofollow">
<meta name="referrer" content="no-referrer">
<title>Talvio Admin</title>
<style>${STYLE}</style>
</head>
<body>
<div class="wrap">
  <header>
    <h1>Talvio <span>admin</span></h1>
    <span class="stamp" id="stamp"></span>
    <button class="btn hidden" id="signout" type="button">Sign out</button>
  </header>

  <section class="card login hidden" id="login">
    <h2>Sign in</h2>
    <p class="sub">With your Talvio account. Only accounts this server lists as admins can open the dashboard.</p>
    <form id="login-form">
      <label class="sub" for="email">Email</label>
      <input class="control" id="email" type="email" autocomplete="username" spellcheck="false" required>
      <label class="sub" for="password">Password</label>
      <input class="control" id="password" type="password" autocomplete="current-password" required>
      <button class="btn primary" id="login-submit" type="submit">Sign in</button>
    </form>
    <p class="sub or hidden" id="or">or</p>
    <button class="btn wide hidden" id="google" type="button">Continue with Google</button>
    <p class="error" id="login-error" role="alert"></p>
  </section>

  <main class="hidden" id="app">
    <p class="error" id="app-error" role="alert"></p>
    <div class="grid kpis" id="kpis"></div>
    <div class="grid two">
      <section class="card" id="daily"></section>
      <section class="card" id="services"></section>
    </div>
    <section class="card grid-gap" id="users">
      <h2>Users</h2>
      <div class="toolbar">
        <input class="control" id="search" type="search" placeholder="Search by email, name or id" aria-label="Search users">
        <select class="control" id="sort" aria-label="Sort users">
          <option value="new">Newest first</option>
          <option value="seen">Last seen</option>
          <option value="email">Email</option>
        </select>
        <button class="btn" id="refresh" type="button">Refresh</button>
      </div>
      <div class="rows" id="user-rows"></div>
      <div class="more"><span class="sub" id="user-count"></span><button class="btn hidden" id="user-more" type="button">Show more</button></div>
    </section>
    <section class="card grid-gap" id="feedback">
      <h2>Feedback</h2>
      <div class="sub" id="feedback-count"></div>
      <div class="sub">API: <code id="feedback-url"></code> (with the same token)</div>
      <div id="feedback-list"></div>
      <div class="more"><span></span><button class="btn hidden" id="feedback-more" type="button">Older</button></div>
    </section>
  </main>
</div>
<dialog id="detail" aria-label="Account"></dialog>
<script>${SCRIPT}</script>
</body>
</html>
`;

const hash = (text: string) =>
  `'sha256-${createHash("sha256").update(text, "utf8").digest("base64")}'`;

/**
 * Only this page's own inline script and stylesheet may run (by hash), it may
 * only call this server, and it can only show images it fetched itself (blob:).
 */
export const ADMIN_PAGE_CSP = [
  "default-src 'none'",
  `script-src ${hash(SCRIPT)}`,
  `style-src ${hash(STYLE)}`,
  "connect-src 'self'",
  "img-src blob:",
  "base-uri 'none'",
  "form-action 'none'",
  "frame-ancestors 'none'",
].join("; ");

export function adminPage(): Response {
  return new Response(BODY, {
    headers: {
      "content-type": "text/html; charset=utf-8",
      "content-security-policy": ADMIN_PAGE_CSP,
      "cache-control": "no-store",
      "referrer-policy": "no-referrer",
      "x-content-type-options": "nosniff",
      "x-frame-options": "DENY",
    },
  });
}
