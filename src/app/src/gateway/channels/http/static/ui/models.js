/* Models page: pick a provider on the left, edit it on the right.
 *
 * Server contract (GET/POST /api/models): providers arrive ordered, and the
 * first one is the default used for new messages. Secrets are never sent down —
 * only `api_key_set` and a masked hint — so a blank key field on save means
 * "keep what is stored".
 *
 * Cloud providers come from `evot login` and are re-registered from the models
 * cache on every start. They render read-only: editing them here would be
 * silently undone on the next load.
 */
import {
  esc,
  getJson,
  mountShell,
  postJson,
  setShellCwd,
  toast,
} from "./app.js";

/** Server snapshot. Mutated in place as fields are edited, saved as a whole. */
let state = null;
/** Name of the provider shown in the editor. */
let selected = null;
/** Pending new secrets, keyed by provider name; never populated from a load. */
const newKeys = new Map();

function isCloud(p) {
  return p.cloud === true;
}

function providerByName(name) {
  return state.providers.find((p) => p.name === name) || null;
}

/** The default is positional: whichever provider sits first. */
function defaultName() {
  return state.providers.length ? state.providers[0].name : "";
}

function optionList(values, current) {
  return values
    .map(
      (v) =>
        '<option value="' + esc(v) + '"' +
        (v === current ? " selected" : "") +
        ">" + esc(v) + "</option>",
    )
    .join("");
}

function renderList() {
  const def = defaultName();
  return state.providers
    .map((p) => {
      const tag = isCloud(p)
        ? "cloud"
        : p.api_key_set
          ? ""
          : "no key";
      return (
        '<button type="button" class="pitem" role="tab" data-name="' +
        esc(p.name) +
        '" aria-selected="' +
        (p.name === selected ? "true" : "false") +
        '">' +
        '<span class="dot ' + (isCloud(p) || p.api_key_set ? "on" : "off") + '"></span>' +
        '<span class="pname">' + esc(p.name) + "</span>" +
        (p.name === def ? '<span class="tag">default</span>' : "") +
        (tag ? '<span class="tag">' + esc(tag) + "</span>" : "") +
        "</button>"
      );
    })
    .join("");
}

function renderEditor() {
  const p = providerByName(selected);
  if (!p) {
    return '<div class="empty">No provider selected.</div>';
  }
  const isDefault = p.name === defaultName();
  const head =
    '<div class="editor-head"><h3>' + esc(p.name) + "</h3>" +
    (isDefault ? '<span class="default-badge">default</span>' : "") +
    (isCloud(p) ? '<span class="badge">cloud</span>' : "") +
    '<div class="right">' +
    (isDefault
      ? ""
      : '<button class="btn small" id="makedefault">Set as default</button>') +
    (isCloud(p)
      ? ""
      : '<button class="btn small danger" id="remove">Remove</button>') +
    "</div></div>";

  if (isCloud(p)) {
    return (
      head +
      '<div class="cloud-note">Managed by your evot account and refreshed on ' +
      "every start. Run <code>evot logout</code> to remove it.</div>" +
      '<div class="form">' +
      '<div class="field"><label>Protocol</label>' +
      '<input value="' + esc(p.protocol) + '" disabled /></div>' +
      '<div class="field"><label>Models</label>' +
      '<input value="' + esc(p.models.join(", ")) + '" disabled /></div>' +
      "</div>"
    );
  }

  const pendingKey = newKeys.get(p.name);
  const keyPlaceholder = p.api_key_set
    ? "stored " + p.api_key_hint + " — leave blank to keep"
    : "not set";

  return (
    head +
    '<div class="form">' +
    '<div class="field"><label for="f-name">Name</label>' +
    '<input id="f-name" value="' + esc(p.name) + '" spellcheck="false" /></div>' +
    '<div class="field"><label for="f-protocol">Protocol</label>' +
    '<select id="f-protocol">' + optionList(state.protocols, p.protocol) + "</select></div>" +
    '<div class="field full"><label for="f-base">Base URL</label>' +
    '<input id="f-base" value="' + esc(p.base_url) + '" spellcheck="false" /></div>' +
    '<div class="field full"><label for="f-key">API key</label>' +
    '<input id="f-key" type="password" autocomplete="off" value="' +
    esc(pendingKey || "") +
    '" placeholder="' + esc(keyPlaceholder) + '" />' +
    '<div class="help">Stored in your env file. Leave blank to keep the current key.</div></div>' +
    '<div class="field full"><label for="f-models">Models</label>' +
    '<input id="f-models" value="' + esc(p.models.join(", ")) + '" spellcheck="false" />' +
    '<div class="help">Comma separated. The first one is used by default.</div></div>' +
    '<div class="field"><label for="f-thinking">Thinking level</label>' +
    '<select id="f-thinking"><option value="">use global</option>' +
    optionList(state.thinking_levels, p.thinking_level || "") +
    "</select>" +
    '<div class="help">Overrides the global level for this provider.</div></div>' +
    "</div>"
  );
}

function render() {
  const root = document.getElementById("content");
  root.innerHTML =
    '<div class="panel"><div class="panel-head"><h3>Global</h3></div>' +
    '<div class="panel-body"><div class="form">' +
    '<div class="field"><label for="g-thinking">Thinking level</label>' +
    '<select id="g-thinking"><option value="">model default</option>' +
    optionList(state.thinking_levels, state.thinking_level || "") +
    "</select>" +
    '<div class="help">Applies to every provider without its own override.</div>' +
    "</div></div></div></div>" +
    '<div class="split">' +
    '<div class="plist"><div class="plist-items" role="tablist">' + renderList() + "</div>" +
    '<div class="plist-foot"><button class="btn small" id="add">+ Add provider</button></div>' +
    "</div>" +
    '<div class="editor" id="editor">' + renderEditor() + "</div>" +
    "</div>" +
    '<p class="envpath">Saved to <code>' + esc(state.env_file_path) + "</code></p>";
  wire();
}

/**
 * Copy the editor inputs back into `state`. Called before any action that
 * re-renders or saves, so edits are never lost to a repaint.
 *
 * Cloud providers have no editable inputs, so they are skipped: reading
 * `.value` off absent fields would throw and take the whole save with it.
 */
function syncEditor() {
  const p = providerByName(selected);
  if (!p || isCloud(p)) return;
  const name = document.getElementById("f-name");
  if (!name) return;

  const key = document.getElementById("f-key").value;
  if (key) newKeys.set(p.name, key);
  else newKeys.delete(p.name);

  const nextName = name.value.trim().toLowerCase();
  if (nextName && nextName !== p.name) {
    // Carry a pending secret across the rename so it is not stranded under the
    // old key when the payload is built.
    const pending = newKeys.get(p.name);
    if (pending !== undefined) {
      newKeys.delete(p.name);
      newKeys.set(nextName, pending);
    }
    p.name = nextName;
    selected = nextName;
  }

  p.protocol = document.getElementById("f-protocol").value;
  p.base_url = document.getElementById("f-base").value.trim();
  p.models = document
    .getElementById("f-models")
    .value.split(",")
    .map((m) => m.trim())
    .filter(Boolean);
  p.thinking_level = document.getElementById("f-thinking").value || null;
}

function syncGlobal() {
  const g = document.getElementById("g-thinking");
  if (g) state.thinking_level = g.value || null;
}

function wire() {
  document.querySelectorAll(".pitem").forEach((el) => {
    el.addEventListener("click", () => {
      syncEditor();
      syncGlobal();
      selected = el.getAttribute("data-name");
      render();
    });
  });

  document.getElementById("add").addEventListener("click", () => {
    syncEditor();
    syncGlobal();
    const base = "new-provider";
    let name = base;
    let n = 2;
    while (providerByName(name)) name = base + "-" + n++;
    state.providers.push({
      name,
      cloud: false,
      protocol: state.protocols[0],
      api_key_set: false,
      api_key_hint: "",
      base_url: "",
      models: [],
      thinking_level: null,
    });
    selected = name;
    render();
  });

  const makeDefault = document.getElementById("makedefault");
  if (makeDefault) {
    makeDefault.addEventListener("click", () => {
      syncEditor();
      syncGlobal();
      const i = state.providers.findIndex((p) => p.name === selected);
      if (i > 0) {
        const [p] = state.providers.splice(i, 1);
        state.providers.unshift(p);
      }
      render();
    });
  }

  const remove = document.getElementById("remove");
  if (remove) {
    remove.addEventListener("click", () => {
      const i = state.providers.findIndex((p) => p.name === selected);
      if (i < 0) return;
      state.providers.splice(i, 1);
      newKeys.delete(selected);
      selected = state.providers.length ? state.providers[0].name : null;
      render();
    });
  }
}

/**
 * Build the payload. Cloud providers are filtered out — they are server-owned —
 * but the active name is read before filtering so a cloud provider can still
 * hold the default slot.
 */
function buildPayload() {
  return {
    active_provider: defaultName(),
    thinking_level: state.thinking_level || null,
    providers: state.providers
      .filter((p) => !isCloud(p))
      .map((p) => ({
        name: p.name,
        protocol: p.protocol,
        base_url: p.base_url,
        models: p.models,
        thinking_level: p.thinking_level,
        api_key: newKeys.get(p.name) || null,
      })),
  };
}

async function save() {
  syncEditor();
  syncGlobal();
  const btn = document.getElementById("save");
  btn.disabled = true;
  try {
    const res = await postJson("/api/models", buildPayload());
    newKeys.clear();
    state = res.models;
    if (!providerByName(selected)) selected = defaultName() || null;
    render();
    toast("Saved");
  } catch (err) {
    toast(String(err.message || err), "err");
  } finally {
    const live = document.getElementById("save");
    if (live) live.disabled = false;
  }
}

async function load() {
  const root = mountShell({
    title: "Models",
    lede: "Providers the agent can talk to. The first one is used for new messages.",
    actions: '<button class="btn primary" id="save">Save changes</button>',
  });
  root.innerHTML = '<div class="empty">Loading…</div>';
  // The Save button lives in the shell header, outside the re-rendered content,
  // so it is wired once here rather than on every render.
  document.getElementById("save").addEventListener("click", save);
  try {
    state = await getJson("/api/models");
    selected = state.active_provider || defaultName() || null;
    if (!providerByName(selected)) selected = defaultName() || null;
    setShellCwd(state.env_file_path);
    render();
  } catch (err) {
    root.innerHTML =
      '<div class="empty">Could not load models: ' + esc(String(err.message || err)) + "</div>";
  }
}

void load();
