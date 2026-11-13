//! Drives two `App`s exactly as the Kotlin side does.

use std::{
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

use komunikator_ffi::{
    App, ChatEvent, EventListener, Media, max_avatar_len, normalize_id, qr_code, share_link,
};

const WAIT: Duration = Duration::from_secs(20);

struct Collector(Mutex<mpsc::Sender<ChatEvent>>);

impl EventListener for Collector {
    fn on_event(&self, event: ChatEvent) {
        let _ = self.0.lock().unwrap().send(event);
    }
}

fn new_app() -> (Arc<App>, mpsc::Receiver<ChatEvent>) {
    let dir = std::env::temp_dir().join(format!("komunikator-ffi-{}", rand::random::<u64>()));
    let app = App::new(dir.to_str().unwrap().to_owned(), true).unwrap();
    let (events, events_rx) = mpsc::channel();
    app.start(Arc::new(Collector(Mutex::new(events)))).unwrap();
    (app, events_rx)
}

#[test]
fn queued_message_is_delivered_after_connecting() {
    let (ala, ala_events) = new_app();
    let (ola, ola_events) = new_app();
    ala.add_contact("ola".into(), ola.my_id()).unwrap();
    ola.add_contact("ala".into(), format!("  {}\n", ala.my_id()))
        .unwrap();
    assert_eq!(ala.contacts()[0].alias, "ola");
    assert!(ala.add_contact("ja".into(), ala.my_id()).is_err());
    assert!(
        ala.add_contact("zly".into(), "to nie jest klucz".into())
            .is_err()
    );

    // Written while Ola is unreachable: the message waits in the queue.
    let id = ala.send(ola.my_id(), "Cześć!".into()).unwrap();
    assert!(!ala.is_online(ola.my_id()));

    let address = format!("127.0.0.1:{}", ola.listening_port());
    ala.connect_direct(ola.my_id(), address).unwrap();

    assert!(matches!(
        ola_events.recv_timeout(WAIT).unwrap(),
        ChatEvent::Connected { peer } if peer == ala.my_id()
    ));
    match ola_events.recv_timeout(WAIT).unwrap() {
        ChatEvent::Message {
            peer,
            id: got,
            body,
            ..
        } => {
            assert_eq!(
                (peer, got, body),
                (ala.my_id(), id.clone(), "Cześć!".into())
            );
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(matches!(
        ala_events.recv_timeout(WAIT).unwrap(),
        ChatEvent::Connected { .. }
    ));
    assert!(matches!(
        ala_events.recv_timeout(WAIT).unwrap(),
        ChatEvent::Sent { id: got, .. } if got == id
    ));
    assert!(matches!(
        ala_events.recv_timeout(WAIT).unwrap(),
        ChatEvent::Delivered { id: got, .. } if got == id
    ));
    assert!(ala.is_online(ola.my_id()));

    // Removing a contact cuts the connection.
    ola.remove_contact("ala".into()).unwrap();
    assert!(matches!(
        ala_events.recv_timeout(WAIT).unwrap(),
        ChatEvent::Disconnected { .. }
    ));

    ala.stop();
    ola.stop();
    assert!(ala.send(ola.my_id(), "za późno".into()).is_err());
}

#[test]
fn identifiers_travel_through_qr_links() {
    let (ala, _events) = new_app();
    let link = share_link(ala.my_id());
    assert!(link.starts_with("szept:"));
    assert_eq!(normalize_id(link.clone()).unwrap(), ala.my_id());
    assert_eq!(
        normalize_id(format!("  SZEPT: {}\n", ala.my_id())).unwrap(),
        ala.my_id()
    );
    assert!(normalize_id("szept:".into()).is_err());

    let code = qr_code(link).unwrap();
    assert_eq!(code.modules.len(), (code.size * code.size) as usize);
    assert!(code.modules.iter().any(|dark| *dark));

    // A contact can be added straight from the scanned link.
    let (ola, _ola_events) = new_app();
    ola.add_contact("ala".into(), share_link(ala.my_id()))
        .unwrap();
    assert_eq!(ola.contacts()[0].id, ala.my_id());
    ala.stop();
    ola.stop();
}

#[test]
fn a_file_goes_from_one_app_to_the_other() {
    let (ala, ala_events) = new_app();
    let (ola, ola_events) = new_app();
    ala.add_contact("ola".into(), ola.my_id()).unwrap();
    ola.add_contact("ala".into(), ala.my_id()).unwrap();
    let address = format!("127.0.0.1:{}", ola.listening_port());
    ala.connect_direct(ola.my_id(), address).unwrap();

    let dir = std::env::temp_dir().join(format!("komunikator-ffi-file-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("zdjęcie wakacje.jpg");
    let content: Vec<u8> = (0..700_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&source, &content).unwrap();

    let sent = ala
        .send_file(ola.my_id(), source.to_string_lossy().into_owned())
        .unwrap();
    assert_eq!(
        (sent.name.as_str(), sent.size),
        ("zdjęcie wakacje.jpg", 700_000)
    );

    let offer = loop {
        match ola_events.recv_timeout(WAIT).unwrap() {
            ChatEvent::FileOffered {
                id,
                name,
                size,
                hash,
                ..
            } => break (id, name, size, hash),
            _ => continue,
        }
    };
    assert_eq!(offer.0, sent.id);
    assert_eq!(
        (offer.1.as_str(), offer.2),
        ("zdjęcie wakacje.jpg", 700_000)
    );
    assert_eq!(offer.3, sent.hash);
    loop {
        if let ChatEvent::Delivered { id, .. } = ala_events.recv_timeout(WAIT).unwrap()
            && id == sent.id
        {
            break;
        }
    }

    let downloads = dir.join("pobrane");
    ola.download(sent.id.clone(), downloads.to_string_lossy().into_owned())
        .unwrap();
    let mut saw_progress = false;
    let saved = loop {
        match ola_events.recv_timeout(WAIT).unwrap() {
            ChatEvent::FileProgress { total, .. } => {
                assert_eq!(total, 700_000);
                saw_progress = true;
            }
            ChatEvent::FileSaved { id, path, .. } => {
                assert_eq!(id, sent.id);
                break path;
            }
            ChatEvent::FileFailed { reason, .. } => panic!("download failed: {reason}"),
            _ => {}
        }
    };
    assert!(saw_progress);
    assert_eq!(std::fs::read(&saved).unwrap(), content);
    assert!(ola.download("nieznany".into(), String::new()).is_err());

    ala.stop();
    ola.stop();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn messages_keep_flowing_after_a_network_change() {
    let (ala, ala_events) = new_app();
    let (ola, ola_events) = new_app();
    ala.add_contact("ola".into(), ola.my_id()).unwrap();
    ola.add_contact("ala".into(), ala.my_id()).unwrap();
    let address = format!("127.0.0.1:{}", ola.listening_port());
    ala.connect_direct(ola.my_id(), address).unwrap();
    loop {
        if let ChatEvent::Connected { .. } = ala_events.recv_timeout(WAIT).unwrap() {
            break;
        }
    }

    // As when a phone moves from Wi-Fi to mobile data.
    ala.network_changed();
    ola.network_changed();

    let id = ala.send(ola.my_id(), "nadal tu jestem".into()).unwrap();
    loop {
        if let ChatEvent::Message { id: got, .. } = ola_events.recv_timeout(WAIT).unwrap()
            && got == id
        {
            break;
        }
    }
    loop {
        if let ChatEvent::Delivered { id: got, .. } = ala_events.recv_timeout(WAIT).unwrap()
            && got == id
        {
            break;
        }
    }
    ala.stop();
    ola.stop();
}

#[test]
fn voice_messages_arrive_on_their_own_and_avatars_follow_contacts() {
    let (ala, _ala_events) = new_app();
    let (ola, ola_events) = new_app();
    ala.add_contact("ola".into(), ola.my_id()).unwrap();
    ola.add_contact("ala".into(), ala.my_id()).unwrap();

    // Set while offline: goes out with the first connection.
    let picture = b"\xff\xd8\xff\xe0 a tiny jpeg".to_vec();
    assert!(ala.set_avatar(b"<svg/>".to_vec()).is_err());
    assert!(
        ala.set_avatar(vec![0xff; max_avatar_len() as usize + 1])
            .is_err()
    );
    ala.set_avatar(picture.clone()).unwrap();
    assert_eq!(ala.my_avatar(), Some(picture.clone()));

    let address = format!("127.0.0.1:{}", ola.listening_port());
    ala.connect_direct(ola.my_id(), address).unwrap();
    loop {
        if let ChatEvent::AvatarChanged { peer } = ola_events.recv_timeout(WAIT).unwrap() {
            assert_eq!(peer, ala.my_id());
            break;
        }
    }
    assert_eq!(ola.avatar(ala.my_id()), Some(picture));

    let dir = std::env::temp_dir().join(format!("komunikator-ffi-voice-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("glosowa.m4a");
    std::fs::write(&source, b"....ftypM4A and some sound").unwrap();
    let media = Media::Voice { duration_ms: 3100 };
    let sent = ala
        .send_media(ola.my_id(), source.to_string_lossy().into_owned(), media)
        .unwrap();

    // No call to download: the app fetches voice messages by itself.
    let saved = loop {
        match ola_events.recv_timeout(WAIT).unwrap() {
            ChatEvent::FileOffered { id, media: got, .. } => {
                assert_eq!(id, sent.id);
                assert_eq!(got, Some(media));
            }
            ChatEvent::FileSaved { id, path, .. } => {
                assert_eq!(id, sent.id);
                break path;
            }
            ChatEvent::FileFailed { reason, .. } => panic!("download failed: {reason}"),
            _ => {}
        }
    };
    assert!(saved.starts_with(&ola.media_dir()), "{saved}");
    assert_eq!(
        std::fs::read(&saved).unwrap(),
        b"....ftypM4A and some sound"
    );

    // Removing the picture reaches the contact too.
    ala.set_avatar(Vec::new()).unwrap();
    assert_eq!(ala.my_avatar(), None);
    loop {
        if let ChatEvent::AvatarChanged { .. } = ola_events.recv_timeout(WAIT).unwrap() {
            break;
        }
    }
    assert_eq!(ola.avatar(ala.my_id()), None);

    ala.stop();
    ola.stop();
    let _ = std::fs::remove_dir_all(dir);
}

/// Waits for the first event `pick` accepts, skipping the others.
fn wait_for<T>(
    events: &mpsc::Receiver<ChatEvent>,
    mut pick: impl FnMut(ChatEvent) -> Option<T>,
) -> T {
    let deadline = std::time::Instant::now() + WAIT;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let event = events.recv_timeout(left).expect("the event never came");
        if let Some(found) = pick(event) {
            return found;
        }
    }
}

fn dial(from: &App, to: &App) {
    let address = format!("127.0.0.1:{}", to.listening_port());
    from.connect_direct(to.my_id(), address).unwrap();
}

#[test]
fn groups_reach_members_who_are_not_contacts() {
    let (ala, ala_events) = new_app();
    let (ola, ola_events) = new_app();
    let (ela, ela_events) = new_app();
    // Ala knows both; Ola and Ela do not know each other.
    ala.add_contact("ola".into(), ola.my_id()).unwrap();
    ala.add_contact("ela".into(), ela.my_id()).unwrap();
    ola.add_contact("ala".into(), ala.my_id()).unwrap();
    ela.add_contact("ala".into(), ala.my_id()).unwrap();
    ala.set_name("Ala".into()).unwrap();
    assert!(ala.create_group("Nikt".into(), vec![]).is_err());

    let group = ala
        .create_group("Wycieczka".into(), vec![ola.my_id(), ela.my_id()])
        .unwrap();
    assert!(group.mine);
    assert_eq!(group.members.len(), 3);
    dial(&ala, &ola);
    dial(&ala, &ela);
    for events in [&ola_events, &ela_events] {
        let id = wait_for(events, |event| match event {
            ChatEvent::GroupChanged { group } => Some(group),
            _ => None,
        });
        assert_eq!(id, group.id);
    }
    assert_eq!(ola.groups()[0].name, "Wycieczka");
    assert!(!ola.groups()[0].mine);
    assert_eq!(ola.display_name(ala.my_id()).as_deref(), Some("Ala"));

    // Ola writes to the group; Ela gets it straight from her.
    dial(&ola, &ela);
    let sent = ola
        .send(group.id.clone(), "Kto bierze kanapki?".into())
        .unwrap();
    let (author, body) = wait_for(&ela_events, |event| match event {
        ChatEvent::Message {
            peer,
            body,
            group: Some(g),
            ..
        } if g == group.id => Some((peer, body)),
        _ => None,
    });
    assert_eq!(
        (author, body.as_str()),
        (ola.my_id(), "Kto bierze kanapki?")
    );
    wait_for(&ala_events, |event| match event {
        ChatEvent::Message { id, .. } if id == sent => Some(()),
        _ => None,
    });
    wait_for(&ola_events, |event| match event {
        ChatEvent::Delivered {
            id, group: Some(g), ..
        } if id == sent && g == group.id => Some(()),
        _ => None,
    });

    // A reaction and a sticker.
    ela.react(group.id.clone(), sent.clone(), "pierog".into())
        .unwrap();
    let (target, reaction) = wait_for(&ola_events, |event| match event {
        ChatEvent::Reaction {
            target,
            reaction,
            peer,
            ..
        } if peer == ela.my_id() => Some((target, reaction)),
        _ => None,
    });
    assert_eq!((target, reaction.as_str()), (sent.clone(), "pierog"));
    assert!(
        ela.react(group.id.clone(), sent, "Zła Reakcja".into())
            .is_err()
    );
    ela.send_sticker(group.id.clone(), "ogorek".into()).unwrap();
    let code = wait_for(&ala_events, |event| match event {
        ChatEvent::Sticker {
            code,
            group: Some(_),
            ..
        } => Some(code),
        _ => None,
    });
    assert_eq!(code, "ogorek");

    // Only the owner changes the group; Ela is taken out.
    assert!(
        ola.update_group(group.id.clone(), "Moja".into(), vec![])
            .is_err()
    );
    ala.update_group(group.id.clone(), "Wycieczka 2".into(), vec![ola.my_id()])
        .unwrap();
    let reason = wait_for(&ela_events, |event| match event {
        ChatEvent::GroupClosed { reason, .. } => Some(reason),
        _ => None,
    });
    assert_eq!(reason, "removed");
    assert!(ela.groups().is_empty());
    wait_for(&ola_events, |event| match event {
        ChatEvent::GroupChanged { .. } => Some(()),
        _ => None,
    });
    assert_eq!(ola.groups()[0].name, "Wycieczka 2");

    // The owner leaving closes the group.
    ala.leave_group(group.id.clone()).unwrap();
    let reason = wait_for(&ola_events, |event| match event {
        ChatEvent::GroupClosed { reason, .. } => Some(reason),
        _ => None,
    });
    assert_eq!(reason, "closed");

    for app in [ala, ola, ela] {
        app.stop();
    }
}

#[test]
fn a_contact_can_move_to_a_new_key() {
    let (ala, _ala_events) = new_app();
    let (ola, _ola_events) = new_app();
    let (new_ola, new_ola_events) = new_app();
    ala.add_contact("ola".into(), ola.my_id()).unwrap();
    new_ola.add_contact("ala".into(), ala.my_id()).unwrap();

    assert!(ala.update_contact_key("ola".into(), ala.my_id()).is_err());
    assert!(
        ala.update_contact_key("nikt".into(), new_ola.my_id())
            .is_err()
    );
    let old = ala
        .update_contact_key("ola".into(), new_ola.my_id())
        .unwrap();
    assert_eq!(old, ola.my_id());
    assert_eq!(ala.contacts()[0].id, new_ola.my_id());

    // The new key works right away.
    ala.send(new_ola.my_id(), "Witaj z powrotem".into())
        .unwrap();
    dial(&ala, &new_ola);
    let body = wait_for(&new_ola_events, |event| match event {
        ChatEvent::Message { body, .. } => Some(body),
        _ => None,
    });
    assert_eq!(body, "Witaj z powrotem");
    for app in [ala, ola, new_ola] {
        app.stop();
    }
}

#[test]
fn an_archive_brings_back_identity_contacts_and_conversations() {
    use komunikator_ffi::{
        ArchivedConversation, ArchivedMessage, ArchivedReaction, import_archive,
    };

    let dir = std::env::temp_dir().join(format!("komunikator-arch-{}", rand::random::<u64>()));
    let (ala, _events) = new_app();
    let (ola, _ola_events) = new_app();
    ala.add_contact("ola".into(), ola.my_id()).unwrap();
    ala.set_name("Ala".into()).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let voice = dir.join("glosowa.m4a");
    std::fs::write(&voice, b"....ftypM4A sound").unwrap();
    let conversation = ArchivedConversation {
        key: ola.my_id(),
        messages: vec![ArchivedMessage {
            id: "00112233445566778899aabbccddeeff".into(),
            author: String::new(),
            at_ms: 1,
            kind: "file".into(),
            body: String::new(),
            file_name: Some("glosowa.m4a".into()),
            file_size: 17,
            file_hash: None,
            media: Some(Media::Voice { duration_ms: 1200 }),
            file_path: Some(voice.to_string_lossy().into_owned()),
            status: "delivered".into(),
            reactions: vec![ArchivedReaction {
                who: ola.my_id(),
                reaction: "kapec".into(),
            }],
        }],
    };
    let archive = dir.join("szept.szarch");
    let archive_text = archive.to_string_lossy().into_owned();
    assert!(
        ala.export_archive(
            archive_text.clone(),
            "krótkie".into(),
            true,
            vec![],
            Default::default()
        )
        .is_err()
    );
    // Long but easy to guess: refused.
    assert!(
        ala.export_archive(
            archive_text.clone(),
            "qwerty123456".into(),
            true,
            vec![],
            Default::default()
        )
        .is_err()
    );
    let preferences = [
        ("theme".to_owned(), "las".to_owned()),
        ("zły klucz!".to_owned(), "x".to_owned()),
    ]
    .into_iter()
    .collect();
    ala.export_archive(
        archive_text.clone(),
        "zielony-most-pada-rzeka-17".into(),
        true,
        vec![conversation],
        preferences,
    )
    .unwrap();

    // A fresh install: a new identity, nothing else.
    let fresh = dir.join("fresh");
    let reinstalled = App::new(fresh.to_string_lossy().into_owned(), true).unwrap();
    assert_ne!(reinstalled.my_id(), ala.my_id());
    drop(reinstalled);

    let fresh_text = fresh.to_string_lossy().into_owned();
    assert!(
        import_archive(
            fresh_text.clone(),
            archive_text.clone(),
            "złe hasło".into(),
            true
        )
        .is_err()
    );
    let imported = import_archive(
        fresh_text.clone(),
        archive_text,
        "zielony-most-pada-rzeka-17".into(),
        true,
    )
    .unwrap();
    assert!(imported.identity_restored);
    // Only well-formed preferences come back.
    assert_eq!(imported.preferences.len(), 1);
    assert_eq!(
        imported.preferences.get("theme").map(String::as_str),
        Some("las")
    );
    assert_eq!(imported.contacts_added, 1);
    let message = &imported.conversations[0].messages[0];
    assert_eq!(message.reactions[0].reaction, "kapec");
    let restored = message.file_path.clone().unwrap();
    assert_eq!(std::fs::read(&restored).unwrap(), b"....ftypM4A sound");

    let back = App::new(fresh_text, true).unwrap();
    assert_eq!(back.my_id(), ala.my_id());
    assert_eq!(back.contacts()[0].id, ola.my_id());
    assert_eq!(back.my_name(), "Ala");
    ala.stop();
    ola.stop();
    let _ = std::fs::remove_dir_all(dir);
}
