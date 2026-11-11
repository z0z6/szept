//! Szept on the desktop: a window with the contact list and one window per
//! conversation. The Rust core runs in this process; the windows are web
//! views that talk to it through the commands below.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod store;

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use komunikator_ffi::{App, AppError, ChatEvent, EventListener, file_kind, qr_code, share_link};
use serde::Serialize;
use tauri::{
    AppHandle, DragDropEvent, Emitter, Manager, RunEvent, State, UserAttentionType, WebviewUrl,
    WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

use crate::store::{FileInfo, Message, Status, Store, Transfer};

/// Everything the commands share.
struct Shared {
    core: Arc<App>,
    store: Mutex<Store>,
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
}

#[derive(Serialize)]
struct Overview {
    my_id: String,
    link: String,
    qr_svg: String,
    contacts: Vec<ContactView>,
}

#[derive(Serialize)]
struct ChatView {
    alias: String,
    id: String,
    online: bool,
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
            alias: contact.alias,
            id: contact.id,
        })
        .collect();
    Ok(Overview {
        my_id,
        link,
        qr_svg,
        contacts,
    })
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
    WebviewWindowBuilder::new(&app, label, url)
        .title(format!("{alias} – Szept"))
        .inner_size(420.0, 620.0)
        .min_inner_size(320.0, 380.0)
        .build()
        .map_err(|err| err.to_string())?;
    Ok(())
}

#[tauri::command]
fn chat(shared: State<'_, Shared>, peer: String) -> CommandResult<ChatView> {
    let alias = alias_of(&shared, &peer).ok_or("ten kontakt został usunięty")?;
    let messages = shared.store.lock().unwrap().conversation(&peer);
    Ok(ChatView {
        online: shared.core.is_online(peer.clone()),
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
        let shared = app.state::<Shared>();
        for path in paths {
            match shared
                .core
                .send_file(peer.clone(), path.to_string_lossy().into_owned())
            {
                Ok(sent) => {
                    // Let the windows show pictures and play music we send.
                    let _ = app.asset_protocol_scope().allow_file(&path);
                    let file = FileInfo {
                        kind: file_kind(sent.name.clone()),
                        name: sent.name,
                        size: sent.size,
                        hash: sent.hash,
                        transfer: Transfer::Shared,
                        done: sent.size,
                        path: Some(path.to_string_lossy().into_owned()),
                        reason: None,
                    };
                    shared
                        .store
                        .lock()
                        .unwrap()
                        .push(&peer, Message::outgoing_file(sent.id, file));
                    notify_changed(&app, peer.clone(), "other");
                }
                Err(err) => {
                    let name = path.file_name().map_or_else(
                        || path.display().to_string(),
                        |name| name.to_string_lossy().into_owned(),
                    );
                    let _ = app.emit(
                        "problem",
                        Problem {
                            peer: peer.clone(),
                            text: format!("Nie udało się wysłać {name}: {err}"),
                        },
                    );
                }
            }
        }
    });
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
            } => {
                let file = FileInfo {
                    kind: file_kind(name.clone()),
                    name,
                    size,
                    hash,
                    transfer: Transfer::Offered,
                    done: 0,
                    path: None,
                    reason: None,
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
                let changed = shared
                    .store
                    .lock()
                    .unwrap()
                    .set_transfer(&peer, &id, |file| {
                        file.transfer = Transfer::Saved;
                        file.done = file.size;
                        file.path = Some(path);
                    });
                if changed {
                    notify_changed(&self.app, peer, "saved");
                }
            }
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

/// WebKitGTK keeps the camera off unless asked, and then asks the app about
/// every request. The scanner in the main window is the only page that uses
/// it, and only after the person presses the button.
#[cfg(target_os = "linux")]
fn allow_camera(window: &tauri::WebviewWindow) {
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
fn allow_camera(_window: &tauri::WebviewWindow) {}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let core = App::new(data_dir.to_string_lossy().into_owned(), false)?;
            app.manage(Shared {
                core: core.clone(),
                store: Mutex::new(Store::default()),
            });
            core.start(Arc::new(Listener {
                app: app.handle().clone(),
            }))?;
            if let Some(main) = app.get_webview_window("main") {
                allow_camera(&main);
            }
            // Pictures and music already received can be shown and played.
            if let Ok(dir) = downloads_dir(app.handle()) {
                let _ = app.asset_protocol_scope().allow_directory(dir, true);
            }
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
