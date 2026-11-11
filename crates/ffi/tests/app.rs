//! Drives two `App`s exactly as the Kotlin side does.

use std::{
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

use komunikator_ffi::{App, ChatEvent, EventListener, normalize_id, qr_code, share_link};

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
