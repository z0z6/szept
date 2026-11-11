// Shared by the contact list and the conversation windows.
"use strict";

const tauri = window.__TAURI__;
const invoke = tauri.core.invoke;
const listen = tauri.event.listen;

const SOUNDS_KEY = "szept.sounds";

function soundsOn() {
  try {
    return localStorage.getItem(SOUNDS_KEY) !== "off";
  } catch {
    return true;
  }
}

function setSounds(on) {
  try {
    localStorage.setItem(SOUNDS_KEY, on ? "on" : "off");
  } catch {
    // Storage may be unavailable; the setting then lasts for this window.
  }
}

// Short, quiet tones made on the spot, so the app ships no sound files.
let audio = null;

function tone(notes) {
  if (!soundsOn()) return;
  try {
    audio = audio || new AudioContext();
    const start = audio.currentTime + 0.01;
    for (const [frequency, offset, length, volume] of notes) {
      const osc = audio.createOscillator();
      const gain = audio.createGain();
      osc.type = "sine";
      osc.frequency.value = frequency;
      gain.gain.setValueAtTime(0, start + offset);
      gain.gain.linearRampToValueAtTime(volume, start + offset + 0.012);
      gain.gain.exponentialRampToValueAtTime(0.0001, start + offset + length);
      osc.connect(gain).connect(audio.destination);
      osc.start(start + offset);
      osc.stop(start + offset + length + 0.02);
    }
  } catch {
    // No audio device: stay silent.
  }
}

const sounds = {
  incoming: () => tone([
    [660, 0, 0.18, 0.07],
    [880, 0.11, 0.24, 0.06],
  ]),
  sent: () => tone([[1046, 0, 0.09, 0.035]]),
};

// The same bubble as the app icon.
const LOGO = `<svg viewBox="22 22 64 64" aria-hidden="true">
<rect x="22" y="22" width="64" height="64" rx="18" fill="#2F3A6B"/>
<path fill="#F4F1EA" fill-rule="evenodd" d="M41,31 H67 A13,13 0 0 1 80,44 V56 A13,13 0 0 1 67,69 H52 C48,73 42,77 35,79 C39,76 43,73 44,69 H41 A13,13 0 0 1 28,56 V44 A13,13 0 0 1 41,31 Z M38.9,50 a4.6,4.6 0 1 0 9.2,0 a4.6,4.6 0 1 0 -9.2,0 Z M52.6,50 a3.4,3.4 0 1 0 6.8,0 a3.4,3.4 0 1 0 -6.8,0 Z M63.9,50 a2.3,2.3 0 1 0 4.6,0 a2.3,2.3 0 1 0 -4.6,0 Z"/>
</svg>`;

function el(tag, props = {}, ...children) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    if (value === undefined || value === null || value === false) continue;
    if (key === "class") node.className = value;
    else if (key === "text") node.textContent = value;
    else if (key.startsWith("on")) node.addEventListener(key.slice(2), value);
    else node.setAttribute(key, value === true ? "" : value);
  }
  for (const child of children) {
    if (child !== null && child !== undefined) node.append(child);
  }
  return node;
}

function errorText(err) {
  return typeof err === "string" ? err : err?.message || String(err);
}

// Rust reports errors in English: translate the ones people actually meet.
const ERRORS = [
  [/alias must be a single word/, "Nazwa musi być jednym słowem, bez spacji."],
  [/alias `(.+)` is already taken/, "Masz już kontakt o nazwie $1."],
  [/already saved as `(.+)`/, "Ta osoba jest już w kontaktach jako $1."],
  [/this is your own identifier/, "To Twój własny identyfikator."],
  [/not a valid identifier|identifier is empty/, "To nie jest identyfikator Szeptu."],
  [/message is too long/, "Wiadomość jest za długa."],
  [/app is offline|shutting down/, "Szept nie jest połączony z siecią."],
  [/no longer shares this file|offer is no longer known/, "Nadawca już nie udostępnia tego pliku."],
  [/changed or disappeared/, "Plik zmienił się albo zniknął u nadawcy."],
  [/arrived damaged/, "Plik dotarł uszkodzony i został usunięty. Spróbuj ponownie."],
  [/did not answer in time|cannot connect|connection lost|stalled|cut short/, "Nadawca jest niedostępny. Pobieranie wznowi się od miejsca przerwania."],
];

function friendly(err) {
  const text = errorText(err);
  for (const [pattern, polish] of ERRORS) {
    if (pattern.test(text)) return text.replace(new RegExp(`^.*?(?:${pattern.source}).*$`, "s"), polish);
  }
  return text;
}
