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
  renderMessages(data.messages);
}

function renderMessages(messages) {
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
    if (onlyEmoji(message.body)) classes.push("big-emoji");
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
    nodes.push(el("div", { class: classes.join(" ") }, el("div", { class: "body", text: message.body }), meta));
  });

  if (messages.length === 0) {
    nodes.push(el("p", { class: "empty", text: "Napisz pierwszą wiadomość. Rozmowa trwa, dopóki Szept jest otwarty." }));
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
    if (change.kind === "sent") sounds.sent();
  }
  if (pending) return;
  pending = true;
  requestAnimationFrame(() => {
    pending = false;
    refresh();
  });
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
