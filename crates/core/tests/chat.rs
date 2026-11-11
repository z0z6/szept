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
