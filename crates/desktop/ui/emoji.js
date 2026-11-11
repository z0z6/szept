// Emoji offered by the picker. They are plain Unicode text, drawn by the
// system's own emoji font, so they look the way the person is used to and
// arrive unchanged on every other device.
"use strict";

const EMOJI = [
  {
    name: "Buźki",
    icon: "😀",
    list: "😀 😃 😄 😁 😆 😅 🤣 😂 🙂 🙃 😉 😊 😇 🥰 😍 🤩 😘 😗 😚 😙 🥲 😋 😛 😜 🤪 😝 🤗 🤭 🤫 🤔 🤐 🤨 😐 😑 😶 😏 😒 🙄 😬 😮‍💨 🤥 😌 😔 😪 🤤 😴 😷 🤒 🤕 🤢 🤮 🥵 🥶 🥴 😵 🤯 🤠 🥳 😎 🤓 🧐 😕 😟 🙁 😮 😯 😲 😳 🥺 😦 😧 😨 😰 😥 😢 😭 😱 😖 😣 😞 😓 😩 😫 🥱 😤 😡 😠 🤬 😈 💀 🤡 👻 👽 🤖 💩",
  },
  {
    name: "Gesty",
    icon: "👋",
    list: "👋 🤚 ✋ 🖖 👌 🤌 🤏 ✌️ 🤞 🤟 🤘 🤙 👈 👉 👆 👇 ☝️ 👍 👎 ✊ 👊 🤛 🤜 👏 🙌 👐 🤲 🤝 🙏 ✍️ 💪 🫶 🙋 🙆 🙅 🤷 🤦 🙇",
  },
  {
    name: "Serca",
    icon: "❤️",
    list: "❤️ 🧡 💛 💚 💙 💜 🤎 🖤 🤍 💔 ❤️‍🔥 ❤️‍🩹 💕 💞 💓 💗 💖 💘 💝 💟 💋 💌 💐 🌹",
  },
  {
    name: "Natura",
    icon: "🐶",
    list: "🐶 🐱 🐭 🐹 🐰 🦊 🐻 🐼 🐨 🐯 🦁 🐮 🐷 🐸 🐵 🐔 🐧 🐦 🦆 🦉 🐺 🐴 🦄 🐝 🦋 🐌 🐞 🐢 🐍 🐙 🐬 🐳 🐟 🦀 🌵 🌲 🌳 🌴 🌱 🍀 🍁 🍂 🌸 🌼 🌻 🌞 🌝 🌙 ⭐ 🌈 ☀️ ⛅ 🌧️ ⛈️ ❄️ ☃️ 🔥 💧 🌊",
  },
  {
    name: "Jedzenie",
    icon: "🍕",
    list: "🍏 🍎 🍐 🍊 🍋 🍌 🍉 🍇 🍓 🫐 🍒 🍑 🥭 🍍 🥥 🥝 🍅 🥑 🥦 🥕 🌽 🥔 🥐 🍞 🧀 🥚 🍳 🥓 🍔 🍟 🍕 🌭 🥪 🌮 🍝 🍜 🍲 🍣 🥟 🍰 🎂 🍩 🍪 🍫 🍬 🍭 🍦 ☕ 🍵 🧃 🍺 🍻 🥂 🍷",
  },
  {
    name: "Aktywności",
    icon: "⚽",
    list: "⚽ 🏀 🏈 ⚾ 🎾 🏐 🏉 🎱 🏓 🏸 🥅 ⛳ 🎣 🎿 ⛸️ 🏂 🏋️ 🚴 🏊 🧘 🎯 🎮 🎲 🧩 🎨 🎬 🎤 🎧 🎸 🎹 🥁 🎉 🎊 🎈 🎁 🏆 🥇 🥈 🥉",
  },
  {
    name: "Podróże",
    icon: "🚗",
    list: "🚗 🚕 🚌 🚎 🏎️ 🚓 🚑 🚒 🚲 🛴 🏍️ 🚂 🚆 🚇 🚊 ✈️ 🛫 🛬 🚀 🛸 🚁 ⛵ 🚤 🚢 ⚓ 🗺️ 🧭 🏔️ ⛰️ 🏕️ 🏖️ 🏝️ 🏠 🏡 🏢 🏰 🗼 🗽 ⛪ 🌉 🌃 🎡",
  },
  {
    name: "Przedmioty",
    icon: "💡",
    list: "⌚ 📱 💻 ⌨️ 🖥️ 🖨️ 📷 🎥 📺 📻 ⏰ ⌛ 🔋 🔌 💡 🔦 🕯️ 💸 💳 💎 🔧 🔨 🛠️ 🔑 🗝️ 🔒 🔓 🚪 🛏️ 🛁 🧸 🎀 ✉️ 📦 📝 📌 📎 ✂️ 📚 📖 🔍",
  },
  {
    name: "Symbole",
    icon: "✅",
    list: "✅ ☑️ ✔️ ❌ ❎ ➕ ➖ ❗ ❓ ‼️ ⁉️ 💯 🔞 ⚠️ 🚫 ⛔ ♻️ 💤 💬 💭 🗯️ 🔔 🔕 🎵 🎶 ⬆️ ⬇️ ⬅️ ➡️ 🔄 🆗 🆕 🆒 🆘 ♾️ ✨ ⚡ 💥 🎯",
  },
];

const RECENT_KEY = "szept.recentEmoji";
const RECENT_LIMIT = 24;

function recentEmoji() {
  try {
    const saved = JSON.parse(localStorage.getItem(RECENT_KEY) || "[]");
    return Array.isArray(saved) ? saved.filter((item) => typeof item === "string") : [];
  } catch {
    return [];
  }
}

function rememberEmoji(emoji) {
  const list = [emoji, ...recentEmoji().filter((item) => item !== emoji)].slice(0, RECENT_LIMIT);
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(list));
  } catch {
    // Not remembered; nothing else breaks.
  }
}

// True for a message made of one to three emoji and nothing else.
function onlyEmoji(text) {
  const trimmed = text.trim();
  if (!trimmed || trimmed.length > 40) return false;
  if (!/^[\p{Extended_Pictographic}\p{Emoji_Component}\u200d\ufe0f\s]+$/u.test(trimmed)) return false;
  if (/^[\d#*\s]+$/.test(trimmed)) return false;
  const pictographs = trimmed.match(/\p{Extended_Pictographic}(?:\ufe0f|\p{Emoji_Modifier})*(?:\u200d\p{Extended_Pictographic}(?:\ufe0f|\p{Emoji_Modifier})*)*/gu);
  return !!pictographs && pictographs.length <= 3;
}
