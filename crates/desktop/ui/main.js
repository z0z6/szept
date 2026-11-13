// The contact list window.
"use strict";

const $ = (id) => document.getElementById(id);
let myId = "";
let myAvatar = null;
let lastContacts = [];

$("logo").innerHTML = LOGO;

async function refresh() {
  try {
    const data = await invoke("overview");
    myId = data.my_id;
    // The SVG comes from our own Rust code, never from the network.
    $("qr-small").innerHTML = data.qr_svg;
    $("qr-large").innerHTML = data.qr_svg;
    $("my-id").textContent = data.my_id;
    myAvatar = data.my_avatar || null;
    fillAvatar($("my-avatar"), "", myAvatar);
    fillAvatar($("settings-avatar"), "", myAvatar);
    $("avatar-remove").hidden = !myAvatar;
    if (document.activeElement !== $("my-name")) $("my-name").value = data.my_name;
    lastContacts = data.contacts;
    renderContacts(data.contacts);
    renderGroups(data.groups);
    $("status").textContent = "";
  } catch (err) {
    $("status").textContent = friendly(err);
  }
}

function renderContacts(contacts) {
  const list = $("contacts");
  list.replaceChildren(
    ...contacts.map((contact) => {
      const status = contact.online ? "online" : "offline";
      const preview = contact.last || (contact.online ? "Połączony" : "Niepołączony");
      const label = `${contact.alias}, ${status}` +
        (contact.unread ? `, nieprzeczytane: ${contact.unread}` : "");
      return el(
        "li",
        { class: "contact" },
        el(
          "button",
          {
            class: "contact-open",
            type: "button",
            "aria-label": `Otwórz rozmowę: ${label}`,
            onclick: () => invoke("open_chat", { peer: contact.id }).catch(showStatus),
          },
          (() => {
            const avatar = el("span", { class: "avatar", "data-online": String(contact.online) });
            fillAvatar(avatar, contact.alias, contact.avatar);
            return avatar;
          })(),
          el(
            "span",
            { class: "contact-text" },
            el("span", { class: "alias", text: contact.alias }),
            el("span", { class: "preview", text: preview }),
          ),
          contact.unread ? el("span", { class: "badge", text: String(contact.unread) }) : el("span"),
        ),
        el("button", {
          class: "icon-button remove",
          type: "button",
          title: "Nowy klucz kontaktu (po ponownej instalacji)",
          "aria-label": `Zaktualizuj klucz kontaktu ${contact.alias}`,
          text: "🔑",
          onclick: () => askKey(contact.alias),
        }),
        el("button", {
          class: "icon-button remove",
          type: "button",
          title: "Usuń kontakt",
          "aria-label": `Usuń kontakt ${contact.alias}`,
          text: "✕",
          onclick: () => askRemove(contact.alias),
        }),
      );
    }),
  );
  $("no-contacts").hidden = contacts.length > 0;
}

function renderGroups(groups) {
  $("groups").replaceChildren(
    ...groups.map((group) => {
      const preview = group.last || `${group.members} osób`;
      const label = `Grupa ${group.name}` + (group.unread ? `, nieprzeczytane: ${group.unread}` : "");
      return el(
        "li",
        { class: "contact" },
        el(
          "button",
          {
            class: "contact-open",
            type: "button",
            "aria-label": `Otwórz rozmowę: ${label}`,
            onclick: () => invoke("open_chat", { peer: group.id }).catch(showStatus),
          },
          el("span", { class: "avatar group-avatar", text: [...group.name][0] || "#" }),
          el(
            "span",
            { class: "contact-text" },
            el("span", { class: "alias", text: group.name }),
            el("span", { class: "preview", text: preview }),
          ),
          group.unread ? el("span", { class: "badge", text: String(group.unread) }) : el("span"),
        ),
      );
    }),
  );
  $("no-groups").hidden = groups.length > 0;
}

function showStatus(err) {
  $("status").textContent = friendly(err);
}

// Refresh at most once per animation frame, however many events arrive.
let pending = false;
listen("changed", () => {
  if (pending) return;
  pending = true;
  requestAnimationFrame(() => {
    pending = false;
    refresh();
  });
});

// Your code
$("qr-small").addEventListener("click", () => $("qr-dialog").showModal());
$("copy-id").addEventListener("click", async () => {
  try {
    await navigator.clipboard.writeText(myId);
    $("copy-id").textContent = "Skopiowano";
    setTimeout(() => ($("copy-id").textContent = "Kopiuj identyfikator"), 1500);
  } catch {
    $("qr-dialog").showModal();
  }
});

for (const button of document.querySelectorAll("[data-close]")) {
  button.addEventListener("click", () => button.closest("dialog").close());
}

// Settings: profile picture and colours
const settingsDialog = $("settings-dialog");

$("settings-open").addEventListener("click", () => {
  setAvatarError("");
  renderThemes();
  for (const radio of document.querySelectorAll('input[name="mode"]')) {
    radio.checked = radio.value === currentMode();
  }
  for (const radio of document.querySelectorAll('input[name="text-size"]')) {
    radio.checked = radio.value === currentTextSize();
  }
  settingsDialog.showModal();
});

function setAvatarError(text) {
  $("avatar-error").textContent = text;
  $("avatar-error").hidden = !text;
}

function renderThemes() {
  const chosen = currentTheme().id;
  const dark = isDark();
  $("themes").replaceChildren(
    ...THEMES.map((theme) => {
      const colors = theme[dark ? "dark" : "light"];
      const swatch = el("span", { class: "swatch", "aria-hidden": "true" });
      swatch.style.setProperty("--swatch-accent", colors.accent);
      swatch.style.setProperty("--swatch-mine", colors.mine);
      swatch.style.setProperty("--swatch-mist", colors.mist);
      return el(
        "button",
        {
          class: "theme",
          type: "button",
          role: "radio",
          "aria-checked": String(theme.id === chosen),
          onclick: () => {
            chooseTheme(theme.id);
            renderThemes();
          },
        },
        swatch,
        el("span", { text: theme.name }),
      );
    }),
  );
}

for (const radio of document.querySelectorAll('input[name="mode"]')) {
  radio.addEventListener("change", () => {
    chooseMode(radio.value);
    renderThemes();
  });
}

// The picture is cut to a square in the middle, scaled down and saved as
// JPEG, small enough to travel to contacts in a single message.
async function pictureForAvatar(file) {
  const bitmap = await createImageBitmap(file);
  const side = Math.min(bitmap.width, bitmap.height);
  const sx = (bitmap.width - side) / 2;
  const sy = (bitmap.height - side) / 2;
  const square = document.createElement("canvas");
  const draw = square.getContext("2d");
  for (const [size, quality] of [[256, 0.86], [256, 0.72], [192, 0.7], [128, 0.65]]) {
    square.width = size;
    square.height = size;
    // JPEG has no transparency: put transparent pictures on white.
    draw.fillStyle = "#ffffff";
    draw.fillRect(0, 0, size, size);
    draw.imageSmoothingQuality = "high";
    draw.drawImage(bitmap, sx, sy, side, side, 0, 0, size, size);
    const blob = await new Promise((resolve) => square.toBlob(resolve, "image/jpeg", quality));
    if (blob && blob.size <= MAX_AVATAR_LEN) return new Uint8Array(await blob.arrayBuffer());
  }
  throw new Error("picture is too large");
}

$("avatar-file").addEventListener("change", async (event) => {
  const file = event.target.files?.[0];
  event.target.value = "";
  if (!file) return;
  setAvatarError("");
  let bytes;
  try {
    bytes = await pictureForAvatar(file);
  } catch {
    setAvatarError("Nie udało się otworzyć tego zdjęcia.");
    return;
  }
  try {
    await invoke("set_avatar", bytes);
  } catch (err) {
    setAvatarError(friendly(err));
  }
});

$("avatar-remove").addEventListener("click", async () => {
  setAvatarError("");
  try {
    // No body means no picture.
    await invoke("set_avatar");
  } catch (err) {
    setAvatarError(friendly(err));
  }
});

for (const radio of document.querySelectorAll('input[name="text-size"]')) {
  radio.addEventListener("change", () => chooseTextSize(radio.value));
}

// Preferences in a file of their own
$("prefs-save").addEventListener("click", async () => {
  const json = JSON.stringify(
    { szept: "ustawienia", version: 1, preferences: collectPreferences() },
    null,
    2,
  );
  try {
    if (await invoke("save_settings_file", { json })) showStatus("Ustawienia zapisane do pliku.");
  } catch (err) {
    showStatus(err);
  }
});

$("prefs-load").addEventListener("click", async () => {
  try {
    const text = await invoke("load_settings_file");
    if (text === null) return;
    let data;
    try {
      data = JSON.parse(text);
    } catch {
      data = null;
    }
    if (data?.szept !== "ustawienia" || typeof data.preferences !== "object") {
      showStatus("To nie jest plik ustawień Szeptu.");
      return;
    }
    const applied = applyPreferences(data.preferences);
    refreshPreferenceControls();
    showStatus(applied ? "Ustawienia wczytane." : "W pliku nie było ustawień, które ta wersja zna.");
  } catch (err) {
    showStatus(err);
  }
});

// After preferences change from a file or an archive.
function refreshPreferenceControls() {
  renderThemes();
  for (const radio of document.querySelectorAll('input[name="mode"]')) {
    radio.checked = radio.value === currentMode();
  }
  for (const radio of document.querySelectorAll('input[name="text-size"]')) {
    radio.checked = radio.value === currentTextSize();
  }
  $("sounds").checked = soundsOn();
}

// Your name
$("name-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  try {
    await invoke("set_name", { name: $("my-name").value });
    $("my-name").blur();
    showStatus("Zapisano imię.");
  } catch (err) {
    showStatus(err);
  }
});

// Archives
let archiveMode = "export";

function setArchiveError(text) {
  $("archive-error").textContent = text;
  $("archive-error").hidden = !text;
}

function openArchive(mode) {
  archiveMode = mode;
  const exporting = mode === "export";
  $("archive-form").reset();
  setArchiveError("");
  $("archive-progress").hidden = true;
  $("archive-dialog-title").textContent = exporting ? "Zapisz archiwum" : "Wczytaj archiwum";
  $("archive-hint").textContent = exporting
    ? "Archiwum jest szyfrowane tym hasłem (Argon2id, 128 MiB, XChaCha20-Poly1305). Bez hasła nie da się go odczytać, więc go nie zgub. Najlepsze jest długie hasło z kilku słów."
    : "Kontakty, grupy i rozmowy z archiwum dołączą do tych, które już tu są.";
  $("archive-repeat-field").hidden = !exporting;
  $("archive-repeat").required = exporting;
  $("archive-identity").checked = true;
  $("archive-identity-warning").hidden = !exporting;
  $("archive-strength").hidden = true;
  $("archive-suggest-row").hidden = !exporting;
  $("archive-suggestion").hidden = true;
  setPasswordVisible(false);
  $("archive-identity-label").textContent = exporting
    ? "Dołącz mój klucz tożsamości (po wczytaniu zostanie ten sam identyfikator, kontakty nic nie muszą robić)"
    : "Przywróć identyfikator z archiwum (obecny identyfikator zostanie zastąpiony)";
  $("archive-confirm").textContent = exporting ? "Wybierz plik i zapisz" : "Wybierz plik i wczytaj";
  $("archive-dialog").showModal();
}

const STRENGTH_WORDS = ["bardzo słabe", "słabe", "średnie", "dobre", "bardzo dobre"];
let strengthTimer = 0;
let lastStrength = null;

$("archive-password").addEventListener("input", () => {
  if (archiveMode !== "export") return;
  clearTimeout(strengthTimer);
  strengthTimer = setTimeout(rateArchivePassword, 150);
});

async function rateArchivePassword() {
  const password = $("archive-password").value;
  if (!password) {
    $("archive-strength").hidden = true;
    lastStrength = null;
    return;
  }
  try {
    lastStrength = await invoke("password_strength", { password });
  } catch {
    return;
  }
  $("archive-strength").hidden = false;
  $("archive-meter").value = lastStrength.score;
  const enough = lastStrength.acceptable ? "" : " – za słabe na archiwum";
  $("archive-strength-text").textContent = `Hasło: ${STRENGTH_WORDS[lastStrength.score]}${enough}`;
}

function setPasswordVisible(visible) {
  for (const id of ["archive-password", "archive-repeat"]) $(id).type = visible ? "text" : "password";
  $("archive-show").setAttribute("aria-pressed", String(visible));
  $("archive-show").textContent = visible ? "Ukryj hasło" : "Pokaż hasło";
}

$("archive-show").addEventListener("click", () =>
  setPasswordVisible($("archive-show").getAttribute("aria-pressed") !== "true"));

$("archive-suggest").addEventListener("click", async () => {
  try {
    const phrase = await invoke("suggest_passphrase");
    $("archive-password").value = phrase;
    $("archive-repeat").value = phrase;
    setPasswordVisible(true);
    $("archive-suggestion").textContent =
      "Zapisz to hasło na kartce albo w menedżerze haseł. Bez niego archiwum przepadnie.";
    $("archive-suggestion").hidden = false;
    await rateArchivePassword();
  } catch (err) {
    setArchiveError(friendly(err));
  }
});

$("archive-identity").addEventListener("change", () => {
  if (archiveMode === "export") $("archive-identity-warning").hidden = !$("archive-identity").checked;
});

$("archive-export").addEventListener("click", () => openArchive("export"));
$("archive-import").addEventListener("click", () => openArchive("import"));

$("archive-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const password = $("archive-password").value;
  const identity = $("archive-identity").checked;
  setArchiveError("");
  if ([...password].length < 12) {
    setArchiveError("Hasło musi mieć co najmniej 12 znaków.");
    return;
  }
  if (archiveMode === "export") {
    await rateArchivePassword();
    if (!lastStrength?.acceptable) {
      setArchiveError("To hasło łatwo odgadnąć. Dodaj kilka słów albo użyj przycisku Wymyśl mocne hasło.");
      return;
    }
  }
  if (archiveMode === "export" && password !== $("archive-repeat").value) {
    setArchiveError("Hasła się różnią.");
    return;
  }
  $("archive-confirm").disabled = true;
  $("archive-progress").hidden = false;
  $("archive-progress").textContent = archiveMode === "export" ? "Zapisywanie…" : "Wczytywanie…";
  try {
    if (archiveMode === "export") {
      const saved = await invoke("export_archive", {
        password,
        includeIdentity: identity,
        preferences: collectPreferences(),
      });
      if (saved) {
        $("archive-dialog").close();
        showStatus("Archiwum zapisane.");
      }
    } else {
      const report = await invoke("import_archive_file", { password, restoreIdentity: identity });
      if (report) {
        $("archive-dialog").close();
        const applied = applyPreferences(report.preferences || {});
        refreshPreferenceControls();
        const parts = [
          `rozmowy: ${report.conversations}`,
          `nowe kontakty: ${report.contacts_added}`,
          `nowe grupy: ${report.groups_added}`,
        ];
        if (report.identity_restored) parts.unshift("przywrócono identyfikator");
        if (applied) parts.push("przywrócono ustawienia");
        else if (identity && !report.had_identity) parts.push("archiwum nie zawierało klucza tożsamości");
        showStatus(`Archiwum wczytane: ${parts.join(", ")}.`);
      }
    }
  } catch (err) {
    setArchiveError(friendly(err));
  } finally {
    $("archive-confirm").disabled = false;
    $("archive-progress").hidden = true;
  }
});

// A contact's new key
let rekeying = null;

function askKey(alias) {
  rekeying = alias;
  $("key-form").reset();
  $("key-error").hidden = true;
  $("key-title").textContent = `Nowy klucz: ${alias}`;
  $("key-dialog").showModal();
}

$("key-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  try {
    await invoke("update_contact_key", { alias: rekeying, id: $("key-id").value.trim() });
    $("key-dialog").close();
    showStatus(`${rekeying} ma nowy klucz. Rozmowa toczy się dalej.`);
  } catch (err) {
    $("key-error").textContent = friendly(err);
    $("key-error").hidden = false;
  }
});

// New group
$("new-group").addEventListener("click", () => {
  $("group-form").reset();
  $("group-error").hidden = true;
  if (!lastContacts.length) {
    showStatus("Najpierw dodaj kontakty, z którymi chcesz założyć grupę.");
    return;
  }
  $("group-members").replaceChildren(
    ...lastContacts.map((contact) =>
      el(
        "label",
        { class: "check" },
        el("input", { type: "checkbox", value: contact.id }),
        el("span", { text: contact.alias }),
      ),
    ),
  );
  $("group-dialog").showModal();
});

$("group-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const members = [...$("group-members").querySelectorAll("input:checked")].map((input) => input.value);
  if (!members.length) {
    $("group-error").textContent = "Zaznacz co najmniej jedną osobę.";
    $("group-error").hidden = false;
    return;
  }
  try {
    const group = await invoke("create_group", { name: $("group-name").value.trim(), members });
    $("group-dialog").close();
    await invoke("open_chat", { peer: group });
  } catch (err) {
    $("group-error").textContent = friendly(err);
    $("group-error").hidden = false;
  }
});

// Sounds
$("sounds").checked = soundsOn();
$("sounds").addEventListener("change", (event) => {
  setSounds(event.target.checked);
  if (event.target.checked) sounds.incoming();
});

// Adding a contact
const addDialog = $("add-dialog");

$("add").addEventListener("click", () => {
  $("add-form").reset();
  setAddError("");
  setScanStatus("");
  addDialog.showModal();
  $("new-id").focus();
});

addDialog.addEventListener("close", stopCamera);

$("add-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const alias = $("new-alias").value.trim();
  const id = $("new-id").value.trim();
  try {
    await invoke("add_contact", { alias, id });
    addDialog.close();
    $("status").textContent = `Dodano: ${alias}. Ta osoba też musi dodać Ciebie.`;
  } catch (err) {
    setAddError(friendly(err));
  }
});

function setAddError(text) {
  $("add-error").textContent = text;
  $("add-error").hidden = !text;
}

function setScanStatus(text) {
  $("scan-status").textContent = text;
  $("scan-status").hidden = !text;
}

// The scanner: the camera picture is read frame by frame until a Szept
// code shows up. Nothing leaves this computer.
let stream = null;
let scanTimer = null;
const canvas = document.createElement("canvas");
const context = canvas.getContext("2d", { willReadFrequently: true });

$("scan").addEventListener("click", async () => {
  if (stream) {
    stopCamera();
    return;
  }
  setAddError("");
  if (!navigator.mediaDevices?.getUserMedia) {
    setScanStatus("Kamera nie jest tu dostępna. Wczytaj zrzut ekranu z kodem albo wklej identyfikator.");
    return;
  }
  try {
    stream = await navigator.mediaDevices.getUserMedia({
      video: { facingMode: "environment", width: { ideal: 1280 }, height: { ideal: 720 } },
      audio: false,
    });
  } catch (err) {
    stream = null;
    const missing = err?.name === "NotFoundError" || err?.name === "OverconstrainedError";
    setScanStatus(missing
      ? "Nie znaleziono kamery. Wczytaj zrzut ekranu z kodem albo wklej identyfikator."
      : "Brak dostępu do kamery. Zezwól na nią w ustawieniach systemu albo wczytaj obraz z kodem.");
    return;
  }
  const video = $("video");
  video.srcObject = stream;
  $("scanner").hidden = false;
  $("scan").textContent = "Zatrzymaj kamerę";
  setScanStatus("Skieruj kamerę na kod QR drugiej osoby.");
  await video.play().catch(() => {});
  scanTimer = setInterval(() => scanFrame(video), 200);
});

function scanFrame(video) {
  if (!video.videoWidth) return;
  // Reading a smaller picture is faster and still finds the code.
  const scale = Math.min(1, 640 / video.videoWidth);
  canvas.width = Math.round(video.videoWidth * scale);
  canvas.height = Math.round(video.videoHeight * scale);
  context.drawImage(video, 0, 0, canvas.width, canvas.height);
  const found = readCode(context.getImageData(0, 0, canvas.width, canvas.height));
  if (found) accept(found);
}

function readCode(image) {
  const code = jsQR(image.data, image.width, image.height, { inversionAttempts: "attemptBoth" });
  return code?.data || null;
}

let lastRejected = "";

async function accept(text) {
  try {
    const id = await invoke("normalize_id", { text });
    stopCamera();
    $("new-id").value = id;
    setScanStatus("Kod odczytany. Nadaj kontaktowi nazwę.");
    $("new-alias").focus();
  } catch {
    if (text !== lastRejected) {
      lastRejected = text;
      setScanStatus("To nie jest kod Szeptu. Zeskanuj kod z aplikacji drugiej osoby.");
    }
  }
}

function stopCamera() {
  clearInterval(scanTimer);
  scanTimer = null;
  if (stream) {
    for (const track of stream.getTracks()) track.stop();
  }
  stream = null;
  $("video").srcObject = null;
  $("scanner").hidden = true;
  $("scan").textContent = "Skanuj kamerą";
}

// Reading a code from a picture: a screenshot, a photo, a saved image.
$("scan-file").addEventListener("change", async (event) => {
  const file = event.target.files?.[0];
  event.target.value = "";
  if (!file) return;
  stopCamera();
  try {
    const bitmap = await createImageBitmap(file);
    const scale = Math.min(1, 1600 / Math.max(bitmap.width, bitmap.height));
    canvas.width = Math.round(bitmap.width * scale);
    canvas.height = Math.round(bitmap.height * scale);
    context.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
    const found = readCode(context.getImageData(0, 0, canvas.width, canvas.height));
    if (found) {
      lastRejected = "";
      await accept(found);
    } else {
      setScanStatus("Na tym obrazie nie ma czytelnego kodu QR.");
    }
  } catch {
    setScanStatus("Nie udało się otworzyć tego obrazu.");
  }
});

// Removing a contact
let removing = null;

function askRemove(alias) {
  removing = alias;
  $("remove-title").textContent = `Usunąć kontakt ${alias}?`;
  $("remove-dialog").showModal();
}

$("remove-confirm").addEventListener("click", async () => {
  $("remove-dialog").close();
  if (!removing) return;
  try {
    await invoke("remove_contact", { alias: removing });
  } catch (err) {
    showStatus(err);
  }
  removing = null;
});

refresh();
