// Szept in the person's language. Texts in the code are Polish, wrapped in
// t(…); i18n-data.js (made by tools/i18n.py) maps them to the chosen
// language. Loaded first, before anything that shows text.
"use strict";

const LANGUAGE_KEY = "szept.language";

// "system", or a code from I18N_LANGUAGES.
function languageChoice() {
  try {
    return localStorage.getItem(LANGUAGE_KEY) || "system";
  } catch {
    return "system";
  }
}

// The language to use: the choice, else the system's first one Szept
// speaks, else English.
function resolveLanguage(choice) {
  const codes = I18N_LANGUAGES.map((language) => language.code);
  if (codes.includes(choice)) return choice;
  for (const tag of navigator.languages?.length ? navigator.languages : [navigator.language || ""]) {
    const base = tag.toLowerCase().split("-")[0];
    if (codes.includes(base)) return base;
  }
  return "en";
}

const LANG = resolveLanguage(languageChoice());
const LANG_INFO = I18N_LANGUAGES.find((language) => language.code === LANG);
const STRINGS = I18N[LANG] || {};
// For dates and numbers.
const LOCALE = { zh: "zh-CN", pt: "pt-BR" }[LANG] || LANG;

// `text` in the chosen language, with each {name} replaced from `args`.
function t(text, args) {
  let out = STRINGS[text] ?? text;
  if (args) out = out.replace(/\{(\w+)\}/g, (whole, name) => (name in args ? String(args[name]) : whole));
  return out;
}

// Marks a text to translate later, where it is shown.
function N_(text) {
  return text;
}

document.documentElement.lang = LANG;
document.documentElement.dir = LANG_INFO?.rtl ? "rtl" : "ltr";

// Translates the texts written in the HTML pages: text nodes and the
// attributes people read. Texts already translated are not keys, so a
// second pass changes nothing.
function translatePage(root) {
  if (LANG === "pl") return;
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const key = node.nodeValue.replace(/\s+/g, " ").trim();
    if (key && STRINGS[key]) {
      const lead = node.nodeValue.match(/^\s*/)[0];
      const tail = node.nodeValue.match(/\s*$/)[0];
      node.nodeValue = lead + STRINGS[key] + tail;
    }
  }
  for (const element of root.querySelectorAll("[aria-label], [title], [placeholder]")) {
    for (const name of ["aria-label", "title", "placeholder"]) {
      const value = element.getAttribute(name);
      const key = value?.replace(/\s+/g, " ").trim();
      if (key && STRINGS[key]) element.setAttribute(name, STRINGS[key]);
    }
  }
  const title = document.title.replace(/\s+/g, " ").trim();
  if (STRINGS[title]) document.title = STRINGS[title];
}

document.addEventListener("DOMContentLoaded", () => translatePage(document.body));

// Every window starts again in the new language.
window.__TAURI__?.event.listen("language", () => location.reload());

async function chooseLanguage(choice) {
  try {
    localStorage.setItem(LANGUAGE_KEY, choice);
  } catch {
    // Without storage the choice lasts until the app closes.
  }
  await window.__TAURI__?.core.invoke("set_language", { language: resolveLanguage(choice) });
  await window.__TAURI__?.event.emit("language");
}
