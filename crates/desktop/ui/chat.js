// One conversation, in its own window.
"use strict";

const $ = (id) => document.getElementById(id);
const peer = decodeURIComponent(location.hash.slice(1));
const shown = new Set();
let firstRender = true;

const STATUS = {
  queued: {
    label: "W kolejce",
    svg: '<svg viewBox="0 0 16 12" aria-hidden="true"><circle cx="8" cy="6" r="4.6" fill="none" stroke="currentColor" stroke-width="1.4"/><path d="M8 3.6V6l1.6 1.2" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/></svg>',
  },
  sent: {
    label: "Wysłano",
    svg: '<svg viewBox="0 0 16 12" aria-hidden="true"><path d="M3 6.4 6 9.2 12.6 2.8" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>',
  },
  delivered: {
    label: "Dostarczono",
    svg: '<svg viewBox="0 0 16 12" aria-hidden="true"><path d="M1 6.4 4 9.2 10.6 2.8M6.6 8.4l.8.8L14 2.8" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>',
  },
};

const KIND_ICON = {
  program: "📦",
  archive: "🗜️",
  image: "🖼️",
  audio: "🎵",
  video: "🎬",
  document: "📄",
  other: "📎",
};

const sizeFormat = new Intl.NumberFormat("pl-PL", { maximumFractionDigits: 1 });

function formatSize(bytes) {
  const units = ["B", "kB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  return `${sizeFormat.format(value)} ${units[unit]}`;
}

// First 16 hex digits of the hash, in groups: enough to compare by voice.
function fingerprint(hash) {
  return hash.slice(0, 16).match(/.{4}/g).join(" ");
}

// Players and pictures survive redraws, so music keeps playing while other
// messages arrive.
const mediaCache = new Map();

function media(message, tag) {
  const key = `${message.id}:${message.file.path}`;
  let node = mediaCache.get(key);
  if (!node) {
    const src = tauri.core.convertFileSrc(message.file.path);
    if (tag === "img") {
      node = el("img", { class: "file-preview", src, alt: message.file.name, loading: "lazy" });
      node.addEventListener("click", () => act("open_file", message.id));
    } else {
      node = el(tag, { src, controls: true, preload: "metadata" });
    }
    mediaCache.set(key, node);
  }
  return node;
}

async function act(command, id) {
  try {
    await invoke(command, { peer, id });
    showProblem("");
  } catch (err) {
    showProblem(friendly(err));
  }
}

function fileCard(message, alias) {
  const file = message.file;
  const card = el("div", { class: "file" });
  card.append(el("span", { class: "file-icon", "aria-hidden": "true", text: KIND_ICON[file.kind] || "📎" }));
  card.append(el("span", { class: "file-name", text: file.name }));

  let meta = formatSize(file.size);
  if (file.transfer === "downloading") meta = `${formatSize(file.done)} z ${formatSize(file.size)}`;
  if (file.transfer === "saved") meta += ", zapisany";
  if (file.transfer === "shared") meta += ", udostępniony z tego urządzenia";
  card.append(el("span", { class: "file-meta", text: meta }));

  if (file.kind === "program" && !message.mine) {
    card.append(
      el(
        "p",
        { class: "file-warning" },
        `Program od: ${alias}. Uruchamiaj go tylko, jeśli wiesz, co to jest. Odcisk: `,
        el("span", { class: "fingerprint", text: fingerprint(file.hash) }),
      ),
    );
  }

  if (file.transfer === "downloading") {
    card.append(el("progress", { max: String(file.size || 1), value: String(file.done) }));
  }
  if (file.transfer === "failed" && file.reason) {
    card.append(el("p", { class: "file-error", text: `Pobieranie przerwane: ${friendly(file.reason)}` }));
  }

  const local = file.path && (file.transfer === "saved" || file.transfer === "shared");
  if (local && file.kind === "image") card.append(media(message, "img"));
  if (local && file.kind === "audio") card.append(media(message, "audio"));
  if (local && file.kind === "video") card.append(media(message, "video"));

  const actions = el("div", { class: "file-actions" });
  if (file.transfer === "offered" || file.transfer === "failed") {
    actions.append(
      el("button", {
        class: "button primary",
        type: "button",
        text: file.transfer === "failed" ? "Wznów pobieranie" : "Pobierz",
        onclick: () => act("download", message.id),
      }),
    );
  }
  if (local) {
    if (file.kind !== "program") {
      actions.append(el("button", { class: "button", type: "button", text: "Otwórz", onclick: () => act("open_file", message.id) }));
    }
    actions.append(el("button", { class: "button", type: "button", text: "Pokaż w folderze", onclick: () => act("reveal", message.id) }));
  }
  if (actions.childElementCount) card.append(actions);
  return card;
}

function showProblem(text) {
  $("problem").textContent = text;
  $("problem").hidden = !text;
}

const timeFormat = new Intl.DateTimeFormat("pl-PL", { hour: "2-digit", minute: "2-digit" });
const dayFormat = new Intl.DateTimeFormat("pl-PL", { weekday: "long", day: "numeric", month: "long" });

async function refresh() {
  let data;
  try {
    data = await invoke("chat", { peer });
  } catch (err) {
    $("presence").textContent = friendly(err);
    $("draft").disabled = true;
    $("send").disabled = true;
    return;
  }
  document.title = `${data.alias} – Szept`;
  $("alias").textContent = data.alias;
  $("avatar").textContent = [...data.alias][0] || "?";
  $("avatar").dataset.online = String(data.online);
  $("presence").textContent = data.online
    ? "Połączony"
    : "Niepołączony. Wiadomości wyjdą, gdy kontakt się pojawi.";
  renderMessages(data.messages, data.alias);
}

function renderMessages(messages, alias) {
  const box = $("messages");
  const nearBottom = box.scrollHeight - box.scrollTop - box.clientHeight < 80;
  const nodes = [];
  let lastDay = "";
  const lastMine = messages.findLastIndex((message) => message.mine);

  messages.forEach((message, index) => {
    const when = new Date(message.at_ms || Date.now());
    const day = when.toDateString();
    if (day !== lastDay) {
      lastDay = day;
      const today = new Date().toDateString() === day;
      nodes.push(el("p", { class: "day", text: today ? "Dzisiaj" : dayFormat.format(when) }));
    }
    const fresh = !firstRender && !shown.has(message.id);
    shown.add(message.id);
    const classes = ["bubble", message.mine ? "mine" : "theirs"];
    if (!message.file && onlyEmoji(message.body)) classes.push("big-emoji");
    if (fresh) classes.push("fresh");

    const meta = el("span", { class: "meta" }, el("span", { text: timeFormat.format(when) }));
    if (message.mine) {
      const status = STATUS[message.status] || STATUS.queued;
      // The newest message also says its status in words.
      if (index === lastMine) meta.prepend(el("span", { text: status.label }));
      const tick = el("span", {
        class: `tick ${message.status}`,
        title: status.label,
        role: "img",
        "aria-label": status.label,
      });
      tick.innerHTML = status.svg;
      meta.append(tick);
    }
    const content = message.file ? fileCard(message, alias) : el("div", { class: "body", text: message.body });
    nodes.push(el("div", { class: classes.join(" ") }, content, meta));
  });

  if (messages.length === 0) {
    nodes.push(el("p", { class: "empty", text: "Napisz pierwszą wiadomość albo przeciągnij tu pliki. Rozmowa trwa, dopóki Szept jest otwarty." }));
  }
  box.replaceChildren(...nodes);
  if (firstRender || nearBottom) box.scrollTop = box.scrollHeight;
  firstRender = false;
}

let pending = false;
listen("changed", (event) => {
  const change = event.payload || {};
  if (change.peer && change.peer !== peer) return;
  if (document.hasFocus()) {
    if (change.kind === "incoming") sounds.incoming();
    if (change.kind === "sent" || change.kind === "saved") sounds.sent();
  }
  if (pending) return;
  pending = true;
  requestAnimationFrame(() => {
    pending = false;
    refresh();
  });
});

listen("problem", (event) => {
  if (event.payload?.peer === peer) showProblem(event.payload.text);
});

$("attach").addEventListener("click", () => {
  showProblem("");
  invoke("send_files", { peer }).catch((err) => showProblem(friendly(err)));
});

// Writing
const draft = $("draft");

function fitDraft() {
  draft.style.height = "auto";
  draft.style.height = `${Math.min(draft.scrollHeight, 144)}px`;
  $("send").disabled = draft.value.trim() === "";
}

draft.addEventListener("input", fitDraft);
draft.addEventListener("keydown", (event) => {
  if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
    event.preventDefault();
    $("composer").requestSubmit();
  }
  if (event.key === "Escape") closeEmoji();
});

$("composer").addEventListener("submit", async (event) => {
  event.preventDefault();
  const body = draft.value.trim();
  if (!body) return;
  try {
    await invoke("send", { peer, body });
    draft.value = "";
    fitDraft();
    closeEmoji();
  } catch (err) {
    $("presence").textContent = friendly(err);
  }
  draft.focus();
});

// Emoji
const panel = $("emoji-panel");
const tabs = $("emoji-tabs");
const grid = $("emoji-grid");
let activeTab = 0;

function categories() {
  const recent = recentEmoji();
  const list = recent.length ? [{ name: "Ostatnio używane", icon: "🕘", items: recent }] : [];
  return list.concat(EMOJI.map((group) => ({ ...group, items: group.list.split(" ") })));
}

function renderEmoji() {
  const groups = categories();
  activeTab = Math.min(activeTab, groups.length - 1);
  tabs.replaceChildren(
    ...groups.map((group, index) =>
      el("button", {
        type: "button",
        role: "tab",
        title: group.name,
        "aria-label": group.name,
        "aria-selected": String(index === activeTab),
        text: group.icon,
        onclick: () => {
          activeTab = index;
          renderEmoji();
        },
      }),
    ),
  );
  grid.setAttribute("aria-label", groups[activeTab].name);
  grid.replaceChildren(
    ...groups[activeTab].items.map((emoji) =>
      el("button", { type: "button", text: emoji, "aria-label": emoji, onclick: () => insertEmoji(emoji) }),
    ),
  );
  grid.scrollTop = 0;
}

function insertEmoji(emoji) {
  const start = draft.selectionStart ?? draft.value.length;
  const end = draft.selectionEnd ?? draft.value.length;
  draft.setRangeText(emoji, start, end, "end");
  rememberEmoji(emoji);
  fitDraft();
  draft.focus();
}

function openEmoji() {
  renderEmoji();
  panel.hidden = false;
  $("emoji-toggle").setAttribute("aria-expanded", "true");
}

function closeEmoji() {
  panel.hidden = true;
  $("emoji-toggle").setAttribute("aria-expanded", "false");
}

$("emoji-toggle").addEventListener("click", () => (panel.hidden ? openEmoji() : closeEmoji()));
document.addEventListener("click", (event) => {
  if (!panel.hidden && !$("composer").contains(event.target)) closeEmoji();
});

// The system's own picker works in the message field too.
const platform = navigator.userAgent;
const shortcut = /Windows/.test(platform)
  ? "Win + . otwiera systemowy wybór emoji."
  : /Mac OS X|Macintosh/.test(platform)
    ? "Ctrl + Cmd + Spacja otwiera systemowy wybór emoji."
    : "";
if (shortcut) {
  $("emoji-hint").textContent = shortcut;
  $("emoji-hint").hidden = false;
}

window.addEventListener("focus", () => invoke("mark_read", { peer }).catch(() => {}));

refresh();
draft.focus();
