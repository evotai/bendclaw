/* Models settings has two concepts and only two sections:
 *   Cloud  — account-managed, read-only providers.
 *   Custom — user-managed providers with an editor.
 *
 * `active_provider` is the authoritative default; provider order is only
 * presentation. Secrets never arrive from the server, so a blank key field
 * means keep the stored value.
 */
import { esc, getJson, mountShell, postJson, setShellCwd, skeletonHtml, tierLabel, toast } from "./app.js";

let state = null;
let selectedCustom = null;
/** Cloud default picked but not yet saved; cleared once the save lands. */
let pendingCloudModel = null;
const newKeys = new Map();

const isCloud = (provider) => provider.cloud === true;
const customProviders = () => state.providers.filter((provider) => !isCloud(provider));
const providerByName = (name) => state.providers.find((provider) => provider.name === name) || null;
const defaultName = () => state.active_provider || "";

function options(values, current) {
  return values.map((value) =>
    '<option value="' + esc(value) + '"' + (value === current ? " selected" : "") + ">" +
    esc(value) + "</option>",
  ).join("");
}

const cloudTiers = () => (Array.isArray(state.cloud_tiers) ? state.cloud_tiers : []);

function modelChips(models) {
  if (!models.length) return '<span class="model-chip">No models</span>';
  return models.map((model) => '<span class="model-chip">' + esc(model) + "</span>").join("");
}

/**
 * Cloud directory grouped by catalog tier. The per-protocol provider names the
 * server splits tiers into never render; each chip carries the provider it is
 * served by so picking it can persist both halves of the default.
 */
function cloudHtml() {
  const tiers = cloudTiers();
  if (!tiers.length) {
    return '<div class="section-empty">No Cloud models. Run <code>evot login</code> to connect your account.</div>';
  }
  return '<div class="cloud-list">' + tiers.map((group) =>
    '<div class="cloud-row">' +
    '<div class="provider-name"><span class="dot on"></span><span>' + esc(tierLabel(group.tier)) + "</span></div>" +
    '<div class="model-chips">' + group.models.map((model) =>
      '<button type="button" class="model-chip select' + (model.active ? " active" : "") +
      '" data-cloud-provider="' + esc(model.provider) +
      '" data-cloud-model="' + esc(model.id) + '">' + esc(model.id) + "</button>").join("") +
    "</div></div>").join("") + "</div>";
}

function customListHtml() {
  const providers = customProviders();
  if (!providers.length) return '<div class="section-empty">No custom providers.</div>';
  return providers.map((provider) =>
    '<button type="button" class="custom-item" data-custom="' + esc(provider.name) +
    '" aria-selected="' + (provider.name === selectedCustom ? "true" : "false") + '">' +
    '<span class="dot ' + (provider.api_key_set ? "on" : "off") + '"></span>' +
    '<span class="name">' + esc(provider.name) + "</span>" +
    (provider.name === defaultName()
      ? '<span class="default-label">Default</span>'
      : '<span class="state">' + (provider.api_key_set ? "Ready" : "No key") + "</span>") +
    "</button>",
  ).join("");
}

function customEditorHtml() {
  const provider = providerByName(selectedCustom);
  if (!provider || isCloud(provider)) {
    return '<div class="section-empty">Add or select a custom provider to edit it.</div>';
  }
  const pendingKey = newKeys.get(provider.name) || "";
  const keyPlaceholder = provider.api_key_set
    ? "Stored " + provider.api_key_hint + " — leave blank to keep"
    : "API key";
  const active = provider.name === defaultName();

  return '<div class="editor-head"><h3>' + esc(provider.name) + "</h3>" +
    '<div class="actions">' +
    (active ? '<span class="default-label">Default</span>' : '<button type="button" class="btn small" id="makeDefault">Set default</button>') +
    '<button type="button" class="btn small danger" id="removeCustom">Remove</button>' +
    "</div></div>" +
    '<div class="form">' +
    '<div class="field"><label for="providerName">Name</label>' +
    '<input id="providerName" value="' + esc(provider.name) + '" spellcheck="false" /></div>' +
    '<div class="field"><label for="providerProtocol">Protocol</label>' +
    '<select id="providerProtocol">' + options(state.protocols, provider.protocol) + "</select></div>" +
    '<div class="field full"><label for="providerBase">Base URL</label>' +
    '<input id="providerBase" value="' + esc(provider.base_url) + '" spellcheck="false" placeholder="https://api.example.com/v1" /></div>' +
    '<div class="field full"><label for="providerKey">API key</label>' +
    '<input id="providerKey" type="password" autocomplete="off" value="' + esc(pendingKey) +
    '" placeholder="' + esc(keyPlaceholder) + '" />' +
    '<div class="help">Never shown after save. Leave blank to keep the stored key.</div></div>' +
    '<div class="field full"><label for="providerModels">Models</label>' +
    '<input id="providerModels" value="' + esc(provider.models.join(", ")) + '" spellcheck="false" placeholder="model-a, model-b" />' +
    '<div class="help">Comma separated. The first model is this provider’s default.</div></div>' +
    '<div class="field"><label for="providerThinking">Thinking override</label>' +
    '<select id="providerThinking"><option value="">Use global default</option>' +
    options(state.thinking_levels, provider.thinking_level || "") + "</select></div>" +
    "</div>";
}

function render() {
  const models = cloudTiers().reduce((sum, group) => sum + group.models.length, 0);
  const tierCount = cloudTiers().length;
  const customCount = customProviders().length;
  document.getElementById("content").innerHTML =
    '<section class="model-section">' +
    '<div class="section-head"><div class="section-copy"><h2>Cloud</h2>' +
    '<p>Managed by your evot account. Read-only and refreshed at startup.</p></div>' +
    '<span class="section-count">' + tierCount + " tier" + (tierCount === 1 ? "" : "s") + " · " +
      models + " model" + (models === 1 ? "" : "s") + "</span></div>" +
    cloudHtml() + "</section>" +
    '<section class="model-section">' +
    '<div class="section-head"><div class="section-copy"><h2>Custom</h2>' +
    '<p>Your own API providers, endpoints, keys, and model ids.</p></div>' +
    '<span class="section-count">' + customCount + " provider" + (customCount === 1 ? "" : "s") + "</span>" +
    '<button type="button" class="btn small" id="addCustom">+ Add</button></div>' +
    '<div class="custom-body"><div class="custom-list">' + customListHtml() + "</div>" +
    '<div class="custom-editor">' + customEditorHtml() + "</div></div>" +
    "</section>" +
    '<p class="envpath">Saved to <code>' + esc(state.env_file_path) + "</code></p>";
  wire();
}

/** Copy the visible custom editor into state before any repaint or save. */
function syncEditor() {
  const provider = providerByName(selectedCustom);
  const nameInput = document.getElementById("providerName");
  if (!provider || isCloud(provider) || !nameInput) return;

  const oldName = provider.name;
  const key = document.getElementById("providerKey").value;
  if (key) newKeys.set(oldName, key);
  else newKeys.delete(oldName);

  const nextName = nameInput.value.trim().toLowerCase();
  if (nextName && nextName !== oldName) {
    const pending = newKeys.get(oldName);
    if (pending !== undefined) {
      newKeys.delete(oldName);
      newKeys.set(nextName, pending);
    }
    provider.name = nextName;
    selectedCustom = nextName;
    if (state.active_provider === oldName) state.active_provider = nextName;
  }
  provider.protocol = document.getElementById("providerProtocol").value;
  provider.base_url = document.getElementById("providerBase").value.trim();
  provider.models = document.getElementById("providerModels").value
    .split(",").map((model) => model.trim()).filter(Boolean);
  provider.thinking_level = document.getElementById("providerThinking").value || null;
}

function syncGlobalThinking() {
  const select = document.getElementById("globalThinking");
  if (select) state.thinking_level = select.value || null;
}

function makeDefault(name) {
  syncEditor();
  syncGlobalThinking();
  if (providerByName(name)) state.active_provider = name;
  render();
}

function addCustom() {
  syncEditor();
  syncGlobalThinking();
  let name = "custom";
  let suffix = 2;
  while (providerByName(name)) name = "custom-" + suffix++;
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
  selectedCustom = name;
  render();
}

function removeCustom() {
  const index = state.providers.findIndex((provider) => provider.name === selectedCustom && !isCloud(provider));
  if (index < 0) return;
  const [removed] = state.providers.splice(index, 1);
  newKeys.delete(removed.name);
  if (state.active_provider === removed.name) {
    state.active_provider = state.providers[0]?.name || "";
  }
  selectedCustom = customProviders()[0]?.name || null;
  render();
}

function wire() {
  document.querySelectorAll("[data-custom]").forEach((button) => {
    button.addEventListener("click", () => {
      syncEditor();
      syncGlobalThinking();
      selectedCustom = button.dataset.custom;
      render();
    });
  });
  document.querySelectorAll("[data-default]").forEach((button) => {
    button.addEventListener("click", () => makeDefault(button.dataset.default));
  });
  // A cloud chip picks both halves of the default: the tier's provider keeps
  // serving the request, the model becomes that profile's head.
  document.querySelectorAll("[data-cloud-model]").forEach((chip) => {
    chip.addEventListener("click", () => {
      if (chip.classList.contains("active")) return;
      state.active_provider = chip.dataset.cloudProvider;
      pendingCloudModel = chip.dataset.cloudModel;
      void save();
    });
  });
  document.getElementById("addCustom").addEventListener("click", addCustom);
  document.getElementById("makeDefault")?.addEventListener("click", () => makeDefault(selectedCustom));
  document.getElementById("removeCustom")?.addEventListener("click", removeCustom);
}

function payload() {
  return {
    active_provider: defaultName(),
    active_model: pendingCloudModel,
    thinking_level: state.thinking_level || null,
    providers: customProviders().map((provider) => ({
      name: provider.name,
      protocol: provider.protocol,
      base_url: provider.base_url,
      models: provider.models,
      thinking_level: provider.thinking_level,
      api_key: newKeys.get(provider.name) || null,
    })),
  };
}

async function save() {
  syncEditor();
  syncGlobalThinking();
  const button = document.getElementById("saveModels");
  button.disabled = true;
  try {
    const response = await postJson("/api/models", payload());
    newKeys.clear();
    pendingCloudModel = null;
    state = response.models;
    if (!providerByName(selectedCustom) || isCloud(providerByName(selectedCustom))) {
      selectedCustom = customProviders()[0]?.name || null;
    }
    render();
    toast("Saved");
  } catch (error) {
    toast(String(error.message || error), "err");
  } finally {
    const live = document.getElementById("saveModels");
    if (live) live.disabled = false;
  }
}

async function load() {
  const root = mountShell({
    title: "Models",
    lede: "Choose account models or configure your own provider.",
    actions:
      '<label class="default-thinking">Default thinking <select id="globalThinking"></select></label>' +
      '<button class="btn primary" id="saveModels">Save changes</button>',
  });
  root.innerHTML =
    '<section class="model-section" aria-busy="true">' +
    '<div class="section-head"><div class="section-copy"><h2>Cloud</h2>' +
    '<p>Managed by your evot account.</p></div></div>' +
    skeletonHtml(3, "cloud") + "</section>" +
    '<section class="model-section" aria-busy="true">' +
    '<div class="section-head"><div class="section-copy"><h2>Custom</h2>' +
    '<p>Your own API providers.</p></div></div>' +
    skeletonHtml(4) + "</section>";
  document.getElementById("saveModels").addEventListener("click", save);
  try {
    state = await getJson("/api/models");
    selectedCustom = customProviders().find((provider) => provider.name === state.active_provider)?.name ||
      customProviders()[0]?.name || null;
    const global = document.getElementById("globalThinking");
    global.innerHTML = '<option value="">Model default</option>' +
      options(state.thinking_levels, state.thinking_level || "");
    setShellCwd(state.env_file_path);
    render();
  } catch (error) {
    root.innerHTML = '<div class="empty">Could not load models: ' + esc(String(error.message || error)) + "</div>";
  }
}

void load();
