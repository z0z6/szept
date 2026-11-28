// The optional Nano wallet, in the contact list window.
"use strict";

let wallet = null;

function setError(id, text) {
  $(id).textContent = text;
  $(id).hidden = !text;
}

async function refreshWallet() {
  try {
    wallet = await invoke("wallet");
  } catch (err) {
    showStatus(err);
    return;
  }
  // The settings: off by default, nothing changes for people who never want it.
  $("wallet-toggle").textContent = wallet.enabled ? t("Wyłącz portfel") : t("Włącz portfel");
  $("wallet-settings-hint").textContent = wallet.enabled
    ? t("Portfel jest włączony. Kontakty widzą Twój adres Nano.")
    : wallet.exists
      ? t("Portfel jest wyłączony. Seed i środki zostały na tym komputerze: włącz go, żeby znowu z niego korzystać.")
      : t("Opcjonalny portfel kryptowaluty Nano: przesyłaj kontaktom środki prosto z rozmowy.");
  $("wallet-restore-open").hidden = false;
  $("wallet-nodes-open").hidden = !wallet.exists;

  $("wallet-card").hidden = !wallet.enabled;
  $("wallet-card-backup").hidden = !wallet.enabled || wallet.backed_up;
  $("wallet-card-backup-do").hidden = !wallet.enabled || wallet.backed_up;
  if (wallet.enabled) {
    $("wallet-qr").innerHTML = wallet.qr_svg || "";
    $("wallet-address").textContent = wallet.address || "";
    refreshBalance();
  }
}

let balanceRun = 0;
async function refreshBalance() {
  const run = ++balanceRun;
  $("wallet-card-balance").textContent = t("Sprawdzam saldo…");
  $("wallet-balance").textContent = t("Sprawdzam saldo…");
  try {
    const balance = await invoke("wallet_balance");
    if (run !== balanceRun) return;
    const text = `${balance.balance} XNO`;
    $("wallet-card-balance").textContent = text;
    $("wallet-balance").textContent = text;
    const waiting = balance.receivable !== "0";
    $("wallet-receivable").hidden = !waiting;
    $("wallet-receivable").textContent = waiting
      ? t("Czeka na odebranie: {receivable} XNO", { receivable: balance.receivable })
      : "";
  } catch (err) {
    if (run !== balanceRun) return;
    $("wallet-card-balance").textContent = t("Saldo niedostępne");
    $("wallet-balance").textContent = t("Saldo niedostępne");
    setError("wallet-error", friendly(err));
  }
}

// Turning it on: explain, create, then the 24 words straight away.
$("wallet-toggle").addEventListener("click", async () => {
  if (!wallet) return;
  if (wallet.enabled) {
    try {
      await invoke("wallet_disable");
      showStatus(t("Portfel wyłączony. Seed i środki zostały na tym komputerze."));
    } catch (err) {
      showStatus(err);
    }
    return;
  }
  if (wallet.exists) {
    try {
      await invoke("wallet_create");
      if (!wallet.backed_up) openWords();
    } catch (err) {
      showStatus(err);
    }
    return;
  }
  setError("wallet-intro-error", "");
  $("wallet-intro-dialog").showModal();
});

$("wallet-intro-confirm").addEventListener("click", async () => {
  try {
    await invoke("wallet_create");
    $("wallet-intro-dialog").close();
    openWords();
  } catch (err) {
    setError("wallet-intro-error", friendly(err));
  }
});

$("wallet-open").addEventListener("click", () => {
  setError("wallet-error", "");
  $("wallet-dialog").showModal();
  refreshBalance();
});

$("wallet-copy").addEventListener("click", async () => {
  try {
    await navigator.clipboard.writeText(wallet?.address || "");
    $("wallet-copy").textContent = t("Skopiowano");
    setTimeout(() => ($("wallet-copy").textContent = t("Kopiuj adres")), 1500);
  } catch {
    // The address stays visible to copy by hand.
  }
});

$("wallet-refresh").addEventListener("click", () => {
  setError("wallet-error", "");
  refreshBalance();
});

$("wallet-receive").addEventListener("click", async () => {
  setError("wallet-error", "");
  $("wallet-receive").disabled = true;
  $("wallet-receive").textContent = t("Odbieram…");
  try {
    const count = await invoke("wallet_receive");
    showStatus(count ? t("Odebrano przelewy: {count}.", { count }) : t("Nic nie czeka na odebranie."));
    refreshBalance();
  } catch (err) {
    setError("wallet-error", friendly(err));
  } finally {
    $("wallet-receive").disabled = false;
    $("wallet-receive").textContent = t("Odbierz teraz");
  }
});

// The 24 words, then three of them to check the copy.
let quiz = [];

async function openWords() {
  setError("words-error", "");
  let words;
  try {
    words = await invoke("wallet_words");
  } catch (err) {
    showStatus(err);
    return;
  }
  $("words-list").replaceChildren(...words.map((word) => el("li", { text: word })));
  $("words-show").hidden = false;
  $("words-quiz").hidden = true;
  $("words-next").textContent = t("Zapisałem, dalej");
  $("words-dialog").showModal();
  words = null;
}

function closeWords() {
  // The words leave the page as soon as the dialog closes.
  $("words-list").replaceChildren();
  $("words-quiz-fields").replaceChildren();
}

$("words-dialog").addEventListener("close", closeWords);
$("wallet-words-open").addEventListener("click", openWords);
$("wallet-card-backup-do").addEventListener("click", openWords);

$("words-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  setError("words-error", "");
  if (!$("words-show").hidden) {
    quiz = await invoke("wallet_backup_quiz");
    $("words-list").replaceChildren();
    $("words-quiz-fields").replaceChildren(
      ...quiz.map((position) =>
        el(
          "label",
          { class: "field" },
          t("Słowo nr {position}", { position }),
          el("input", { "data-position": String(position), autocomplete: "off", spellcheck: "false", required: true }),
        ),
      ),
    );
    $("words-show").hidden = true;
    $("words-quiz").hidden = false;
    $("words-next").textContent = t("Sprawdź");
    $("words-quiz-fields").querySelector("input")?.focus();
    return;
  }
  const inputs = [...$("words-quiz-fields").querySelectorAll("input")];
  try {
    const ok = await invoke("wallet_confirm_backup", {
      positions: inputs.map((input) => Number(input.dataset.position)),
      words: inputs.map((input) => input.value),
    });
    if (!ok) {
      setError("words-error", t("Te słowa się nie zgadzają. Sprawdź kopię i spróbuj jeszcze raz albo wróć do listy."));
      $("words-next").textContent = t("Sprawdź");
      return;
    }
    $("words-dialog").close();
    showStatus(t("Kopia portfela potwierdzona."));
  } catch (err) {
    setError("words-error", friendly(err));
  }
});

// Restoring from words.
$("wallet-restore-open").addEventListener("click", () => {
  setError("restore-error", "");
  $("restore-words").value = "";
  $("restore-note").hidden = !wallet?.exists;
  $("restore-dialog").showModal();
});

$("restore-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  setError("restore-error", "");
  try {
    const restored = await invoke("wallet_restore", { backup: $("restore-words").value });
    $("restore-words").value = "";
    $("restore-dialog").close();
    showStatus(t("Portfel przywrócony: {address}", { address: restored.address }));
  } catch (err) {
    setError("restore-error", friendly(err));
  }
});

// Nano nodes.
$("wallet-nodes-open").addEventListener("click", () => {
  setError("nodes-error", "");
  const nodes = wallet?.nodes || [];
  const same = JSON.stringify(nodes) === JSON.stringify(wallet?.default_nodes || []);
  $("nodes-list").value = same ? "" : nodes.join("\n");
  $("nodes-list").placeholder = (wallet?.default_nodes || []).join("\n");
  $("nodes-dialog").showModal();
});

$("nodes-default").addEventListener("click", () => ($("nodes-list").value = ""));

$("nodes-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  setError("nodes-error", "");
  const nodes = $("nodes-list").value.split("\n").map((line) => line.trim()).filter(Boolean);
  try {
    await invoke("wallet_set_nodes", { nodes });
    $("nodes-dialog").close();
  } catch (err) {
    setError("nodes-error", friendly(err));
  }
});

// Sending to any address, for example another wallet of yours.
let sendReady = null;

function resetSend() {
  sendReady = null;
  $("send-fields").hidden = false;
  $("send-summary").hidden = true;
  $("send-progress").hidden = true;
  $("send-confirm").textContent = t("Dalej");
  $("send-confirm").disabled = false;
  setError("send-error", "");
}

$("wallet-send-open").addEventListener("click", () => {
  resetSend();
  $("send-address").value = "";
  $("send-amount").value = "";
  $("send-dialog").showModal();
});

$("send-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  setError("send-error", "");
  if (!sendReady) {
    try {
      const address = await invoke("check_nano_address", { text: $("send-address").value });
      const amount = await invoke("check_nano_amount", { text: $("send-amount").value });
      sendReady = { address, amount };
      $("send-fields").hidden = true;
      $("send-summary").hidden = false;
      $("send-summary").replaceChildren(
        el("strong", { text: t("Wysłać {amount} XNO?", { amount }) }),
        el("span", { class: "identifier", text: address }),
        el("span", { text: t("Przelewu nie da się cofnąć. Sprawdź adres znak po znaku.") }),
      );
      $("send-confirm").textContent = t("Wyślij {amount} XNO", { amount });
    } catch (err) {
      setError("send-error", friendly(err));
    }
    return;
  }
  $("send-confirm").disabled = true;
  $("send-progress").hidden = false;
  $("send-progress").textContent = t("Wysyłam… Dowód pracy sieci Nano może liczyć się do minuty.");
  try {
    await invoke("send_nano_to_address", sendReady);
    $("send-dialog").close();
    showStatus(t("Wysłano {amount} XNO.", { amount: sendReady.amount }));
    refreshBalance();
  } catch (err) {
    setError("send-error", friendly(err));
    $("send-confirm").disabled = false;
    $("send-progress").hidden = true;
  }
});

let walletPending = false;
listen("wallet", () => {
  if (walletPending) return;
  walletPending = true;
  setTimeout(() => {
    walletPending = false;
    refreshWallet();
  }, 200);
});

refreshWallet();
