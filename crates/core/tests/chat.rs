//! End-to-end tests: real nodes talking over the loopback interface, without
//! any outside service.

use std::{net::Ipv4Addr, path::PathBuf, time::Duration};

use komunikator_core::{EndpointAddr, Event, Identity, Network, Node, Outgoing, identity};
use tokio::{sync::mpsc::Receiver, time::timeout};

const WAIT: Duration = Duration::from_secs(20);

fn temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("komunikator-test-{}", rand::random::<u64>()))
}

fn new_identity() -> Identity {
    Identity::create(&identity::default_path(&temp_dir())).unwrap()
}

/// The address of `node` on the loopback interface.
fn loopback(node: &Node) -> EndpointAddr {
    let port = node
        .bound_sockets()
        .into_iter()
        .find(|socket| socket.is_ipv4())
        .expect("node listens on IPv4")
        .port();
    EndpointAddr::new(node.id()).with_ip_addr((Ipv4Addr::LOCALHOST, port).into())
}

async fn next(events: &mut Receiver<Event>) -> Event {
    timeout(WAIT, events.recv())
        .await
        .expect("timed out waiting for an event")
        .expect("event stream ended")
}

#[tokio::test]
async fn two_contacts_exchange_messages() {
    let (ala_identity, ola_identity) = (new_identity(), new_identity());
    let (ala_id, ola_id) = (ala_identity.id(), ola_identity.id());

    let (ala, mut ala_events) = Node::spawn(&ala_identity, [ola_id], Network::LocalOnly)
        .await
        .unwrap();
    let (ola, mut ola_events) = Node::spawn(&ola_identity, [ala_id], Network::LocalOnly)
        .await
        .unwrap();

    ala.connect(loopback(&ola)).await.unwrap();
    assert!(matches!(next(&mut ala_events).await, Event::Connected { peer } if peer == ola_id));
    assert!(matches!(next(&mut ola_events).await, Event::Connected { peer } if peer == ala_id));

    // Ala writes, Ola receives, Ala gets the delivery confirmation.
    let hello = Outgoing::new("Cześć, Olu!").unwrap();
    ala.send(ola_id, &hello).await.unwrap();
    match next(&mut ola_events).await {
        Event::Message { peer, id, body, .. } => {
            assert_eq!(peer, ala_id);
            assert_eq!(id, hello.id);
            assert_eq!(body, "Cześć, Olu!");
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(matches!(
        next(&mut ala_events).await,
        Event::Delivered { peer, id } if peer == ola_id && id == hello.id
    ));

    // Ola answers over the connection that Ala opened.
    let reply = Outgoing::new("Cześć, Alu!").unwrap();
    ola.send(ala_id, &reply).await.unwrap();
    assert!(matches!(
        next(&mut ala_events).await,
        Event::Message { peer, body, .. } if peer == ola_id && body == "Cześć, Alu!"
    ));
    assert!(matches!(
        next(&mut ola_events).await,
        Event::Delivered { id, .. } if id == reply.id
    ));

    // When Ola goes away, Ala notices.
    ola.shutdown().await;
    assert!(matches!(
        next(&mut ala_events).await,
        Event::Disconnected { peer } if peer == ola_id
    ));
    ala.shutdown().await;
}

#[tokio::test]
async fn strangers_are_turned_away() {
    let (ala_identity, stranger_identity) = (new_identity(), new_identity());
    let ala_id = ala_identity.id();

    // Ala has an empty address book. The stranger knows Ala's address.
    let (ala, mut ala_events) = Node::spawn(&ala_identity, [], Network::LocalOnly)
        .await
        .unwrap();
    let (stranger, mut stranger_events) =
        Node::spawn(&stranger_identity, [ala_id], Network::LocalOnly)
            .await
            .unwrap();

    // The QUIC handshake itself succeeds, then Ala closes the connection.
    stranger.connect(loopback(&ala)).await.unwrap();
    assert!(matches!(
        next(&mut stranger_events).await,
        Event::Connected { .. }
    ));
    assert!(matches!(
        next(&mut stranger_events).await,
        Event::Disconnected { peer } if peer == ala_id
    ));

    // Nothing reached Ala's user interface.
    let _ = stranger.send(ala_id, &Outgoing::new("spam").unwrap()).await;
    assert!(
        timeout(Duration::from_millis(500), ala_events.recv())
            .await
            .is_err(),
        "a stranger must not produce events"
    );

    stranger.shutdown().await;
    ala.shutdown().await;
}

#[tokio::test]
async fn dialling_a_non_contact_is_refused_locally() {
    let (ala_identity, ola_identity) = (new_identity(), new_identity());
    let (ala, _ala_events) = Node::spawn(&ala_identity, [], Network::LocalOnly)
        .await
        .unwrap();
    assert!(ala.connect(ola_identity.id()).await.is_err());
    ala.shutdown().await;
}

#[tokio::test]
async fn files_travel_directly_and_are_verified() {
    use komunikator_core::{Content, MessageId};

    let (ala_identity, ola_identity) = (new_identity(), new_identity());
    let (ala_id, ola_id) = (ala_identity.id(), ola_identity.id());
    let (ala, mut ala_events) = Node::spawn(&ala_identity, [ola_id], Network::LocalOnly)
        .await
        .unwrap();
    let (ola, mut ola_events) = Node::spawn(&ola_identity, [ala_id], Network::LocalOnly)
        .await
        .unwrap();
    ala.connect(loopback(&ola)).await.unwrap();
    next(&mut ala_events).await;
    next(&mut ola_events).await;

    // Three and a bit megabytes of noise, named like an installer.
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("instalator.exe");
    let content: Vec<u8> = (0..3_300_000u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    std::fs::write(&source, &content).unwrap();

    let message = ala.share(ola_id, &source).await.unwrap();
    let Content::File(sent_offer) = &message.content else {
        panic!("not a file offer")
    };
    assert_eq!(sent_offer.name, "instalator.exe");
    assert_eq!(sent_offer.size, content.len() as u64);
    ala.send(ola_id, &message).await.unwrap();

    let (id, offer) = match next(&mut ola_events).await {
        Event::FileOffered {
            peer, id, offer, ..
        } => {
            assert_eq!(peer, ala_id);
            (id, offer)
        }
        other => panic!("unexpected event: {other:?}"),
    };
    assert_eq!(id, message.id);
    assert!(matches!(next(&mut ala_events).await, Event::Delivered { id: got, .. } if got == id));

    // Pretend an earlier attempt stopped after the first 1000 bytes.
    let downloads = temp_dir();
    std::fs::create_dir_all(&downloads).unwrap();
    std::fs::write(
        downloads.join(format!(".{id}.szept-part")),
        &content[..1000],
    )
    .unwrap();
    let mut first_progress = None;
    let saved = ola
        .download(ala_id, id, &offer, &downloads, |done| {
            first_progress.get_or_insert(done);
        })
        .await
        .unwrap();
    assert_eq!(first_progress, Some(1000), "the download did not resume");
    assert_eq!(saved, downloads.join("instalator.exe"));
    assert_eq!(std::fs::read(&saved).unwrap(), content);

    // A second copy gets a new name instead of overwriting the first.
    let again = ola
        .download(ala_id, id, &offer, &downloads, |_| {})
        .await
        .unwrap();
    assert_eq!(again, downloads.join("instalator (1).exe"));

    // An offer under a made-up id is refused.
    let unknown: MessageId = "00112233445566778899aabbccddeeff".parse().unwrap();
    let err = ola
        .download(ala_id, unknown, &offer, &downloads, |_| {})
        .await
        .unwrap_err();
    assert!(err.to_string().contains("no longer shares"), "{err:#}");

    // Changing the file after offering it makes the transfer fail safely.
    std::fs::write(&source, b"something else").unwrap();
    let err = ola
        .download(ala_id, id, &offer, &downloads, |_| {})
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("changed or disappeared"),
        "{err:#}"
    );

    // Same length, different bytes: caught by the hash and nothing is kept.
    let mut forged = content.clone();
    forged[123_456] ^= 0xff;
    std::fs::write(&source, &forged).unwrap();
    let err = ola
        .download(ala_id, id, &offer, &downloads, |_| {})
        .await
        .unwrap_err();
    assert!(err.to_string().contains("damaged"), "{err:#}");
    assert!(!downloads.join(format!(".{id}.szept-part")).exists());
    assert!(!downloads.join("instalator (2).exe").exists());

    // Withdrawn: refused.
    ala.unshare(&id);
    assert!(
        ola.download(ala_id, id, &offer, &downloads, |_| {})
            .await
            .is_err()
    );

    ala.shutdown().await;
    ola.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(downloads);
}

#[tokio::test]
async fn voice_messages_gifs_and_avatars_arrive() {
    use komunikator_core::{Content, Media};

    let (ala_identity, ola_identity) = (new_identity(), new_identity());
    let (ala_id, ola_id) = (ala_identity.id(), ola_identity.id());
    let (ala, mut ala_events) = Node::spawn(&ala_identity, [ola_id], Network::LocalOnly)
        .await
        .unwrap();
    let (ola, mut ola_events) = Node::spawn(&ola_identity, [ala_id], Network::LocalOnly)
        .await
        .unwrap();
    ala.connect(loopback(&ola)).await.unwrap();
    next(&mut ala_events).await;
    next(&mut ola_events).await;

    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let voice = dir.join("glosowa.ogg");
    std::fs::write(&voice, b"OggS and some sound").unwrap();
    let media = Media::Voice { duration_ms: 2400 };
    let message = ala.share_media(ola_id, &voice, media).await.unwrap();
    assert!(matches!(&message.content, Content::Media(_, got) if *got == media));
    ala.send(ola_id, &message).await.unwrap();

    let offer = match next(&mut ola_events).await {
        Event::FileOffered {
            id,
            offer,
            media: got,
            ..
        } => {
            assert_eq!(id, message.id);
            assert_eq!(got, Some(media));
            offer
        }
        other => panic!("unexpected event: {other:?}"),
    };
    let saved = ola
        .download(ala_id, message.id, &offer, &dir.join("in"), |_| {})
        .await
        .unwrap();
    assert_eq!(std::fs::read(saved).unwrap(), b"OggS and some sound");

    // A program is not a GIF, and a big file is not a voice message.
    let program = dir.join("kot.exe");
    std::fs::write(&program, b"MZ").unwrap();
    assert!(ala.share_media(ola_id, &program, Media::Gif).await.is_err());

    // A profile picture goes straight to the contact; empty means removed.
    let picture = b"\xff\xd8\xff\xe0 a tiny jpeg".to_vec();
    ala.send_avatar(ola_id, &picture).await.unwrap();
    loop {
        match next(&mut ola_events).await {
            Event::Avatar { peer, image } => {
                assert_eq!(peer, ala_id);
                assert_eq!(image, picture);
                break;
            }
            Event::Delivered { .. } => {}
            other => panic!("unexpected event: {other:?}"),
        }
    }
    assert!(ala.send_avatar(ola_id, b"<svg/>").await.is_err());
    ala.send_avatar(ola_id, &[]).await.unwrap();
    assert!(matches!(
        next(&mut ola_events).await,
        Event::Avatar { image, .. } if image.is_empty()
    ));

    ala.shutdown().await;
    ola.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}
