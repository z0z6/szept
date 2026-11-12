//! Szept on the desktop: a window with the contact list and one window per
//! conversation. The Rust core runs in this process; the windows are web
//! views that talk to it through the commands below.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod store;

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use base64::Engine as _;
use komunikator_ffi::{
    App, AppError, ChatEvent, EventListener, Media, file_kind, max_media_size, qr_code, share_link,
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
    core: Arc<App>,
    store: Mutex<Store>,
    /// Voice messages and pasted GIFs we send wait here until the app
    /// closes: the contact fetches them from this folder.
    outgoing: PathBuf,
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
struct Overview {
    my_id: String,
    link: String,
    qr_svg: String,
    /// Our own profile picture as a `data:` URL.
    my_avatar: Option<String>,
    contacts: Vec<ContactView>,
}

#[derive(Serialize)]
struct ChatView {
    alias: String,
    id: String,
    online: bool,
    avatar: Option<String>,
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
    let my_id = shared.core.my_id();
    let link = share_link(my_id.clone());
    let qr_svg = qr_svg(&link)?;
    let store = shared.store.lock().unwrap();
    let contacts = shared
        .core
        .contacts()
        .into_iter()
        .map(|contact| ContactView {
            online: shared.core.is_online(contact.id.clone()),
            unread: store.unread(&contact.id),
            last: store.last_body(&contact.id),
            avatar: data_url(shared.core.avatar(contact.id.clone())),
            alias: contact.alias,
            id: contact.id,
        })
        .collect();
    Ok(Overview {
        my_id,
        link,
        qr_svg,
        my_avatar: data_url(shared.core.my_avatar()),
        contacts,
    })
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
    shared.core.set_avatar(image).map_err(failed)?;
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
    shared.core.add_contact(alias, id).map_err(failed)?;
    notify_changed(&app, String::new(), "other");
    Ok(())
}

#[tauri::command]
fn remove_contact(app: AppHandle, shared: State<'_, Shared>, alias: String) -> CommandResult<()> {
    let id = shared
        .core
        .contacts()
        .into_iter()
        .find(|contact| contact.alias == alias)
        .map(|contact| contact.id);
    shared.core.remove_contact(alias).map_err(failed)?;
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
    let alias = alias_of(&shared, &peer).ok_or("nieznany kontakt")?;
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
    let alias = alias_of(&shared, &peer).ok_or("ten kontakt został usunięty")?;
    let messages = shared.store.lock().unwrap().conversation(&peer);
    Ok(ChatView {
        online: shared.core.is_online(peer.clone()),
        avatar: data_url(shared.core.avatar(peer.clone())),
        alias,
        id: peer,
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
    // The core may report "sent" right away, on another thread. Holding the
    // lock until the message is stored makes that report wait for it.
    {
        let mut store = shared.store.lock().unwrap();
        let id = shared
            .core
            .send(peer.clone(), body.clone())
            .map_err(failed)?;
        store.push(&peer, Message::outgoing(id, body));
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
    let text_path = path.to_string_lossy().into_owned();
    let result = match media {
        None => shared.core.send_file(peer.to_owned(), text_path.clone()),
        Some(media) => shared
            .core
            .send_media(peer.to_owned(), text_path.clone(), media),
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
            shared
                .store
                .lock()
                .unwrap()
                .push(peer, Message::outgoing_file(sent.id, file));
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
    alias_of(&shared, &peer).ok_or("nieznany kontakt")?;
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
    let dir = downloads_dir(&app)?;
    shared
        .core
        .download(id.clone(), dir.to_string_lossy().into_owned())
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
            ChatEvent::Message {
                peer,
                id,
                sent_at_ms,
                body,
            } => {
                let message = Message {
                    id,
                    mine: false,
                    body,
                    at_ms: sent_at_ms,
                    status: Status::Received,
                    file: None,
                };
                self.incoming(peer, message);
            }
            ChatEvent::FileOffered {
                peer,
                id,
                sent_at_ms,
                name,
                size,
                hash,
                media,
            } => {
                let (media, duration_ms) = describe_media(media);
                let file = FileInfo {
                    kind: file_kind(name.clone()),
                    name,
                    size,
                    hash,
                    transfer: Transfer::Offered,
                    done: 0,
                    path: None,
                    reason: None,
                    media,
                    duration_ms,
                };
                let message = Message {
                    id,
                    mine: false,
                    body: String::new(),
                    at_ms: sent_at_ms,
                    status: Status::Received,
                    file: Some(file),
                };
                self.incoming(peer, message);
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
                    .set_transfer(&peer, &id, |file| {
                        file.transfer = Transfer::Downloading;
                        file.done = done;
                    });
                if changed {
                    notify_changed(&self.app, peer, "other");
                }
            }
            ChatEvent::FileSaved { peer, id, path } => {
                let _ = self.app.asset_protocol_scope().allow_file(&path);
                let mut media = false;
                let changed = shared
                    .store
                    .lock()
                    .unwrap()
                    .set_transfer(&peer, &id, |file| {
                        file.transfer = Transfer::Saved;
                        file.done = file.size;
                        file.path = Some(path);
                        media = file.media.is_some();
                    });
                if changed {
                    // Media arrive on their own: their message already chimed.
                    notify_changed(&self.app, peer, if media { "other" } else { "saved" });
                }
            }
            ChatEvent::AvatarChanged { peer } => notify_changed(&self.app, peer, "other"),
            ChatEvent::FileFailed { peer, id, reason } => {
                let changed = shared
                    .store
                    .lock()
                    .unwrap()
                    .set_transfer(&peer, &id, |file| {
                        file.transfer = Transfer::Failed;
                        file.reason = Some(reason);
                    });
                if changed {
                    notify_changed(&self.app, peer, "other");
                }
            }
            ChatEvent::Sent { peer, id } => {
                let changed = shared
                    .store
                    .lock()
                    .unwrap()
                    .set_status(&peer, &id, Status::Sent);
                if changed {
                    notify_changed(&self.app, peer, "sent");
                }
            }
            ChatEvent::Delivered { peer, id } => {
                let changed =
                    shared
                        .store
                        .lock()
                        .unwrap()
                        .set_status(&peer, &id, Status::Delivered);
                if changed {
                    notify_changed(&self.app, peer, "delivered");
                }
            }
        }
    }
}

impl Listener {
    /// Stores something the contact sent and signals it if unseen.
    fn incoming(&self, peer: String, message: Message) {
        let shared = self.app.state::<Shared>();
        let window = self.app.get_webview_window(&chat_label(&peer));
        let seen = window
            .as_ref()
            .is_some_and(|window| window.is_focused().unwrap_or(false));
        let fresh = shared.store.lock().unwrap().receive(&peer, message, seen);
        if !fresh {
            return;
        }
        if !seen {
            self.alert(&peer, window);
        }
        notify_changed(&self.app, peer, "incoming");
    }

    /// Tells the person about a message they have not seen: a system
    /// notification without the text, so nothing leaks onto the screen,
    /// and a highlight on the window in the taskbar.
    fn alert(&self, peer: &str, chat_window: Option<tauri::WebviewWindow>) {
        let shared = self.app.state::<Shared>();
        let alias = alias_of(&shared, peer).unwrap_or_else(|| "kontakt".into());
        let _ = self
            .app
            .notification()
            .builder()
            .title("Szept")
            .body(format!("Nowa wiadomość od: {alias}"))
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

fn alias_of(shared: &Shared, peer: &str) -> Option<String> {
    shared
        .core
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
                core: core.clone(),
                store: Mutex::new(Store::default()),
                outgoing,
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
                shared.core.stop();
            }
        });
}
