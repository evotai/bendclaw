/* Feishu page: link a bot so the agent can be reached from chat.
 *
 * Server contract (GET/POST /api/channels/feishu): the secret is never sent
 * down, only `app_secret_set` and a masked hint, so a blank field on save means
 * "keep what is stored". Clearing the app id unlinks the channel.
 *
 * The channel is spawned at startup, so a save persists the config but does not
 * take effect until evot restarts. The response says so and the page repeats it
 * rather than implying the bot is live.
 */
import {
  esc,
  getJson,
  mountShell,
  postJson,
  setShellCwd,
  skeletonHtml,
  toast,
} from "./app.js";

let state = null;

function feishu() {
  return (state && state.feishu) || null;
}

function render() {
  const f = feishu();
  const linked = f !== null;
  const secretPlaceholder = linked && f.app_secret_set
    ? "stored " + f.app_secret_hint + " — leave blank to keep"
    : "not set";

  document.getElementById("content").innerHTML =
    '<div class="panel">' +
    '<div class="panel-head"><h3>Bot credentials</h3>' +
    '<div class="right"><span class="dot ' + (linked ? "on" : "off") + '"></span>' +
    '<span class="badge' + (linked ? " green" : "") + '">' +
    (linked ? "linked" : "not linked") +
    "</span></div></div>" +
    '<div class="panel-body"><div class="form">' +
    '<div class="field"><label for="f-appid">App ID</label>' +
    '<input id="f-appid" value="' + esc(linked ? f.app_id : "") +
    '" spellcheck="false" placeholder="cli_..." />' +
    '<div class="help">Clear this field to unlink the bot.</div></div>' +
    '<div class="field"><label for="f-secret">App secret</label>' +
    '<input id="f-secret" type="password" autocomplete="off" placeholder="' +
    esc(secretPlaceholder) + '" />' +
    '<div class="help">Stored in your env file. Leave blank to keep the current secret.</div></div>' +
    '<div class="field full"><label class="check">' +
    '<input type="checkbox" id="f-mention"' +
    (!linked || f.mention_only ? " checked" : "") + " />" +
    "<span>Only reply when mentioned</span></label>" +
    '<div class="help">Off means the bot answers every message in a group it belongs to.</div>' +
    "</div></div></div>" +
    '<div class="panel-foot"><span class="hint">Changes apply after evot restarts.</span></div>' +
    "</div>" +
    '<p class="envpath">Saved to <code>' + esc(state.env_file_path) + "</code></p>";
}

function buildPayload() {
  const secret = document.getElementById("f-secret").value;
  return {
    app_id: document.getElementById("f-appid").value.trim(),
    app_secret: secret.length ? secret : null,
    mention_only: document.getElementById("f-mention").checked,
  };
}

async function save() {
  const btn = document.getElementById("save");
  btn.disabled = true;
  try {
    const res = await postJson("/api/channels/feishu", buildPayload());
    // The response carries a fresh snapshot in the same shape as the GET.
    state = res.channel;
    render();
    toast("Saved · restart evot to apply");
  } catch (err) {
    toast(String(err.message || err), "err");
  } finally {
    const live = document.getElementById("save");
    if (live) live.disabled = false;
  }
}

async function load() {
  const root = mountShell({
    title: "Feishu",
    lede: "Link a Feishu bot so the agent can be reached from chat.",
    actions: '<button class="btn primary" id="save">Save changes</button>',
  });
  root.innerHTML = skeletonHtml(3, "form");
  document.getElementById("save").addEventListener("click", save);
  try {
    state = await getJson("/api/channels/feishu");
    setShellCwd(state.env_file_path);
    render();
  } catch (err) {
    root.innerHTML =
      '<div class="empty">Could not load channel config: ' +
      esc(String(err.message || err)) + "</div>";
  }
}

void load();
