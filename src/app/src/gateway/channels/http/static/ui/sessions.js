/* Sessions page: browse, search, favorite, and clean up saved sessions.
 *
 * Data comes from `/api/sessions`, which carries a flattened `search_text`
 * (id, title, cwd, model plus transcript snippets) so filtering and match
 * highlighting happen client-side, mirroring the terminal `/resume` selector.
 *
 * Favorites live in their own document (`/api/favorites`) rather than on the
 * session, so they survive a session rewrite.
 */
import {
  esc,
  formatBytes,
  formatCount,
  getJson,
  mountShell,
  postJson,
  relTime,
  toast,
} from "./app.js";

const PAGE_SIZE = 24;

let rows = [];
let favorites = new Set();
let selected = new Set();
let vitals = null;
let query = "";
let timeFilter = "all";
let page = 1;

/** Escape, then wrap each case-insensitive match in <mark>. */
function highlight(text, needle) {
  const escaped = esc(text);
  if (!needle) return escaped;
  const target = esc(needle).toLowerCase();
  if (!target) return escaped;
  let out = "";
  let pos = 0;
  const lower = escaped.toLowerCase();
  for (;;) {
    const at = lower.indexOf(target, pos);
    if (at < 0) break;
    out += escaped.slice(pos, at) + "<mark>" +
      escaped.slice(at, at + target.length) + "</mark>";
    pos = at + target.length;
  }
  return out + escaped.slice(pos);
}

/** The passage around the first match, so a hit shows why it matched. */
function snippet(source, needle, width) {
  if (!source) return "";
  if (!needle) return source.slice(0, width);
  const at = source.toLowerCase().indexOf(needle.toLowerCase());
  if (at < 0) return source.slice(0, width);
  const start = Math.max(0, at - Math.floor(width / 3));
  return (start > 0 ? "…" : "") + source.slice(start, start + width);
}

function matchesTime(row) {
  if (timeFilter === "all") return true;
  const at = Date.parse(row.updated_at || row.created_at || "");
  if (Number.isNaN(at)) return false;
  const ageDays = (Date.now() - at) / 86_400_000;
  if (timeFilter === "today") return ageDays < 1;
  if (timeFilter === "week") return ageDays < 7;
  if (timeFilter === "month") return ageDays < 30;
  if (timeFilter === "older") return ageDays >= 30;
  return true;
}

/** Rows after filtering, favorites first so pinned work stays reachable. */
function visibleRows() {
  const needle = query.trim().toLowerCase();
  const filtered = rows.filter((row) => {
    if (!matchesTime(row)) return false;
    if (!needle) return true;
    return (row.search_text || "").toLowerCase().includes(needle);
  });
  const starred = filtered.filter((r) => favorites.has(r.session_id));
  const rest = filtered.filter((r) => !favorites.has(r.session_id));
  return starred.concat(rest);
}

function titleOf(row) {
  if (row.title) return row.title;
  const first = (row.user_prompts || [])[0];
  return first ? first.slice(0, 120) : "Untitled session";
}

function cardHtml(row) {
  const id = row.session_id;
  const needle = query.trim();
  const starred = favorites.has(id);
  const isSelected = selected.has(id);
  const tokens = (row.total_input_tokens || 0) + (row.total_output_tokens || 0);
  const spans = row.span_count == null ? row.turns : row.span_count;

  // With a query, show the matched passage in full; otherwise a clamped
  // preview of the opening prompt.
  const body = needle
    ? '<div class="card-body full">' +
      highlight(snippet(row.search_text || "", needle, 320), needle) +
      "</div>"
    : '<div class="card-body">' + esc((row.user_prompts || [])[0] || "") + "</div>";

  return (
    '<div class="card' + (isSelected ? " selected" : "") + '" data-id="' + esc(id) + '">' +
    '<div class="card-top">' +
    '<input type="checkbox" class="pick"' + (isSelected ? " checked" : "") +
    ' aria-label="Select session ' + esc(id.slice(0, 8)) + '" />' +
    '<span class="card-id"><a href="/sessions/' + encodeURIComponent(id) + '/trace">' +
    highlight(id.slice(0, 8), needle) + "</a></span>" +
    '<span class="card-acts">' +
    '<button class="iconbtn fav' + (starred ? " on" : "") + '" data-act="fav"' +
    ' title="' + (starred ? "Remove from favorites" : "Add to favorites") + '"' +
    ' aria-label="' + (starred ? "Remove from favorites" : "Add to favorites") + '">' +
    (starred ? "\u2605" : "\u2606") + "</button>" +
    '<button class="iconbtn del" data-act="del" title="Delete session"' +
    ' aria-label="Delete session">\u00d7</button>' +
    "</span></div>" +
    '<div class="card-title">' + highlight(titleOf(row), needle) + "</div>" +
    body +
    '<div class="card-cwd" title="' + esc(row.cwd || "") + '">' +
    highlight(row.cwd || "", needle) + "</div>" +
    '<div class="card-meta"><span>' + esc(relTime(row.updated_at)) + "</span>" +
    "<span>" + formatCount(tokens) + " tok · " + (spans || 0) +
    (spans === 1 ? " span" : " spans") + "</span>" +
    "</div></div>"
  );
}

function pagerHtml(total, totalPages) {
  if (totalPages <= 1) return "";
  const buttons = [];
  const push = (label, target, extra) =>
    buttons.push(
      '<button class="btn small' + (extra || "") + '" data-page="' + target + '"' +
      (target < 1 || target > totalPages ? " disabled" : "") + ">" + label + "</button>",
    );

  push("‹ Prev", page - 1);
  // Window the numbers so a large history does not render a hundred buttons.
  const from = Math.max(1, page - 2);
  const to = Math.min(totalPages, from + 4);
  for (let n = from; n <= to; n += 1) {
    push(String(n), n, n === page ? " active" : "");
  }
  push("Next ›", page + 1);

  const start = (page - 1) * PAGE_SIZE + 1;
  const end = Math.min(total, page * PAGE_SIZE);
  return (
    '<div class="pager">' + buttons.join("") +
    '<span class="info">' + start + "–" + end + " of " + total + "</span></div>"
  );
}

/** Host gauges. Rendered only when the endpoint answered. */
function vitalsHtml() {
  if (!vitals) return "";
  const totalTokens = rows.reduce(
    (sum, r) => sum + (r.total_input_tokens || 0) + (r.total_output_tokens || 0),
    0,
  );
  const stat = (label, value, sub) =>
    '<div class="stat"><div class="label">' + esc(label) + "</div>" +
    '<div class="value">' + esc(value) + "</div>" +
    (sub ? '<div class="sub">' + esc(sub) + "</div>" : "") +
    "</div>";

  return (
    '<div class="stats">' +
    stat("Sessions", String(rows.length), formatCount(totalTokens) + " tokens total") +
    (vitals.cpu_available
      ? stat("CPU", vitals.cpu_percent.toFixed(0) + "%", "")
      : stat("CPU", "n/a", "")) +
    stat(
      "Memory",
      Math.round(vitals.ram_percent || 0) + "%",
      formatBytes(vitals.ram_used) + " of " + formatBytes(vitals.ram_total),
    ) +
    stat(
      "Disk",
      Math.round(vitals.disk_percent || 0) + "%",
      formatBytes(vitals.disk_used) + " of " + formatBytes(vitals.disk_total),
    ) +
    "</div>"
  );
}

function render() {
  const all = visibleRows();
  const totalPages = Math.max(1, Math.ceil(all.length / PAGE_SIZE));
  if (page > totalPages) page = totalPages;
  const pageRows = all.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE);
  const options = [
    ["all", "All time"],
    ["today", "Today"],
    ["week", "Past week"],
    ["month", "Past month"],
    ["older", "Older than 30d"],
  ]
    .map(
      ([value, label]) =>
        '<option value="' + value + '"' +
        (value === timeFilter ? " selected" : "") + ">" + label + "</option>",
    )
    .join("");

  document.getElementById("content").innerHTML =
    vitalsHtml() +
    '<div class="toolbar">' +
    '<div class="search"><input id="q" type="search" autocomplete="off"' +
    ' placeholder="Search by id, path, or content…" value="' + esc(query) + '"' +
    ' aria-label="Search sessions" /></div>' +
    '<select id="time" aria-label="Filter by time">' + options + "</select>" +
    '<span class="count">' + all.length + " of " + rows.length + "</span>" +
    '<span class="spacer"></span>' +
    '<button class="btn small danger" id="clean"' +
    (selected.size ? "" : " disabled") + ">Delete selected" +
    (selected.size ? " (" + selected.size + ")" : "") + "</button>" +
    "</div>" +
    (pageRows.length
      ? '<div class="grid">' + pageRows.map(cardHtml).join("") + "</div>"
      : '<div class="empty">' +
        (rows.length ? "No sessions match." : "No saved sessions yet.") +
        "</div>") +
    pagerHtml(all.length, totalPages);

  wire();
}

/* Listeners are attached per render. The grid is small (a page at a time) and
   re-rendering is driven by user actions, not a timer, so delegation would add
   indirection without saving meaningful work. */
function wire() {
  const q = document.getElementById("q");
  q.addEventListener("input", () => {
    query = q.value;
    page = 1;
    render();
    // Re-rendering replaces the input, so focus and caret are restored to keep
    // typing continuous.
    const live = document.getElementById("q");
    live.focus();
    live.setSelectionRange(live.value.length, live.value.length);
  });

  document.getElementById("time").addEventListener("change", (e) => {
    timeFilter = e.target.value;
    page = 1;
    render();
  });

  document.getElementById("clean").addEventListener("click", deleteSelected);

  document.querySelectorAll(".pager .btn").forEach((btn) => {
    btn.addEventListener("click", () => {
      const target = Number(btn.getAttribute("data-page"));
      if (Number.isFinite(target)) {
        page = target;
        render();
        window.scrollTo({ top: 0 });
      }
    });
  });

  document.querySelectorAll(".card").forEach((card) => {
    const id = card.getAttribute("data-id");
    card.querySelector(".pick").addEventListener("change", (e) => {
      if (e.target.checked) selected.add(id);
      else selected.delete(id);
      render();
    });
    card.querySelector('[data-act="fav"]').addEventListener("click", () => {
      void toggleFavorite(id);
    });
    card.querySelector('[data-act="del"]').addEventListener("click", () => {
      void deleteOne(id);
    });
  });
}

async function toggleFavorite(id) {
  try {
    const res = await postJson("/api/favorites/toggle", { id });
    if (res && res.favorited) favorites.add(id);
    else favorites.delete(id);
    render();
  } catch (err) {
    toast(String(err.message || err), "err");
  }
}

async function deleteOne(id) {
  if (!window.confirm("Delete session " + id.slice(0, 8) + "? This cannot be undone.")) {
    return;
  }
  await deleteIds([id]);
}

async function deleteSelected() {
  const ids = [...selected];
  if (!ids.length) return;
  const label = ids.length === 1 ? "1 session" : ids.length + " sessions";
  if (!window.confirm("Delete " + label + "? This cannot be undone.")) return;
  await deleteIds(ids);
}

/**
 * Delete and drop the rows locally rather than refetching: the server reports
 * exactly which ids went, so a reload would only cost a round trip.
 */
async function deleteIds(ids) {
  try {
    const res = await postJson("/api/sessions/delete", { ids });
    const gone = new Set(res && res.deleted_ids ? res.deleted_ids : ids);
    rows = rows.filter((row) => !gone.has(row.session_id));
    gone.forEach((id) => {
      selected.delete(id);
      favorites.delete(id);
    });
    render();
    const failed = (res && res.failed) || [];
    if (failed.length) {
      toast("Deleted " + gone.size + ", failed " + failed.length, "err");
    } else {
      toast("Deleted " + gone.size);
    }
  } catch (err) {
    toast(String(err.message || err), "err");
  }
}

async function load() {
  const root = mountShell({
    title: "Sessions",
    lede: "Saved sessions on this machine. Open one to read its trace.",
  });
  root.innerHTML = '<div class="empty">Loading…</div>';
  try {
    // Favorites are ordering input, so both land before the first paint.
    // Vitals are decoration: a failure there must not blank the page.
    const [sessions, favs, host] = await Promise.all([
      getJson("/api/sessions"),
      getJson("/api/favorites").catch(() => ({ ids: [] })),
      getJson("/api/vitals").catch(() => null),
    ]);
    rows = Array.isArray(sessions) ? sessions : [];
    favorites = new Set((favs && favs.ids) || []);
    vitals = host;
    render();
  } catch (err) {
    root.innerHTML =
      '<div class="empty">Could not load sessions: ' +
      esc(String(err.message || err)) + "</div>";
  }
}

void load();
