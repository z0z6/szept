//! Szept on the desktop: a window with the contact list and one window per
//! conversation. The Rust core runs in this process; the windows are web
//! views that talk to it through the commands below.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod store;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

use base64::Engine as _;
use komunikator_ffi::{
    App, AppError, ChatEvent, EventListener, Group, Media, file_kind, import_archive,
    max_media_size, qr_code, share_link,
};
use serde::Serialize;
use tauri::{
    AppHandle, DragDropEvent, Emitter, Manager, RunEvent, State, UserAttentionType, WebviewUrl,
    WebviewWindowBuilder, WindowEvent,
    ipc::{InvokeBody, Request},
};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

use crate::store::{FileInfo, Message, Status, Store, Transfer};

/// Everything the commands share.
struct Shared {
    /// Replaced when an archive brings back an older identity.
    core: RwLock<Arc<App>>,
    store: Mutex<Store>,
    /// Voice messages and pasted GIFs we send wait here until the app
    /// closes: the contact fetches them from this folder.
    outgoing: PathBuf,
    data_dir: PathBuf,
}

impl Shared {
    fn core(&self) -> Arc<App> {
        self.core.read().unwrap().clone()
    }
}

/// Group identifiers have 32 hex digits, people's 64.
fn is_group(key: &str) -> bool {
    key.len() == 32
}

type CommandResult<T> = Result<T, String>;

fn failed(err: AppError) -> String {
    err.to_string()
}

#[derive(Serialize)]
struct ContactView {
    alias: String,
    id: String,
    online: bool,
    unread: u32,
    last: Option<String>,
    /// The contact's profile picture as a `data:` URL.
    avatar: Option<String>,
}

#[derive(Serialize)]
struct GroupSummary {
    id: String,
    name: String,
    mine: bool,
    members: usize,
    unread: u32,
    last: Option<String>,
}

#[derive(Serialize)]
struct Overview {
    my_id: String,
    my_name: String,
    link: String,
    qr_svg: String,
    /// Our own profile picture as a `data:` URL.
    my_avatar: Option<String>,
    contacts: Vec<ContactView>,
    groups: Vec<GroupSummary>,
}

/// Someone in a conversation, as the window shows them.
#[derive(Serialize)]
struct Person {
    id: String,
    name: String,
    avatar: Option<String>,
    online: bool,
    contact: bool,
}

#[derive(Serialize)]
struct ChatView {
    /// The contact's alias or the group's name.
    alias: String,
    id: String,
    my_id: String,
    online: bool,
    avatar: Option<String>,
    /// For a group: whether we own it, and who is in it.
    group: bool,
    mine: bool,
    members: Vec<Person>,
    messages: Vec<Message>,
}

/// What a window needs to redraw after something changed for `peer`.
#[derive(Clone, Serialize)]
struct Changed {
    peer: String,
    /// "incoming", "sent", "delivered", "saved" or "other": lets the windows
    /// play the matching sound.
    kind: &'static str,
}

/// Something went wrong for `peer` outside a command the window awaits.
#[derive(Clone, Serialize)]
struct Problem {
    peer: String,
    text: String,
}

#[tauri::command]
fn overview(shared: State<'_, Shared>) -> CommandResult<Overview> {
    let my_id = shared.core().my_id();
    let link = share_link(my_id.clone());
    let qr_svg = qr_svg(&link)?;
    let core = shared.core();
    let store = shared.store.lock().unwrap();
    let contacts = core
        .contacts()
        .into_iter()
        .map(|contact| ContactView {
            online: core.is_online(contact.id.clone()),
            unread: store.unread(&contact.id),
            last: store.last_body(&contact.id),
            avatar: data_url(core.avatar(contact.id.clone())),
            alias: contact.alias,
            id: contact.id,
        })
        .collect();
    let groups = core
        .groups()
        .into_iter()
        .map(|group| GroupSummary {
            unread: store.unread(&group.id),
            last: store.last_body(&group.id),
            members: group.members.len(),
            mine: group.mine,
            name: group.name,
            id: group.id,
        })
        .collect();
    Ok(Overview {
        my_id,
        my_name: core.my_name(),
        link,
        qr_svg,
        my_avatar: data_url(core.my_avatar()),
        contacts,
        groups,
    })
}

#[tauri::command]
fn set_name(app: AppHandle, shared: State<'_, Shared>, name: String) -> CommandResult<()> {
    shared.core().set_name(name).map_err(failed)?;
    notify_changed(&app, String::new(), "other");
    Ok(())
}

/// The name to show for `id`: our alias for a contact, else the name they
/// chose, else what the group's owner calls them.
fn person_name(core: &App, id: &str, roster: Option<&Group>) -> String {
    if let Some(contact) = core.contacts().into_iter().find(|contact| contact.id == id) {
        return contact.alias;
    }
    if let Some(name) = core.display_name(id.to_owned()) {
        return name;
    }
    roster
        .and_then(|group| group.members.iter().find(|member| member.id == id))
        .map(|member| member.name.clone())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| format!("Ktoś ({}…)", &id[..id.len().min(6)]))
}

fn find_group(core: &App, id: &str) -> Option<Group> {
    core.groups().into_iter().find(|group| group.id == id)
}

#[tauri::command]
fn create_group(
    app: AppHandle,
    shared: State<'_, Shared>,
    name: String,
    members: Vec<String>,
) -> CommandResult<String> {
    let group = shared.core().create_group(name, members).map_err(failed)?;
    notify_changed(&app, String::new(), "other");
    Ok(group.id)
}

#[tauri::command]
fn update_group(
    app: AppHandle,
    shared: State<'_, Shared>,
    group: String,
    name: String,
    members: Vec<String>,
) -> CommandResult<()> {
    let updated = shared
        .core()
        .update_group(group, name, members)
        .map_err(failed)?;
    if let Some(window) = app.get_webview_window(&chat_label(&updated.id)) {
        let _ = window.set_title(&format!("{} – Szept", updated.name));
    }
    notify_changed(&app, updated.id, "other");
    Ok(())
}

#[tauri::command]
fn leave_group(app: AppHandle, shared: State<'_, Shared>, group: String) -> CommandResult<()> {
    shared.core().leave_group(group.clone()).map_err(failed)?;
    shared.store.lock().unwrap().forget(&group);
    if let Some(window) = app.get_webview_window(&chat_label(&group)) {
        let _ = window.close();
    }
    notify_changed(&app, String::new(), "other");
    Ok(())
}

/// Gives a contact the new identifier they got after reinstalling Szept,
/// and carries the conversation over to it.
#[tauri::command]
fn update_contact_key(
    app: AppHandle,
    shared: State<'_, Shared>,
    alias: String,
    id: String,
) -> CommandResult<()> {
    let id = komunikator_ffi::normalize_id(id).map_err(failed)?;
    let old = shared
        .core()
        .update_contact_key(alias, id.clone())
        .map_err(failed)?;
    shared.store.lock().unwrap().migrate(
        &old,
        &id,
        "🔑 Kontakt ma nowy klucz. Wcześniejsza rozmowa zakończyła się tutaj, nowa toczy się poniżej."
            .into(),
    );
    if let Some(window) = app.get_webview_window(&chat_label(&old)) {
        let _ = window.close();
    }
    notify_changed(&app, id, "other");
    Ok(())
}

#[tauri::command]
fn send_sticker(
    app: AppHandle,
    shared: State<'_, Shared>,
    peer: String,
    code: String,
) -> CommandResult<()> {
    let recipients = recipients(&shared, &peer);
    {
        let mut store = shared.store.lock().unwrap();
        let id = shared
            .core()
            .send_sticker(peer.clone(), code.clone())
            .map_err(failed)?;
        let mut message = Message::outgoing(id, String::new());
        message.sticker = Some(code);
        message.recipients = recipients;
        store.push(&peer, message);
    }
    notify_changed(&app, peer, "other");
    Ok(())
}

/// Reacts to a message, or takes our reaction back with an empty `reaction`.
#[tauri::command]
fn react(
    app: AppHandle,
    shared: State<'_, Shared>,
    peer: String,
    id: String,
    reaction: String,
) -> CommandResult<()> {
    let core = shared.core();
    core.react(peer.clone(), id.clone(), reaction.clone())
        .map_err(failed)?;
    if shared
        .store
        .lock()
        .unwrap()
        .react(&peer, &id, &core.my_id(), &reaction)
    {
        notify_changed(&app, peer, "other");
    }
    Ok(())
}

/// How many people must confirm a message to `conversation`.
fn recipients(shared: &Shared, conversation: &str) -> u32 {
    if is_group(conversation) {
        find_group(&shared.core(), conversation).map_or(1, |group| {
            group.members.len().saturating_sub(1).max(1) as u32
        })
    } else {
        1
    }
}

/// Writes an encrypted archive of everything to a file the person picks.
#[tauri::command]
async fn export_archive(
    app: AppHandle,
    password: String,
    include_identity: bool,
    preferences: HashMap<String, String>,
) -> CommandResult<bool> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    let picked = app
        .dialog()
        .file()
        .set_title("Zapisz archiwum Szeptu")
        .set_file_name(format!("szept-archiwum-{stamp}.szept"))
        .add_filter("Archiwum Szeptu", &["szept"])
        .blocking_save_file();
    let Some(path) = picked.and_then(|file| file.into_path().ok()) else {
        return Ok(false);
    };
    let shared = app.state::<Shared>();
    let core = shared.core();
    let conversations = shared.store.lock().unwrap().to_archive(&core.my_id());
    core.export_archive(
        path.to_string_lossy().into_owned(),
        password,
        include_identity,
        conversations,
        preferences,
    )
    .map_err(failed)?;
    Ok(true)
}

/// Rates an archive password as the person types it.
#[tauri::command]
fn password_strength(password: String) -> komunikator_ffi::PasswordStrength {
    komunikator_ffi::password_strength(password)
}

/// A strong password made of words, easy to write down.
#[tauri::command]
fn suggest_passphrase() -> String {
    komunikator_ffi::suggest_passphrase()
}

/// The largest settings file accepted.
const MAX_SETTINGS_FILE: u64 = 64 * 1024;

/// Saves the preferences the window hands over (JSON) to a file the person
/// picks. They hold no secrets, so the file is plain.
#[tauri::command]
async fn save_settings_file(app: AppHandle, json: String) -> CommandResult<bool> {
    if json.len() as u64 > MAX_SETTINGS_FILE {
        return Err("Za dużo ustawień.".into());
    }
    let picked = app
        .dialog()
        .file()
        .set_title("Zapisz ustawienia Szeptu")
        .set_file_name("szept-ustawienia.json")
        .add_filter("Ustawienia Szeptu", &["json"])
        .blocking_save_file();
    let Some(path) = picked.and_then(|file| file.into_path().ok()) else {
        return Ok(false);
    };
    std::fs::write(&path, json).map_err(|err| err.to_string())?;
    Ok(true)
}

/// Reads a settings file the person picks and returns its text.
#[tauri::command]
async fn load_settings_file(app: AppHandle) -> CommandResult<Option<String>> {
    let picked = app
        .dialog()
        .file()
        .set_title("Wczytaj ustawienia Szeptu")
        .add_filter("Ustawienia Szeptu", &["json"])
        .blocking_pick_file();
    let Some(path) = picked.and_then(|file| file.into_path().ok()) else {
        return Ok(None);
    };
    let size = std::fs::metadata(&path)
        .map_err(|err| err.to_string())?
        .len();
    if size > MAX_SETTINGS_FILE {
        return Err("To nie jest plik ustawień Szeptu.".into());
    }
    std::fs::read_to_string(&path)
        .map(Some)
        .map_err(|_| "To nie jest plik ustawień Szeptu.".into())
}

/// What an import brought back, for the window to report.
#[derive(Serialize)]
struct ImportReport {
    identity_restored: bool,
    had_identity: bool,
    contacts_added: u32,
    groups_added: u32,
    conversations: usize,
    preferences: HashMap<String, String>,
}

/// Reads an archive the person picks: contacts, groups and conversations
/// are added, and with `restore_identity` the old identifier comes back.
#[tauri::command]
async fn import_archive_file(
    app: AppHandle,
    password: String,
    restore_identity: bool,
) -> CommandResult<Option<ImportReport>> {
    let picked = app
        .dialog()
        .file()
        .set_title("Wybierz archiwum Szeptu")
        .add_filter("Archiwum Szeptu", &["szept"])
        .blocking_pick_file();
    let Some(path) = picked.and_then(|file| file.into_path().ok()) else {
        return Ok(None);
    };
    let shared = app.state::<Shared>();
    let data_dir = shared.data_dir.to_string_lossy().into_owned();
    // The archive is read while nothing runs on the data directory.
    let old = shared.core();
    old.stop();
    let imported = import_archive(
        data_dir.clone(),
        path.to_string_lossy().into_owned(),
        password,
        restore_identity,
    );
    let core = App::new(data_dir, false).map_err(failed)?;
    core.start(Arc::new(Listener { app: app.clone() }))
        .map_err(failed)?;
    *shared.core.write().unwrap() = core.clone();
    let imported = imported.map_err(failed)?;
    let report = ImportReport {
        identity_restored: imported.identity_restored,
        had_identity: imported.had_identity,
        contacts_added: imported.contacts_added,
        groups_added: imported.groups_added,
        conversations: imported.conversations.len(),
        preferences: imported.preferences.clone(),
    };
    shared
        .store
        .lock()
        .unwrap()
        .merge_archive(imported.conversations, &core.my_id());
    // Conversations open under the old identity no longer fit.
    for (label, window) in app.webview_windows() {
        if label.starts_with("chat-") {
            let _ = window.close();
        }
    }
    notify_changed(&app, String::new(), "other");
    Ok(Some(report))
}

/// Sets our profile picture from the bytes the window sends (a JPEG it
/// scaled down), or removes it when they are empty.
#[tauri::command]
fn set_avatar(
    app: AppHandle,
    shared: State<'_, Shared>,
    request: Request<'_>,
) -> CommandResult<()> {
    // A call without a body removes the picture.
    let image = match request.body() {
        InvokeBody::Raw(bytes) => bytes.clone(),
        InvokeBody::Json(_) => Vec::new(),
    };
    shared.core().set_avatar(image).map_err(failed)?;
    notify_changed(&app, String::new(), "other");
    Ok(())
}

/// Turns a picture into a `data:` URL the windows can show without
/// touching the disk. The type comes from the picture's own signature.
fn data_url(image: Option<Vec<u8>>) -> Option<String> {
    let image = image?;
    let mime = if image.starts_with(&[0xff, 0xd8]) {
        "image/jpeg"
    } else if image.starts_with(b"\x89PNG") {
        "image/png"
    } else if image.starts_with(b"GIF8") {
        "image/gif"
    } else {
        "image/webp"
    };
    let encoded = base64::engine::general_purpose::STANDARD.encode(image);
    Some(format!("data:{mime};base64,{encoded}"))
}

#[tauri::command]
fn add_contact(
    app: AppHandle,
    shared: State<'_, Shared>,
    alias: String,
    id: String,
) -> CommandResult<()> {
    shared.core().add_contact(alias, id).map_err(failed)?;
    notify_changed(&app, String::new(), "other");
    Ok(())
}

#[tauri::command]
fn remove_contact(app: AppHandle, shared: State<'_, Shared>, alias: String) -> CommandResult<()> {
    let core = shared.core();
    let id = core
        .contacts()
        .into_iter()
        .find(|contact| contact.alias == alias)
        .map(|contact| contact.id);
    core.remove_contact(alias).map_err(failed)?;
    if let Some(id) = id {
        shared.store.lock().unwrap().forget(&id);
        if let Some(window) = app.get_webview_window(&chat_label(&id)) {
            let _ = window.close();
        }
    }
    notify_changed(&app, String::new(), "other");
    Ok(())
}

/// Opens the conversation with `peer` in its own window, or brings the
/// existing one to the front.
#[tauri::command]
fn open_chat(app: AppHandle, shared: State<'_, Shared>, peer: String) -> CommandResult<()> {
    let alias = title_of(&shared, &peer).ok_or("nieznana rozmowa")?;
    let label = chat_label(&peer);
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.unminimize();
        let _ = window.show();
        return window.set_focus().map_err(|err| err.to_string());
    }
    let url = WebviewUrl::App(format!("chat.html#{peer}").into());
    let window = WebviewWindowBuilder::new(&app, label, url)
        .title(format!("{alias} – Szept"))
        .inner_size(420.0, 620.0)
        .min_inner_size(320.0, 380.0)
        .build()
        .map_err(|err| err.to_string())?;
    // The microphone, for voice messages.
    allow_media(&window);
    Ok(())
}

#[tauri::command]
fn chat(shared: State<'_, Shared>, peer: String) -> CommandResult<ChatView> {
    let core = shared.core();
    let messages = shared.store.lock().unwrap().conversation(&peer);
    if is_group(&peer) {
        let group = find_group(&core, &peer).ok_or("nie jesteś już w tej grupie")?;
        let contacts: Vec<String> = core
            .contacts()
            .into_iter()
            .map(|contact| contact.id)
            .collect();
        let my_id = core.my_id();
        let members = group
            .members
            .iter()
            .map(|member| Person {
                name: if member.id == my_id {
                    "Ty".into()
                } else {
                    person_name(&core, &member.id, Some(&group))
                },
                avatar: if member.id == my_id {
                    data_url(core.my_avatar())
                } else {
                    data_url(core.avatar(member.id.clone()))
                },
                online: core.is_online(member.id.clone()),
                contact: contacts.contains(&member.id),
                id: member.id.clone(),
            })
            .collect::<Vec<_>>();
        let online = members.iter().any(|member| member.online);
        return Ok(ChatView {
            alias: group.name.clone(),
            id: peer,
            my_id,
            online,
            avatar: None,
            group: true,
            mine: group.mine,
            members,
            messages,
        });
    }
    let alias = alias_of(&shared, &peer).ok_or("ten kontakt został usunięty")?;
    Ok(ChatView {
        online: core.is_online(peer.clone()),
        avatar: data_url(core.avatar(peer.clone())),
        alias,
        id: peer,
        my_id: core.my_id(),
        group: false,
        mine: false,
        members: Vec::new(),
        messages,
    })
}

#[tauri::command]
fn mark_read(app: AppHandle, shared: State<'_, Shared>, peer: String) {
    if shared.store.lock().unwrap().mark_read(&peer) {
        notify_changed(&app, peer, "other");
    }
}

#[tauri::command]
fn send(
    app: AppHandle,
    shared: State<'_, Shared>,
    peer: String,
    body: String,
) -> CommandResult<()> {
    let body = body.trim().to_owned();
    let recipients = recipients(&shared, &peer);
    // The core may report "sent" right away, on another thread. Holding the
    // lock until the message is stored makes that report wait for it.
    {
        let mut store = shared.store.lock().unwrap();
        let id = shared
            .core()
            .send(peer.clone(), body.clone())
            .map_err(failed)?;
        let mut message = Message::outgoing(id, body);
        message.recipients = recipients;
        store.push(&peer, message);
    }
    notify_changed(&app, peer, "other");
    Ok(())
}

/// Lets the person pick files and offers them to `peer`.
#[tauri::command]
async fn send_files(app: AppHandle, peer: String) -> CommandResult<()> {
    let picked = app
        .dialog()
        .file()
        .set_title("Wybierz pliki do wysłania")
        .blocking_pick_files();
    let paths: Vec<PathBuf> = picked
        .unwrap_or_default()
        .into_iter()
        .filter_map(|file| file.into_path().ok())
        .collect();
    send_paths(app, peer, paths);
    Ok(())
}

/// Offers files to `peer` in the background: each one is hashed first,
/// which takes a moment for big files.
fn send_paths(app: AppHandle, peer: String, paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }
    tauri::async_runtime::spawn_blocking(move || {
        for path in paths {
            offer_path(&app, &peer, &path, None);
        }
    });
}

/// Offers one file, or a voice message or GIF when `media` is set, and
/// adds it to the conversation. Blocks while the file is hashed.
fn offer_path(app: &AppHandle, peer: &str, path: &Path, media: Option<Media>) {
    let shared = app.state::<Shared>();
    let core = shared.core();
    let text_path = path.to_string_lossy().into_owned();
    let result = match media {
        None => core.send_file(peer.to_owned(), text_path.clone()),
        Some(media) => core.send_media(peer.to_owned(), text_path.clone(), media),
    };
    match result {
        Ok(sent) => {
            // Let the windows show pictures and play music we send.
            let _ = app.asset_protocol_scope().allow_file(path);
            let (media, duration_ms) = describe_media(media);
            let file = FileInfo {
                kind: file_kind(sent.name.clone()),
                name: sent.name,
                size: sent.size,
                hash: sent.hash,
                transfer: Transfer::Shared,
                done: sent.size,
                path: Some(text_path),
                reason: None,
                media,
                duration_ms,
            };
            let mut message = Message::outgoing_file(sent.id, file);
            message.recipients = recipients(&shared, peer);
            shared.store.lock().unwrap().push(peer, message);
            notify_changed(app, peer.to_owned(), "other");
        }
        Err(err) => {
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            );
            let text = match media {
                Some(Media::Voice { .. }) => format!("Nie udało się wysłać nagrania: {err}"),
                _ => format!("Nie udało się wysłać {name}: {err}"),
            };
            let _ = app.emit(
                "problem",
                Problem {
                    peer: peer.to_owned(),
                    text,
                },
            );
        }
    }
}

/// How the windows tell media apart: "voice" or "gif", and a length.
fn describe_media(media: Option<Media>) -> (Option<&'static str>, Option<u32>) {
    match media {
        Some(Media::Voice { duration_ms }) => (Some("voice"), Some(duration_ms)),
        Some(Media::Gif) => (Some("gif"), None),
        None => (None, None),
    }
}

/// Sends a voice message recorded in a conversation window, or a GIF pasted
/// into it. The bytes come as the raw body; the headers say the rest:
/// `peer`, `media` ("voice" or "gif"), `extension` and, for voice,
/// `duration` in milliseconds.
#[tauri::command]
fn send_media(
    app: AppHandle,
    shared: State<'_, Shared>,
    request: Request<'_>,
) -> CommandResult<()> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("brak danych".into());
    };
    let header = |name: &str| {
        request
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    };
    let peer = header("peer");
    title_of(&shared, &peer).ok_or("nieznana rozmowa")?;
    let extension = header("extension").to_ascii_lowercase();
    if extension.is_empty() || !extension.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("nieznany typ pliku".into());
    }
    let (media, stem) = match header("media").as_str() {
        "voice" => {
            let duration_ms = header("duration").parse().unwrap_or(0);
            (Media::Voice { duration_ms }, "glosowa")
        }
        "gif" => (Media::Gif, "gif"),
        _ => return Err("nieznany rodzaj wiadomości".into()),
    };
    if bytes.is_empty() {
        return Err("Nagranie jest puste.".into());
    }
    if bytes.len() as u64 > max_media_size() {
        return Err("Ten plik jest za duży na wiadomość. Wyślij go spinaczem.".into());
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    // Each one gets its own folder, so two names never clash.
    let dir = shared.outgoing.join(stamp.to_string());
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let path = dir.join(format!("{stem}-{}.{extension}", stamp / 1_000_000));
    std::fs::write(&path, bytes).map_err(|err| err.to_string())?;
    tauri::async_runtime::spawn_blocking(move || offer_path(&app, &peer, &path, Some(media)));
    Ok(())
}

/// Starts or resumes fetching a file the contact offered.
#[tauri::command]
fn download(
    app: AppHandle,
    shared: State<'_, Shared>,
    peer: String,
    id: String,
) -> CommandResult<()> {
    let core = shared.core();
    let media = shared
        .store
        .lock()
        .unwrap()
        .file(&peer, &id)
        .is_some_and(|file| file.media.is_some());
    // Voice messages and GIFs live with the core's own media.
    let dir = if media {
        PathBuf::from(core.media_dir())
    } else {
        downloads_dir(&app)?
    };
    core.download(id.clone(), dir.to_string_lossy().into_owned())
        .map_err(failed)?;
    shared
        .store
        .lock()
        .unwrap()
        .set_transfer(&peer, &id, |file| {
            file.transfer = Transfer::Downloading;
            file.reason = None;
        });
    notify_changed(&app, peer, "other");
    Ok(())
}

/// Opens a received or sent file in its usual app. Programs are refused:
/// they are run on purpose, from the folder.
#[tauri::command]
fn open_file(
    app: AppHandle,
    shared: State<'_, Shared>,
    peer: String,
    id: String,
) -> CommandResult<()> {
    let file = local_file(&shared, &peer, &id)?;
    if file.kind == "program" {
        return Err("Programy uruchamiaj świadomie, z folderu.".into());
    }
    app.opener()
        .open_path(file.path.unwrap_or_default(), None::<&str>)
        .map_err(|err| err.to_string())
}

/// Shows a received or sent file in the system file manager.
#[tauri::command]
fn reveal(
    app: AppHandle,
    shared: State<'_, Shared>,
    peer: String,
    id: String,
) -> CommandResult<()> {
    let file = local_file(&shared, &peer, &id)?;
    app.opener()
        .reveal_item_in_dir(file.path.unwrap_or_default())
        .map_err(|err| err.to_string())
}

fn local_file(shared: &Shared, peer: &str, id: &str) -> CommandResult<FileInfo> {
    let file = shared
        .store
        .lock()
        .unwrap()
        .file(peer, id)
        .ok_or("nie ma takiego pliku")?;
    match &file.path {
        Some(path) if Path::new(path).exists() => Ok(file),
        _ => Err("Tego pliku nie ma już na dysku.".into()),
    }
}

/// Where received files go: a "Szept" folder inside Downloads.
fn downloads_dir(app: &AppHandle) -> CommandResult<PathBuf> {
    let base = app
        .path()
        .download_dir()
        .or_else(|_| app.path().home_dir())
        .map_err(|err| err.to_string())?;
    Ok(base.join("Szept"))
}

/// Turns pasted or scanned text into an identifier, or explains why not.
#[tauri::command]
fn normalize_id(text: String) -> CommandResult<String> {
    komunikator_ffi::normalize_id(text).map_err(|_| "to nie jest identyfikator Szeptu".into())
}

/// Receives core events on a background thread and updates the windows.
struct Listener {
    app: AppHandle,
}

impl EventListener for Listener {
    fn on_event(&self, event: ChatEvent) {
        let shared = self.app.state::<Shared>();
        match event {
            ChatEvent::Connected { peer } | ChatEvent::Disconnected { peer } => {
                notify_changed(&self.app, peer, "other");
            }
            ChatEvent::AvatarChanged { peer } | ChatEvent::NameChanged { peer } => {
                notify_changed(&self.app, peer, "other");
            }
            ChatEvent::Message {
                peer,
                id,
                sent_at_ms,
                body,
                group,
            } => {
                let (key, author) = conversation_of(peer, group);
                let message = Message {
                    body,
                    ..Message::incoming(id, author, sent_at_ms)
                };
                self.incoming(key, message);
            }
            ChatEvent::Sticker {
                peer,
                id,
                sent_at_ms,
                code,
                group,
            } => {
                let (key, author) = conversation_of(peer, group);
                let message = Message {
                    sticker: Some(code),
                    ..Message::incoming(id, author, sent_at_ms)
                };
                self.incoming(key, message);
            }
            ChatEvent::FileOffered {
                peer,
                id,
                sent_at_ms,
                name,
                size,
                hash,
                media,
                group,
            } => {
                let (media, duration_ms) = describe_media(media);
                let file = FileInfo {
                    kind: file_kind(name.clone()),
                    name,
                    size,
                    hash,
                    // Media are fetched by the core right away.
                    transfer: if media.is_some() {
                        Transfer::Downloading
                    } else {
                        Transfer::Offered
                    },
                    done: 0,
                    path: None,
                    reason: None,
                    media,
                    duration_ms,
                };
                let (key, author) = conversation_of(peer, group);
                let message = Message {
                    file: Some(file),
                    ..Message::incoming(id, author, sent_at_ms)
                };
                self.incoming(key, message);
            }
            ChatEvent::Reaction {
                peer,
                target,
                reaction,
                group,
            } => {
                let key = group.unwrap_or_else(|| peer.clone());
                if shared
                    .store
                    .lock()
                    .unwrap()
                    .react(&key, &target, &peer, &reaction)
                {
                    notify_changed(&self.app, key, "other");
                }
            }
            ChatEvent::GroupChanged { group } => {
                if let Some(info) = find_group(&shared.core(), &group)
                    && let Some(window) = self.app.get_webview_window(&chat_label(&group))
                {
                    let _ = window.set_title(&format!("{} – Szept", info.name));
                }
                notify_changed(&self.app, group, "other");
            }
            ChatEvent::GroupClosed { group, reason } => {
                let text = if reason == "removed" {
                    "Właściciel usunął Cię z tej grupy."
                } else {
                    "Właściciel zamknął tę grupę."
                };
                shared.store.lock().unwrap().note(&group, text.into());
                if let Some(window) = self.app.get_webview_window(&chat_label(&group)) {
                    let _ = window.close();
                }
                notify_changed(&self.app, group, "other");
            }
            ChatEvent::FileProgress {
                peer,
                id,
                done,
                total: _,
            } => {
                let changed = shared
                    .store
                    .lock()
                    .unwrap()
                    .update_file(&peer, &id, |file| {
                        file.transfer = Transfer::Downloading;
                        file.done = done;
                    });
                if let Some(key) = changed {
                    notify_changed(&self.app, key, "other");
                }
            }
            ChatEvent::FileSaved { peer, id, path } => {
                let _ = self.app.asset_protocol_scope().allow_file(&path);
                let mut media = false;
                let changed = shared
                    .store
                    .lock()
                    .unwrap()
                    .update_file(&peer, &id, |file| {
                        file.transfer = Transfer::Saved;
                        file.done = file.size;
                        file.path = Some(path);
                        media = file.media.is_some();
                    });
                if let Some(key) = changed {
                    // Media arrive on their own: their message already chimed.
                    notify_changed(&self.app, key, if media { "other" } else { "saved" });
                }
            }
            ChatEvent::FileFailed { peer, id, reason } => {
                let changed = shared
                    .store
                    .lock()
                    .unwrap()
                    .update_file(&peer, &id, |file| {
                        file.transfer = Transfer::Failed;
                        file.reason = Some(reason);
                    });
                if let Some(key) = changed {
                    notify_changed(&self.app, key, "other");
                }
            }
            ChatEvent::Sent { peer, id, group } => {
                let key = group.unwrap_or(peer);
                if shared
                    .store
                    .lock()
                    .unwrap()
                    .set_status(&key, &id, Status::Sent)
                {
                    notify_changed(&self.app, key, "sent");
                }
            }
            ChatEvent::Delivered { peer, id, group } => {
                let changed = match &group {
                    Some(group) => shared.store.lock().unwrap().delivered_by(group, &id, &peer),
                    None => shared
                        .store
                        .lock()
                        .unwrap()
                        .set_status(&peer, &id, Status::Delivered),
                };
                if changed {
                    notify_changed(&self.app, group.unwrap_or(peer), "delivered");
                }
            }
        }
    }
}

/// The conversation an event belongs to, and its author in a group.
fn conversation_of(peer: String, group: Option<String>) -> (String, Option<String>) {
    match group {
        Some(group) => (group, Some(peer)),
        None => (peer, None),
    }
}

impl Listener {
    /// Stores something sent to us and signals it if unseen.
    fn incoming(&self, key: String, message: Message) {
        let shared = self.app.state::<Shared>();
        let window = self.app.get_webview_window(&chat_label(&key));
        let seen = window
            .as_ref()
            .is_some_and(|window| window.is_focused().unwrap_or(false));
        let fresh = shared.store.lock().unwrap().receive(&key, message, seen);
        if !fresh {
            return;
        }
        if !seen {
            self.alert(&key, window);
        }
        notify_changed(&self.app, key, "incoming");
    }

    /// Tells the person about a message they have not seen: a system
    /// notification without the text, so nothing leaks onto the screen,
    /// and a highlight on the window in the taskbar.
    fn alert(&self, key: &str, chat_window: Option<tauri::WebviewWindow>) {
        let shared = self.app.state::<Shared>();
        let body = match title_of(&shared, key) {
            Some(title) if is_group(key) => format!("Nowa wiadomość w grupie: {title}"),
            Some(title) => format!("Nowa wiadomość od: {title}"),
            None => "Nowa wiadomość".into(),
        };
        let _ = self
            .app
            .notification()
            .builder()
            .title("Szept")
            .body(body)
            .show();
        let window = chat_window.or_else(|| self.app.get_webview_window("main"));
        if let Some(window) = window {
            let _ = window.request_user_attention(Some(UserAttentionType::Informational));
        }
    }
}

fn notify_changed(app: &AppHandle, peer: String, kind: &'static str) {
    let _ = app.emit("changed", Changed { peer, kind });
}

/// The window title of a conversation: the contact's alias or the group's name.
fn title_of(shared: &Shared, key: &str) -> Option<String> {
    if is_group(key) {
        find_group(&shared.core(), key).map(|group| group.name)
    } else {
        alias_of(shared, key)
    }
}

fn alias_of(shared: &Shared, peer: &str) -> Option<String> {
    shared
        .core()
        .contacts()
        .into_iter()
        .find(|contact| contact.id == peer)
        .map(|contact| contact.alias)
}

/// Window labels allow letters, digits, `-` and `_`: the hex id fits.
fn chat_label(peer: &str) -> String {
    format!("chat-{peer}")
}

fn qr_svg(text: &str) -> CommandResult<String> {
    let code = qr_code(text.to_owned()).map_err(failed)?;
    let size = code.size as usize;
    // A quiet zone of four modules, as the QR standard asks for.
    let total = size + 8;
    let mut path = String::new();
    for (index, _) in code.modules.iter().enumerate().filter(|(_, dark)| **dark) {
        let (x, y) = (index % size + 4, index / size + 4);
        path.push_str(&format!("M{x} {y}h1v1h-1z"));
    }
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {total} {total}\" \
         shape-rendering=\"crispEdges\" role=\"img\" \
         aria-label=\"Kod QR z Twoim identyfikatorem\">\
         <rect width=\"{total}\" height=\"{total}\" fill=\"#fff\"/>\
         <path d=\"{path}\" fill=\"#1b2140\"/></svg>"
    ))
}

/// WebKitGTK keeps the camera and the microphone off unless asked, and then
/// asks the app about every request. The scanner in the main window uses the
/// camera and the conversation windows the microphone, each only after the
/// person presses the button.
#[cfg(target_os = "linux")]
fn allow_media(window: &tauri::WebviewWindow) {
    use webkit2gtk::{
        PermissionRequestExt, SettingsExt, UserMediaPermissionRequest, WebViewExt,
        glib::object::ObjectExt,
    };
    let _ = window.with_webview(|webview| {
        let view = webview.inner();
        if let Some(settings) = WebViewExt::settings(&view) {
            settings.set_enable_media_stream(true);
        }
        view.connect_permission_request(|_, request| {
            if request.is::<UserMediaPermissionRequest>() {
                request.allow();
                true
            } else {
                false
            }
        });
    });
}

#[cfg(not(target_os = "linux"))]
fn allow_media(_window: &tauri::WebviewWindow) {}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let core = App::new(data_dir.to_string_lossy().into_owned(), false)?;
            // Offers end with the app, so last session's recordings can go.
            let outgoing = data_dir.join("outgoing");
            let _ = std::fs::remove_dir_all(&outgoing);
            app.manage(Shared {
                core: RwLock::new(core.clone()),
                store: Mutex::new(Store::default()),
                outgoing,
                data_dir: data_dir.clone(),
            });
            core.start(Arc::new(Listener {
                app: app.handle().clone(),
            }))?;
            if let Some(main) = app.get_webview_window("main") {
                allow_media(&main);
            }
            // Pictures and music already received can be shown and played,
            // and so can voice messages and GIFs.
            if let Ok(dir) = downloads_dir(app.handle()) {
                let _ = app.asset_protocol_scope().allow_directory(dir, true);
            }
            let media = PathBuf::from(core.media_dir());
            let _ = std::fs::create_dir_all(&media);
            let _ = app.asset_protocol_scope().allow_directory(media, true);
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Closing the contact list closes the whole app.
            WindowEvent::CloseRequested { .. } if window.label() == "main" => {
                window.app_handle().exit(0);
            }
            // Files dropped onto a conversation are offered to that contact.
            WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) => {
                if let Some(peer) = window.label().strip_prefix("chat-") {
                    send_paths(window.app_handle().clone(), peer.to_owned(), paths.clone());
                }
            }
            // Looking at a conversation counts as reading it.
            WindowEvent::Focused(true) => {
                if let Some(peer) = window.label().strip_prefix("chat-") {
                    let app = window.app_handle();
                    mark_read(app.clone(), app.state(), peer.to_owned());
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            overview,
            add_contact,
            remove_contact,
            open_chat,
            chat,
            mark_read,
            send,
            send_files,
            send_media,
            set_avatar,
            set_name,
            create_group,
            update_group,
            leave_group,
            update_contact_key,
            send_sticker,
            react,
            export_archive,
            import_archive_file,
            password_strength,
            suggest_passphrase,
            save_settings_file,
            load_settings_file,
            download,
            open_file,
            reveal,
            normalize_id,
        ])
        .build(tauri::generate_context!())
        .expect("cannot start the application")
        .run(|app, event| {
            if let RunEvent::Exit = event
                && let Some(shared) = app.try_state::<Shared>()
            {
                shared.core().stop();
            }
        });
}
