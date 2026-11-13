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

// Only one voice message plays at a time.
let playing = null;

function voicePlayer(message) {
  const file = message.file;
  const key = `voice:${message.id}:${file.path}`;
  let node = mediaCache.get(key);
  if (node) return node;

  const total = file.duration_ms || 0;
  const audio = new Audio(tauri.core.convertFileSrc(file.path));
  audio.preload = "metadata";
  const button = el("button", { class: "voice-play", type: "button", "aria-label": "Odtwórz" });
  const seek = el("input", {
    class: "voice-seek",
    type: "range",
    min: "0",
    max: "1000",
    value: "0",
    "aria-label": "Postęp nagrania",
  });
  const time = el("span", { class: "voice-time", text: formatDuration(total) });

  // Recordings from a browser often carry no length; ours travels along.
  const length = () => (Number.isFinite(audio.duration) && audio.duration > 0 ? audio.duration * 1000 : total);
  const draw = () => {
    const done = audio.currentTime * 1000;
    const whole = length();
    seek.value = String(whole ? Math.min(1000, Math.round((done / whole) * 1000)) : 0);
    time.textContent = audio.paused && done === 0 ? formatDuration(whole) : formatDuration(done);
    const on = !audio.paused;
    button.classList.toggle("on", on);
    button.setAttribute("aria-label", on ? "Pauza" : "Odtwórz");
  };
  button.addEventListener("click", () => {
    if (audio.paused) {
      if (playing && playing !== audio) playing.pause();
      playing = audio;
      audio.play().catch(() => showProblem("Nie udało się odtworzyć nagrania."));
    } else {
      audio.pause();
    }
  });
  seek.addEventListener("input", () => {
    const whole = length();
    if (whole) audio.currentTime = (Number(seek.value) / 1000) * (whole / 1000);
  });
  for (const name of ["timeupdate", "play", "pause", "loadedmetadata"]) audio.addEventListener(name, draw);
  audio.addEventListener("ended", () => {
    audio.currentTime = 0;
    draw();
  });
  draw();

  node = el("div", { class: "voice" }, button, seek, time);
  mediaCache.set(key, node);
  return node;
}

// A voice message or a GIF: fetched on its own, shown without a file card.
function mediaContent(message) {
  const file = message.file;
  const local = file.path && (file.transfer === "saved" || file.transfer === "shared");
  if (local) {
    if (file.media === "voice") return voicePlayer(message);
    const key = `gif:${message.id}:${file.path}`;
    let node = mediaCache.get(key);
    if (!node) {
      node = el("img", {
        class: "gif",
        src: tauri.core.convertFileSrc(file.path),
        alt: "GIF",
        draggable: "false",
      });
      mediaCache.set(key, node);
    }
    return node;
  }
  const what = file.media === "voice" ? `Wiadomość głosowa, ${formatDuration(file.duration_ms || 0)}` : "GIF";
  const box = el("div", { class: "media-wait" }, el("span", { text: what }));
  if (file.transfer === "failed") {
    box.append(
      el("span", { class: "file-error", text: "Nie udało się pobrać." }),
      el("button", {
        class: "button",
        type: "button",
        text: "Pobierz ponownie",
        onclick: () => act("download", message.id),
      }),
    );
  } else {
    box.append(el("span", { class: "file-meta", text: "Pobieranie…" }));
    if (file.transfer === "downloading" && file.size) {
      box.append(el("progress", { max: String(file.size), value: String(file.done) }));
    }
  }
  return box;
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
    $("record").disabled = true;
    return;
  }
  chatData = data;
  document.title = `${data.alias} – Szept`;
  $("alias").textContent = data.alias;
  if (data.group) {
    $("avatar").textContent = [...data.alias][0] || "#";
    $("avatar").classList.add("group-avatar");
    const online = data.members.filter((member) => member.online).length;
    $("presence").textContent = `${data.members.length} osób, połączonych: ${online}`;
    $("members-open").hidden = false;
  } else {
    fillAvatar($("avatar"), data.alias, data.avatar);
    $("presence").textContent = data.online
      ? "Połączony"
      : "Niepołączony. Wiadomości wyjdą, gdy kontakt się pojawi.";
  }
  $("avatar").dataset.online = String(data.online);
  renderMessages(data.messages, data.alias);
}

let chatData = null;

// Who someone is in this conversation.
function personName(id) {
  if (!chatData) return "";
  if (id === chatData.my_id) return "Ty";
  if (!chatData.group) return chatData.alias;
  return chatData.members.find((member) => member.id === id)?.name ?? "Ktoś spoza grupy";
}

function authorAvatar(id) {
  const member = chatData?.members.find((person) => person.id === id);
  const node = el("span", { class: "avatar tiny", "aria-hidden": "true" });
  fillAvatar(node, member?.name ?? "?", member?.avatar);
  return node;
}

// Reactions under a message: one chip per kind, with who chose it.
function reactionRow(message) {
  const counts = new Map();
  for (const [who, code] of Object.entries(message.reactions || {})) {
    if (!FIGURES[code]) continue;
    if (!counts.has(code)) counts.set(code, []);
    counts.get(code).push(who);
  }
  const mine = message.reactions?.[chatData?.my_id];
  const chips = [...counts.entries()].map(([code, people]) => {
    const names = people.map(personName).join(", ");
    const svg = figureSvg(code, 22, { animate: false });
    const chip = el(
      "button",
      {
        class: `reaction${code === mine ? " own" : ""}`,
        type: "button",
        title: `${figureName(code)}: ${names}`,
        "aria-label": `${figureName(code)}, ${people.length}: ${names}`,
        onclick: () => react(message.id, code === mine ? "" : code),
      },
      svg,
      people.length > 1 ? el("span", { text: String(people.length) }) : "",
    );
    chip.addEventListener("mouseenter", () => svg.unpauseAnimations?.());
    chip.addEventListener("mouseleave", () => svg.pauseAnimations?.());
    return chip;
  });
  const add = el("button", {
    class: "react-add",
    type: "button",
    title: "Zareaguj",
    "aria-label": "Zareaguj na wiadomość",
    text: "＋",
    onclick: (event) => {
      event.stopPropagation();
      openPalette(add, message.id, mine);
    },
  });
  return el("div", { class: "reactions" }, ...chips, add);
}

async function react(id, reaction) {
  try {
    await invoke("react", { peer, id, reaction });
  } catch (err) {
    showProblem(friendly(err));
  }
}

// The palette of Szeptusie, next to the message.
const palette = el("div", { class: "palette", role: "menu", hidden: "" });
document.body.append(palette);

function openPalette(anchor, id, mine) {
  palette.replaceChildren(
    ...REACTIONS.map((code) =>
      el(
        "button",
        {
          class: `palette-item${code === mine ? " own" : ""}`,
          type: "button",
          role: "menuitem",
          title: figureName(code),
          "aria-label": figureName(code),
          onclick: () => {
            closePalette();
            react(id, code === mine ? "" : code);
          },
        },
        figureSvg(code, 36),
      ),
    ),
  );
  palette.hidden = false;
  const box = anchor.getBoundingClientRect();
  const width = palette.offsetWidth;
  const left = Math.max(8, Math.min(window.innerWidth - width - 8, box.left - width / 2));
  const top = box.top - palette.offsetHeight - 8;
  palette.style.left = `${left}px`;
  palette.style.top = `${top < 8 ? box.bottom + 8 : top}px`;
  palette.querySelector("button")?.focus();
}

function closePalette() {
  palette.hidden = true;
}

document.addEventListener("click", (event) => {
  if (!palette.hidden && !palette.contains(event.target)) closePalette();
});
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && !palette.hidden) closePalette();
});

function stickerContent(message) {
  const key = `sticker:${message.id}`;
  let node = mediaCache.get(key);
  if (!node) {
    const label = `Naklejka: ${stickerCaption(message.sticker)}`;
    node = el(
      "figure",
      { class: "sticker" },
      figureSvg(message.sticker, 128, { label }),
      el("figcaption", { text: stickerCaption(message.sticker) }),
    );
    mediaCache.set(key, node);
  }
  return node;
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
    if (message.system) {
      nodes.push(el("p", { class: "note", text: message.body }));
      return;
    }
    const classes = ["bubble", message.mine ? "mine" : "theirs"];
    if (message.sticker) classes.push("sticker-bubble");
    if (!message.file && onlyEmoji(message.body)) classes.push("big-emoji");
    if (message.file?.media) classes.push(`media-${message.file.media}`);
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
    const content = message.sticker
      ? stickerContent(message)
      : message.file?.media
        ? mediaContent(message)
        : message.file
          ? fileCard(message, alias)
          : el("div", { class: "body", text: message.body });
    const bubble = el("div", { class: classes.join(" ") });
    if (chatData?.group && !message.mine && message.author) {
      bubble.append(el("span", { class: "author", text: personName(message.author) }));
    }
    bubble.append(content, meta);
    const row = el("div", { class: `row ${message.mine ? "mine" : "theirs"}` });
    if (chatData?.group && !message.mine) row.append(authorAvatar(message.author));
    row.append(el("div", { class: "stack" }, bubble, reactionRow(message)));
    nodes.push(row);
  });

  if (messages.length === 0) {
    nodes.push(el("p", { class: "empty", text: "Napisz pierwszą wiadomość albo przeciągnij tu pliki. Rozmowa trwa, dopóki Szept jest otwarty." }));
  }
  box.replaceChildren(...nodes);
  if (firstRender || nearBottom) box.scrollTop = box.scrollHeight;
  firstRender = false;
}

// Stickers
const stickerPanel = $("sticker-panel");
$("sticker-toggle").append(figureSvg("pierog", 24, { animate: false }));
$("sticker-grid").append(
  ...STICKERS.map((code) =>
    el(
      "button",
      {
        class: "sticker-choice",
        type: "button",
        title: stickerCaption(code),
        "aria-label": `Wyślij naklejkę: ${stickerCaption(code)}`,
        onclick: () => sendSticker(code),
      },
      figureSvg(code, 64),
      el("span", { text: stickerCaption(code) }),
    ),
  ),
);

async function sendSticker(code) {
  stickerPanel.hidden = true;
  $("sticker-toggle").setAttribute("aria-expanded", "false");
  try {
    await invoke("send_sticker", { peer, code });
    showProblem("");
  } catch (err) {
    showProblem(friendly(err));
  }
}

$("sticker-toggle").addEventListener("click", (event) => {
  event.stopPropagation();
  const open = stickerPanel.hidden;
  closeEmoji();
  stickerPanel.hidden = !open;
  $("sticker-toggle").setAttribute("aria-expanded", String(open));
});
document.addEventListener("click", (event) => {
  if (!stickerPanel.hidden && !stickerPanel.contains(event.target)) {
    stickerPanel.hidden = true;
    $("sticker-toggle").setAttribute("aria-expanded", "false");
  }
});

// Group members and settings
const membersDialog = $("members-dialog");

for (const button of document.querySelectorAll("[data-close]")) {
  button.addEventListener("click", () => button.closest("dialog").close());
}

$("members-open").addEventListener("click", async () => {
  if (!chatData?.group) return;
  $("members-error").hidden = true;
  const owner = chatData.mine;
  $("group-name").value = chatData.alias;
  $("group-name").readOnly = !owner;
  $("members-save").hidden = !owner;
  $("leave-group").textContent = owner ? "Zamknij grupę" : "Opuść grupę";
  $("members-hint").textContent = owner
    ? "Możesz dodać swoje kontakty albo odznaczyć osoby, które mają wyjść z grupy."
    : "Członków zmienia właściciel grupy.";
  let contacts = [];
  if (owner) {
    try {
      contacts = (await invoke("overview")).contacts;
    } catch {
      contacts = [];
    }
  }
  const rows = chatData.members
    .filter((member) => member.id !== chatData.my_id)
    .map((member) => ({ id: member.id, name: member.name, inside: true, online: member.online }));
  for (const contact of contacts) {
    if (!rows.some((row) => row.id === contact.id)) {
      rows.push({ id: contact.id, name: contact.alias, inside: false, online: contact.online });
    }
  }
  $("member-list").replaceChildren(
    el("p", { class: "member-me", text: owner ? "Ty (właściciel)" : "Ty" }),
    ...rows.map((row) =>
      owner
        ? el(
            "label",
            { class: "check" },
            el("input", { type: "checkbox", value: row.id, ...(row.inside ? { checked: "" } : {}) }),
            el("span", { text: `${row.name}${row.online ? " · połączony" : ""}` }),
          )
        : el("p", { text: `${row.name}${row.online ? " · połączony" : ""}` }),
    ),
  );
  membersDialog.showModal();
});

$("members-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const members = [...$("member-list").querySelectorAll("input:checked")].map((input) => input.value);
  try {
    await invoke("update_group", { group: peer, name: $("group-name").value.trim(), members });
    membersDialog.close();
  } catch (err) {
    $("members-error").textContent = friendly(err);
    $("members-error").hidden = false;
  }
});

$("leave-group").addEventListener("click", async () => {
  const question = chatData?.mine
    ? "Zamknąć grupę dla wszystkich? Rozmowa zniknie."
    : "Opuścić grupę? Rozmowa zniknie z tego komputera.";
  if (!confirm(question)) return;
  try {
    await invoke("leave_group", { group: peer });
  } catch (err) {
    $("members-error").textContent = friendly(err);
    $("members-error").hidden = false;
  }
});

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
  // An empty field offers the microphone instead of the send button.
  const empty = draft.value.trim() === "";
  $("send").hidden = empty;
  $("record").hidden = !empty;
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
  ? "Win + . otwiera systemowy wybór emoji i GIF-ów."
  : /Mac OS X|Macintosh/.test(platform)
    ? "Ctrl + Cmd + Spacja otwiera systemowy wybór emoji."
    : "";
if (shortcut) {
  $("emoji-hint").textContent = shortcut;
  $("emoji-hint").hidden = false;
}

// GIFs and stickers pasted into the message field, for example from the
// GIF tab of the Windows emoji panel (Win + .), go out as GIFs.
const GIF_TYPES = { "image/gif": "gif", "image/webp": "webp", "image/png": "png", "image/jpeg": "jpg" };

draft.addEventListener("paste", (event) => {
  const pictures = [...(event.clipboardData?.files || [])].filter((file) => GIF_TYPES[file.type]);
  if (!pictures.length) return;
  event.preventDefault();
  for (const picture of pictures) sendGif(picture);
});

async function sendGif(picture) {
  if (picture.size > MAX_MEDIA_SIZE) {
    showProblem("Ten obraz jest za duży na GIF. Wyślij go spinaczem.");
    return;
  }
  try {
    const bytes = new Uint8Array(await picture.arrayBuffer());
    await invoke("send_media", bytes, {
      headers: { peer, media: "gif", extension: GIF_TYPES[picture.type] },
    });
    showProblem("");
  } catch (err) {
    showProblem(friendly(err));
  }
}

// Voice messages
const MAX_VOICE_MS = 10 * 60 * 1000;
// Tried in order; each system's web view records at least one of them.
const VOICE_TYPES = ["audio/ogg;codecs=opus", "audio/webm;codecs=opus", "audio/mp4", "audio/webm"];
let recording = null;

function voiceType() {
  if (!window.MediaRecorder) return null;
  if (typeof MediaRecorder.isTypeSupported !== "function") return "";
  return VOICE_TYPES.find((type) => MediaRecorder.isTypeSupported(type)) ?? null;
}

function extensionFor(mime) {
  if (/ogg/.test(mime)) return "ogg";
  if (/webm/.test(mime)) return "webm";
  if (/mp4|aac|m4a/.test(mime)) return "m4a";
  return "webm";
}

$("record").addEventListener("click", startRecording);
$("rec-cancel").addEventListener("click", () => stopRecording(false));
$("rec-send").addEventListener("click", () => stopRecording(true));

async function startRecording() {
  if (recording) return;
  showProblem("");
  const type = voiceType();
  if (type === null || !navigator.mediaDevices?.getUserMedia) {
    showProblem("Ten system nie pozwala tu nagrywać dźwięku.");
    return;
  }
  let stream;
  try {
    stream = await navigator.mediaDevices.getUserMedia({
      audio: { echoCancellation: true, noiseSuppression: true },
      video: false,
    });
  } catch (err) {
    showProblem(err?.name === "NotFoundError"
      ? "Nie znaleziono mikrofonu."
      : "Brak dostępu do mikrofonu. Zezwól na niego w ustawieniach systemu.");
    return;
  }
  let recorder;
  try {
    recorder = new MediaRecorder(stream, type ? { mimeType: type, audioBitsPerSecond: 32000 } : {});
  } catch {
    for (const track of stream.getTracks()) track.stop();
    showProblem("Ten system nie pozwala tu nagrywać dźwięku.");
    return;
  }
  const chunks = [];
  recording = { recorder, stream, chunks, started: performance.now(), send: false, timer: 0 };
  recorder.addEventListener("dataavailable", (event) => {
    if (event.data.size) chunks.push(event.data);
  });
  recorder.addEventListener("stop", finishRecording);
  recorder.start(500);

  closeEmoji();
  $("composer").classList.add("recording");
  $("recorder").hidden = false;
  $("rec-time").textContent = "0:00";
  recording.timer = setInterval(() => {
    const elapsed = performance.now() - recording.started;
    $("rec-time").textContent = formatDuration(elapsed);
    if (elapsed >= MAX_VOICE_MS) stopRecording(true);
  }, 250);
  $("rec-send").focus();
}

function stopRecording(send) {
  if (!recording || recording.recorder.state === "inactive") return;
  recording.send = send;
  recording.duration = Math.round(performance.now() - recording.started);
  clearInterval(recording.timer);
  recording.recorder.stop();
}

async function finishRecording() {
  const { recorder, stream, chunks, send, duration } = recording;
  recording = null;
  for (const track of stream.getTracks()) track.stop();
  $("composer").classList.remove("recording");
  $("recorder").hidden = true;
  draft.focus();
  // Shorter than half a second is a slip of the finger.
  if (!send || duration < 500 || !chunks.length) return;
  const mime = recorder.mimeType || chunks[0].type || "";
  const blob = new Blob(chunks, { type: mime });
  if (blob.size > MAX_MEDIA_SIZE) {
    showProblem("Nagranie jest za długie.");
    return;
  }
  try {
    const bytes = new Uint8Array(await blob.arrayBuffer());
    await invoke("send_media", bytes, {
      headers: { peer, media: "voice", extension: extensionFor(mime), duration: String(duration) },
    });
  } catch (err) {
    showProblem(friendly(err));
  }
}

document.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && recording) stopRecording(false);
});

window.addEventListener("focus", () => invoke("mark_read", { peer }).catch(() => {}));

refresh();
fitDraft();
draft.focus();
