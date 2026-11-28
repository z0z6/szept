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

// Sounds made by tools/sounds.py: the one for new messages is the person's
// choice, the others belong to one event each.
const SOUND_KEY = "szept.sound";

// The sounds' names and descriptions, in the chosen language.
for (const choice of SOUND_CHOICES) {
  choice.name = t(choice.name);
  choice.about = t(choice.about);
}
const PREVIEW_KEY = "szept.notify_preview";

function chosenSound() {
  try {
    const id = localStorage.getItem(SOUND_KEY);
    if (id === "none" || SOUND_CHOICES.some((choice) => choice.id === id)) return id;
  } catch {
    // Fall through to the default.
  }
  return SOUND_CHOICES[0].id;
}

function previewsOn() {
  try {
    return localStorage.getItem(PREVIEW_KEY) !== "off";
  } catch {
    return true;
  }
}

// Stores the choices here and tells the core, which shows the notifications.
async function setNotifySettings(sound, preview) {
  try {
    localStorage.setItem(SOUND_KEY, sound);
    localStorage.setItem(PREVIEW_KEY, preview ? "on" : "off");
  } catch {
    // Storage may be unavailable: the core still keeps them.
  }
  await invoke("set_notify_settings", { sound, preview });
}

// `force` plays even with sounds off, for trying a sound in the settings.
function playSound(id, volume = 0.7, force = false) {
  if (!id || id === "none" || (!force && !soundsOn())) return;
  try {
    const audio = new Audio(`sounds/${id}.wav`);
    audio.volume = volume;
    audio.play().catch(() => {});
  } catch {
    // No audio device: stay silent.
  }
}

const sounds = {
  incoming: () => playSound(chosenSound()),
  sent: () => playSound("fiu", 0.35),
  payment: () => playSound("kaching"),
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

// Mirrors MAX_MEDIA_SIZE and MAX_AVATAR_LEN in crates/core/src/protocol.rs.
const MAX_MEDIA_SIZE = 16 * 1024 * 1024;
const MAX_AVATAR_LEN = 48 * 1024;

const PERSON_SVG = '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="8.5" r="4" fill="currentColor"/><path d="M4 20.5c.8-4 4-6.2 8-6.2s7.2 2.2 8 6.2" fill="currentColor"/></svg>';

// Shows a profile picture in an avatar circle, or the first letter of the
// name when there is none. Without a name, a person outline.
function fillAvatar(node, name, url) {
  if (url) {
    node.replaceChildren(el("img", { src: url, alt: "", draggable: "false" }));
  } else if (name) {
    node.textContent = [...name][0] || "?";
  } else {
    node.innerHTML = PERSON_SVG;
  }
  node.classList.toggle("has-picture", Boolean(url));
}

// 0:07, 1:42, 12:05
function formatDuration(ms) {
  const seconds = Math.max(0, Math.round(ms / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

function errorText(err) {
  return typeof err === "string" ? err : err?.message || String(err);
}

// Rust reports errors in English: translate the ones people actually meet.
const ERRORS = [
  [/alias must be a single word/, t("Nazwa musi być jednym słowem, bez spacji.")],
  [/alias `(.+)` is already taken/, t("Masz już kontakt o nazwie $1.")],
  [/already saved as `(.+)`/, t("Ta osoba jest już w kontaktach jako $1.")],
  [/this is your own identifier/, t("To Twój własny identyfikator.")],
  [/not a valid identifier|identifier is empty/, t("To nie jest identyfikator Szeptu.")],
  [/message is too long/, t("Wiadomość jest za długa.")],
  [/picture is too large/, t("To zdjęcie jest za duże.")],
  [/wrong password, or the archive is damaged/, t("Złe hasło albo uszkodzone archiwum.")],
  [/not a Szept archive/, t("To nie jest archiwum Szeptu.")],
  [/newer Szept/, t("To archiwum pochodzi z nowszej wersji Szeptu. Najpierw zaktualizuj aplikację.")],
  [/password must have at least/, t("Hasło musi mieć co najmniej 12 znaków.")],
  [/password is too easy to guess/, t("To hasło łatwo odgadnąć. Użyj dłuższego, najlepiej z kilku słów.")],
  [/unreasonable key settings/, t("To archiwum ma podejrzane ustawienia szyfrowania i nie zostanie otwarte.")],
  [/already has this key/, t("Ten kontakt ma już ten klucz.")],
  [/this key is already saved as `(.*)`/, t("Ten klucz jest już zapisany jako $1.")],
  [/only contacts can be added to a group/, t("Do grupy możesz dodać tylko swoje kontakty.")],
  [/a group needs at least one other member/, t("Grupa potrzebuje co najmniej jednej osoby poza Tobą.")],
  [/a group has at most (\d+) members/, t("Grupa może mieć najwyżej $1 osób.")],
  [/the group needs a name/, t("Nadaj grupie nazwę.")],
  [/only the owner can change the group/, t("Grupę zmienia tylko jej właściciel.")],
  [/you are not in this group/, t("Nie jesteś już w tej grupie.")],
  [/the name is too long/, t("To imię jest za długie.")],
  [/must be a JPEG, PNG, WebP or GIF/, t("Wybierz zdjęcie w formacie JPEG, PNG, WebP albo GIF.")],
  [/media file is too large/, t("Ten plik jest za duży na wiadomość. Wyślij go spinaczem.")],
  [/cannot be sent as media/, t("Tego pliku nie da się wysłać jako GIF. Wyślij go spinaczem.")],
  [/app is offline|shutting down/, t("Szept nie jest połączony z siecią.")],
  [/no longer shares this file|offer is no longer known/, t("Nadawca już nie udostępnia tego pliku.")],
  [/changed or disappeared/, t("Plik zmienił się albo zniknął u nadawcy.")],
  [/arrived damaged/, t("Plik dotarł uszkodzony i został usunięty. Spróbuj ponownie.")],
  [/did not answer in time|cannot connect|connection lost|stalled|cut short/, t("Nadawca jest niedostępny. Pobieranie wznowi się od miejsca przerwania.")],
  [/the Nano wallet is off/, t("Portfel Nano jest wyłączony.")],
  [/no Nano node answered/, t("Żaden węzeł Nano nie odpowiada. Sprawdź połączenie z internetem albo ustaw inny węzeł.")],
  [/not enough XNO: the balance is (\S+)/, t("Za mało środków: saldo to $1 XNO.")],
  [/the wallet is empty/, t("Portfel jest pusty: nic jeszcze do niego nie wpłynęło.")],
  [/this contact has no Nano wallet/, t("Ten kontakt nie ma włączonego portfela Nano.")],
  [/payments go only to contacts/, t("Płacić z rozmowy można tylko kontaktom.")],
  [/the wallet's own address/, t("To adres Twojego własnego portfela.")],
  [/checksum does not match/, t("W adresie jest literówka (nie zgadza się suma kontrolna).")],
  [/a Nano address starts with nano_/, t("Adres Nano zaczyna się od nano_.")],
  [/Nano address has 60 characters|cannot contain|this is not a Nano address/, t("To nie jest poprawny adres Nano.")],
  [/enter an amount/, t("Wpisz kwotę.")],
  [/must be more than zero/, t("Kwota musi być większa od zera.")],
  [/the amount must be a number/, t("Kwota musi być liczbą, np. 0,5.")],
  [/at most 30 decimal places/, t("Kwota może mieć najwyżej 30 miejsc po przecinku.")],
  [/amount is too large/, t("Ta kwota jest za duża.")],
  [/24 words or 64 hex digits/, t("Kopia portfela to 24 słowa albo 64 znaki szesnastkowe.")],
  [/not a valid backup/, t("Te słowa nie są poprawną kopią portfela (literówka albo słowo nie na swoim miejscu).")],
  [/node address starts with https|plain http/, t("Adres węzła musi zaczynać się od https:// (http:// tylko w sieci lokalnej).")],
  [/give one word for each position/, t("Wpisz wszystkie słowa.")],
];

function friendly(err) {
  const text = errorText(err);
  for (const [pattern, polish] of ERRORS) {
    if (pattern.test(text)) return text.replace(new RegExp(`^.*?(?:${pattern.source}).*$`, "s"), polish);
  }
  return text;
}
