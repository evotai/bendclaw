/* Shared helpers for the evot local console pages: escaping, fetch wrappers,
   toasts, and the sidenav active marker. No framework and no build step — each
   page is a plain document that loads this file. */

export const $ = (id) => document.getElementById(id);

/** Escape for interpolation into HTML. Always used on server-supplied text. */
export function esc(value) {
  const div = document.createElement("div");
  div.textContent = value == null ? "" : String(value);
  return div.innerHTML;
}

let toastTimer = 0;
let loadBarCount = 0;
let loadBarEl = null;
let loadBarHide = 0;

function loadBar() {
  if (!loadBarEl || !document.body.contains(loadBarEl)) {
    const el = document.createElement("div");
    el.className = "load-bar";
    el.setAttribute("aria-hidden", "true");
    loadBarEl = el;
  }
  // Keep the bar a direct child of body: page chrome relocates existing
  // children, and a nested bar can be clipped by overflow:hidden regions.
  if (loadBarEl.parentNode !== document.body) document.body.appendChild(loadBarEl);
  return loadBarEl;
}

/** Top-of-window refresh bar, shared by every in-flight JSON request. */
export function beginLoad() {
  loadBarCount += 1;
  const el = loadBar();
  window.clearTimeout(loadBarHide);
  el.className = "load-bar on";
}

export function endLoad() {
  loadBarCount = Math.max(0, loadBarCount - 1);
  if (loadBarCount > 0) return;
  const el = loadBar();
  el.className = "load-bar done";
  loadBarHide = window.setTimeout(() => {
    if (loadBarCount === 0) el.className = "load-bar";
  }, 220);
}

/** Shimmer rows matching the admin skeleton. */
export function skeletonHtml(rows = 5, kind = "line") {
  if (kind === "form") {
    return '<div class="panel" aria-busy="true">' +
      '<div class="panel-body">' +
      Array.from({ length: rows }, () =>
        '<div class="sk-row"><span class="sk mid"></span></div>' +
        '<div class="sk-row"><span class="sk wide"></span></div>',
      ).join("") +
      "</div></div>";
  }
  if (kind === "cloud") {
    return Array.from({ length: rows }, () =>
      '<div class="sk-row"><span class="sk mid"></span><span class="sk wide"></span></div>',
    ).join("");
  }
  return Array.from({ length: rows }, () =>
    '<div class="sk-row"><span class="sk wide"></span><span class="sk narrow"></span></div>',
  ).join("");
}

/**
 * Show a transient message. `kind` may be "err" to render it as a failure.
 * Passing no message hides the toast immediately.
 */
export function toast(message, kind) {
  const el = $("toast");
  if (!el) return;
  window.clearTimeout(toastTimer);
  if (!message) {
    el.className = "toast";
    return;
  }
  el.textContent = message;
  el.className = "toast show" + (kind ? " " + kind : "");
  toastTimer = window.setTimeout(() => {
    el.className = "toast";
  }, kind === "err" ? 6000 : 2600);
}

/** GET JSON, throwing on a non-2xx so callers can use one catch path. */
export async function getJson(url) {
  beginLoad();
  try {
    const res = await fetch(url, { headers: { accept: "application/json" } });
    if (!res.ok) throw new Error("HTTP " + res.status);
    return await res.json();
  } finally {
    endLoad();
  }
}

/**
 * POST JSON. The server reports validation failures as `{ok:false,error}` with
 * a 4xx, so the message is lifted out of the body when present — a bare status
 * code would hide why a save was rejected.
 */
export async function postJson(url, body, opts) {
  beginLoad();
  try {
    const res = await fetch(url, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
      ...(opts || {}),
    });
    let data = null;
    try {
      data = await res.json();
    } catch {
      data = null;
    }
    if (!res.ok || (data && data.ok === false)) {
      throw new Error((data && data.error) || "HTTP " + res.status);
    }
    return data;
  } finally {
    endLoad();
  }
}

/** Mark the sidenav entry whose href matches the current path. */
export function markActiveNav() {
  const here = window.location.pathname.replace(/\/+$/, "") || "/";
  document.querySelectorAll(".sidenav a").forEach((a) => {
    const href = a.getAttribute("href") || "";
    const path = href.replace(/\/+$/, "") || "/";
    if (path === here) a.classList.add("active");
  });
}

/* Nav lives here rather than in each document so adding a page is a one-line
   change instead of copies drifting apart. Every entry must resolve to a real
   route: a dead nav item is worse than a missing one.

   The console's home is Chat; the sessions list folded into it and "/"
   redirects there, so no nav row points at either. */
const NAV = [
  { href: "/chat", name: "Chat" },
  { href: "/models", name: "Models" },
  { href: "/feishu", name: "Feishu" },
];

/** Brand link markup. Shared with pages that build their own chrome. */
export function brandHtml() {
  return (
    '<a class="brand" href="/"><span class="mark">' +
    '<svg width="16" height="16" viewBox="0 0 18 18" fill="none" aria-hidden="true">' +
    '<rect x="1" y="1" width="16" height="16" stroke="currentColor" stroke-width="1.6"/>' +
    '<rect x="5.5" y="5.5" width="7" height="7" fill="currentColor"/></svg>' +
    "</span>evot <span>console</span></a>"
  );
}

/**
 * Render the topbar + sidenav into `document.body` and return the element that
 * page content goes into. Called before any page-specific rendering.
 *
 * `title` is the page heading; `actions` is optional HTML for the header's
 * right side; `lede` is an optional one-line description under the heading.
 */
/** Nav markup. Shared so a page that cannot use `mountShell` still gets the
 *  same entries from the same list. */
export function navHtml() {
  return NAV.map(
    (item) => '<a href="' + item.href + '">' + esc(item.name) + "</a>",
  ).join("");
}

/**
 * Render the topbar + sidenav into `document.body` and return the element that
 * page content goes into. Called before any page-specific rendering.
 *
 * Not usable by a page whose markup must survive: this replaces `body`.
 *
 * `title` is the page heading; `actions` is optional HTML for the header's
 * right side; `lede` is an optional one-line description under the heading.
 *
 * `fill` gives the page the remaining viewport instead of a scrolling content
 * column, and drops the heading block — chat owns its own scrolling and needs
 * every pixel. `actions` still renders, as a compact bar above the content.
 *
 * `actions` is inserted as HTML, so it must only ever be a literal from the
 * calling page — never server data.
 */
export function mountShell({ title, lede, actions, fill }) {
  const nav = navHtml();

  const head = fill
    ? actions
      ? '<div class="fill-bar">' + actions + "</div>"
      : ""
    : '<div class="page-head"><div><h1>' + esc(title) + "</h1>" +
      (lede ? '<p class="lede">' + esc(lede) + "</p>" : "") +
      '</div><div class="page-actions">' + (actions || "") + "</div></div>";

  if (fill) document.documentElement.classList.add("fill");
  document.title = title + " · evot";

  document.body.innerHTML =
    '<header class="topbar">' +
    brandHtml() +
    '<div class="topbar-right"><span class="cwd" id="shell-cwd"></span></div>' +
    "</header>" +
    '<div class="shell' + (fill ? " fill" : "") + '">' +
    '<aside class="sidenav"><nav>' + nav + "</nav></aside>" +
    '<div class="main">' +
    head +
    '<div class="' + (fill ? "fill-content" : "content") + '" id="content"></div>' +
    "</div></div>" +
    '<div class="toast" id="toast" role="status" aria-live="polite"></div>';

  markActiveNav();
  return $("content");
}

/** Show the working directory in the topbar. Ignored when unavailable. */
export function setShellCwd(text) {
  const el = $("shell-cwd");
  if (el && text) el.textContent = text;
}

/** Compact byte count, e.g. 1.4 GB. */
export function formatBytes(n) {
  if (!Number.isFinite(n) || n <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return (v >= 10 || i === 0 ? Math.round(v) : v.toFixed(1)) + " " + units[i];
}

/** Compact count, e.g. 12.3k. */
export function formatCount(n) {
  if (!Number.isFinite(n)) return "0";
  if (n < 1000) return String(n);
  if (n < 1_000_000) return (n / 1000).toFixed(n < 10_000 ? 1 : 0) + "k";
  return (n / 1_000_000).toFixed(1) + "M";
}

/** Coarse relative time; the console only needs day-level precision. */
export function relTime(iso) {
  if (!iso) return "";
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return "";
  const secs = Math.max(0, (Date.now() - then) / 1000);
  if (secs < 60) return "just now";
  if (secs < 3600) return Math.floor(secs / 60) + "m ago";
  if (secs < 86400) return Math.floor(secs / 3600) + "h ago";
  const days = Math.floor(secs / 86400);
  if (days < 30) return days + "d ago";
  return new Date(then).toLocaleDateString();
}

/* Catalog tier → display name. THE single copy: the Models page's Cloud rows
   and the Chat composer's optgroups both render server tier ids through this,
   so a relabel lands everywhere at once and the per-protocol provider names
   (evot-pro-anthropic, …) never reach either surface. Unknown tiers degrade to
   Title Case of the raw id, so a new server-side tier groups fine on its own
   instead of vanishing or leaking enum soup. */
const TIER_LABELS = { base: "Evot Free", special: "Evot Premium" };

export function tierLabel(tier) {
  const key = String(tier || "").trim().toLowerCase();
  if (!key) return "";
  return TIER_LABELS[key] || key.charAt(0).toUpperCase() + key.slice(1);
}
