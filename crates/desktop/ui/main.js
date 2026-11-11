// The contact list window.
"use strict";

const $ = (id) => document.getElementById(id);
let myId = "";

$("logo").innerHTML = LOGO;

async function refresh() {
  try {
    const data = await invoke("overview");
    myId = data.my_id;
    // The SVG comes from our own Rust code, never from the network.
    $("qr-small").innerHTML = data.qr_svg;
    $("qr-large").innerHTML = data.qr_svg;
    $("my-id").textContent = data.my_id;
    renderContacts(data.contacts);
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
          el("span", {
            class: "avatar",
            "data-online": String(contact.online),
            text: [...contact.alias][0] || "?",
          }),
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
