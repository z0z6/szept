// Colour themes. Loaded in <head>, before anything is drawn, so a window
// never flashes in the wrong colours. The Android app has the same ten.
"use strict";

// Each theme has a light and a dark variant. The keys are the CSS custom
// properties of style.css, without the leading "--".
const THEMES = [
  {
    id: "szept",
    name: "Szept",
    light: { mist: "#f3f4f8", surface: "#ffffff", ink: "#1b2140", "ink-soft": "#5a6183", line: "#dde0eb", accent: "#2f3a6b", "on-accent": "#ffffff", mine: "#e3e6f4", focus: "#5563b8" },
    dark: { mist: "#12152a", surface: "#1b1f38", ink: "#e7e9f4", "ink-soft": "#9aa0c2", line: "#2c3255", accent: "#9aa6f0", "on-accent": "#12152a", mine: "#2a3163", focus: "#b9c1ff" },
  },
  {
    id: "las",
    name: "Las",
    light: { mist: "#f1f5f1", surface: "#ffffff", ink: "#16261c", "ink-soft": "#54675a", line: "#d8e3da", accent: "#2f6b45", "on-accent": "#ffffff", mine: "#dcebdf", focus: "#4a9466" },
    dark: { mist: "#0f1912", surface: "#17241b", ink: "#e5efe7", "ink-soft": "#98b0a0", line: "#27392d", accent: "#8fd3a6", "on-accent": "#0f1912", mine: "#23402d", focus: "#b2e6c4" },
  },
  {
    id: "morze",
    name: "Morze",
    light: { mist: "#eff6f6", surface: "#ffffff", ink: "#102a2d", "ink-soft": "#4d6a6d", line: "#d3e5e6", accent: "#0f6b73", "on-accent": "#ffffff", mine: "#d6ecee", focus: "#2f959e" },
    dark: { mist: "#0b1a1c", surface: "#122527", ink: "#e2f1f2", "ink-soft": "#8fb3b6", line: "#21393c", accent: "#7dd3db", "on-accent": "#0b1a1c", mine: "#1a3f43", focus: "#a6e6ec" },
  },
  {
    id: "wrzos",
    name: "Wrzos",
    light: { mist: "#f6f2f8", surface: "#ffffff", ink: "#2a1a36", "ink-soft": "#6a5878", line: "#e6dcec", accent: "#6b3a8c", "on-accent": "#ffffff", mine: "#ece0f3", focus: "#9160b5" },
    dark: { mist: "#1a1220", surface: "#241a2c", ink: "#f0e8f5", "ink-soft": "#b5a2c2", line: "#3a2b45", accent: "#d2a8f0", "on-accent": "#1a1220", mine: "#3d2a4e", focus: "#e2c4f7" },
  },
  {
    id: "bursztyn",
    name: "Bursztyn",
    light: { mist: "#faf6ee", surface: "#ffffff", ink: "#2e2310", "ink-soft": "#6e5f45", line: "#ece2cf", accent: "#8a5a00", "on-accent": "#ffffff", mine: "#f5e8cc", focus: "#b07a14" },
    dark: { mist: "#1c170d", surface: "#272013", ink: "#f5eedf", "ink-soft": "#bcae92", line: "#3d3320", accent: "#f2c46b", "on-accent": "#1c170d", mine: "#4a3a17", focus: "#f7d68f" },
  },
  {
    id: "malina",
    name: "Malina",
    light: { mist: "#faf1f4", surface: "#ffffff", ink: "#33141f", "ink-soft": "#74505e", line: "#f0d9e1", accent: "#a3204f", "on-accent": "#ffffff", mine: "#f7dde6", focus: "#cc4675" },
    dark: { mist: "#1e0f15", surface: "#2a161e", ink: "#f7e6ec", "ink-soft": "#c49aab", line: "#452332", accent: "#f598b8", "on-accent": "#1e0f15", mine: "#52223a", focus: "#f9bcd1" },
  },
  {
    id: "grafit",
    name: "Grafit",
    light: { mist: "#f3f3f4", surface: "#ffffff", ink: "#1e1f22", "ink-soft": "#5c5f66", line: "#dcdde0", accent: "#3b3e45", "on-accent": "#ffffff", mine: "#e4e5e8", focus: "#6b7180" },
    dark: { mist: "#121214", surface: "#1c1c1f", ink: "#ececee", "ink-soft": "#a0a2a8", line: "#2f3034", accent: "#c9ccd3", "on-accent": "#121214", mine: "#33353a", focus: "#e0e2e7" },
  },
  {
    id: "koral",
    name: "Koral",
    light: { mist: "#fbf3f0", surface: "#ffffff", ink: "#34190f", "ink-soft": "#765649", line: "#f1ddd5", accent: "#b2431f", "on-accent": "#ffffff", mine: "#f8e0d7", focus: "#d9653d" },
    dark: { mist: "#1f120d", surface: "#2b1a13", ink: "#f8e9e3", "ink-soft": "#c9a294", line: "#47291d", accent: "#ffa384", "on-accent": "#1f120d", mine: "#55291a", focus: "#ffc2ad" },
  },
  {
    id: "lazur",
    name: "Lazur",
    light: { mist: "#f0f5fb", surface: "#ffffff", ink: "#0f2238", "ink-soft": "#4e637b", line: "#d6e2ef", accent: "#1f5fa8", "on-accent": "#ffffff", mine: "#dbe8f7", focus: "#3f82cf" },
    dark: { mist: "#0c1522", surface: "#132030", ink: "#e4eef9", "ink-soft": "#94adc8", line: "#233650", accent: "#8ec2ff", "on-accent": "#0c1522", mine: "#1b3a5e", focus: "#b5d6ff" },
  },
  {
    id: "oliwka",
    name: "Oliwka",
    light: { mist: "#f5f5ee", surface: "#ffffff", ink: "#25260f", "ink-soft": "#63654a", line: "#e3e4d2", accent: "#5b6118", "on-accent": "#ffffff", mine: "#e9ebd2", focus: "#838a2c" },
    dark: { mist: "#16170d", surface: "#202114", ink: "#eeefe0", "ink-soft": "#b1b394", line: "#363822", accent: "#cdd47a", "on-accent": "#16170d", mine: "#3b3e1b", focus: "#dfe59f" },
  },
];

// Status colours stay the same in every theme: people learn them once.
const STATUS_COLORS = {
  light: { ok: "#2e7d6b", danger: "#b3261e" },
  dark: { ok: "#6cc3a9", danger: "#f2b8b5" },
};

const THEME_KEY = "szept.theme";
const MODE_KEY = "szept.mode";
const MODES = ["system", "light", "dark"];

function readSetting(key, fallback) {
  try {
    return localStorage.getItem(key) || fallback;
  } catch {
    return fallback;
  }
}

function writeSetting(key, value) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Storage may be unavailable; the choice then lasts for this window.
  }
}

function currentTheme() {
  const id = readSetting(THEME_KEY, "szept");
  return THEMES.find((theme) => theme.id === id) || THEMES[0];
}

function currentMode() {
  const mode = readSetting(MODE_KEY, "system");
  return MODES.includes(mode) ? mode : "system";
}

const darkQuery = window.matchMedia("(prefers-color-scheme: dark)");

function isDark() {
  const mode = currentMode();
  return mode === "dark" || (mode === "system" && darkQuery.matches);
}

function applyTheme() {
  const dark = isDark();
  const colors = { ...currentTheme()[dark ? "dark" : "light"], ...STATUS_COLORS[dark ? "dark" : "light"] };
  const root = document.documentElement.style;
  for (const [name, value] of Object.entries(colors)) root.setProperty(`--${name}`, value);
  root.setProperty("color-scheme", dark ? "dark" : "light");
}

// Every open window follows a change made in any of them.
function chooseTheme(id) {
  writeSetting(THEME_KEY, id);
  applyTheme();
  window.__TAURI__?.event.emit("theme").catch(() => {});
}

function chooseMode(mode) {
  writeSetting(MODE_KEY, mode);
  applyTheme();
  window.__TAURI__?.event.emit("theme").catch(() => {});
}

applyTheme();
darkQuery.addEventListener("change", applyTheme);
window.addEventListener("storage", (event) => {
  if (event.key === THEME_KEY || event.key === MODE_KEY) applyTheme();
});
window.__TAURI__?.event.listen("theme", applyTheme);
