//! **Which runner a real hub names a keeper, and how it says so** (D10,
//! M12, E-16).
//!
//! The milestone's own clause for the hub's half: *two runners, one of them a
//! named keeper — the keeper's `Welcome` carries the keys and the other's
//! omits the field entirely; changing the setting sends `PinPacks` and
//! nothing else.* Every assertion below is made about what **a real hub
//! answered on a socket**, built from the real `Keepers` the binary's
//! command line produces.
//!
//! ## Why the runners here are hand-written and the hub is real
//!
//! The exact opposite division from `pack_custody.rs` beside it, and for the
//! same reason. That file measures what the shipped **runner** does with a
//! `keeper_for` it is given, so its runner is the real binary and its hub is
//! a peer at the other end of the socket. This file measures what the
//! shipped **hub** puts in `keeper_for`, so the hub is real and the runners
//! are peers: what is owed here is the *choice* and the *frame*, and a real
//! runner would add its disk, its budget and its gc to every assertion
//! without making one of them stronger.
//!
//! Together the two files are the whole of custody: this one says the hub
//! names the keeper and sends the frame, that one says the runner honours it
//! and keeps the pack. **Neither of them, and not both of them, says that
//! custody works end to end** — a pack outliving the machine that built it
//! is M12's P7, and it needs M11's push path.
//!
//! ## What "omits the field entirely" is asserted against
//!
//! The **raw text of the frame**, not a decoded `FromHub`. `keeper_for`
//! carries `#[serde(default, skip_serializing_if = "Vec::is_empty")]`, and
//! `herd-proto`'s `round_trip.rs` already asserts that the attribute is
//! there and does what it says. What is unasserted until here is that **the
//! hub puts nothing in it** for an ordinary runner: a decoded frame cannot
//! tell an empty list from an absent one, and the whole point of the
//! attribute is that a runner of an older build reads a welcome it has never
//! seen a new field in.
//!
//! ## Why a health report is sent after every policy change
//!
//! Because *"and nothing else"* is otherwise a statement about a clock. A
//! peer that waited half a second and saw one frame has measured a half
//! second, not a hub. So each peer sends a `Health` after the change and
//! reads until the hub's answer: the frames that arrived before that answer
//! are exactly what the change sent, because the link has since carried a
//! frame in both directions. The same technique, and the same reasoning, as
//! `pack_custody.rs`'s two answered reports.

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use herd_hub::job::JobStore;
use herd_hub::registry::Registration;
use herd_hub::{Hub, HubHandle, JobDirStore, Keepers, Timestamp, KEEPER_KEEP_PER_KEY};
use herd_proto::pack::PackId;
use herd_proto::{from_frame, to_frame, FromHub, Role, ToHub};
use tokio_tungstenite::tungstenite::Message;

/// How long a frame that is owed has to arrive, and how long one that is
/// **not** owed is waited for before the hub is believed to have sent
/// nothing.
///
/// One figure for both on purpose: the second use is the weaker assertion of
/// the two — a frame arriving later than this would be missed rather than
/// caught — and giving it a shorter bound of its own would be making the
/// weaker half weaker still for no reason. The health report that follows
/// every change is what carries the real weight.
const PATIENCE: Duration = Duration::from_secs(20);

/// The runner this hub's policy names, and the one it does not. Both are
/// approved; the only difference between them is the policy.
const THE_KEEPER: &str = "the-keeper";
const THE_OTHER: &str = "the-other-runner";

/// Two key families: the one a keeper is named for, and a second one the
/// policy moves to it later.
const A_KEY_FAMILY: &str = "pack";
const A_SECOND_KEY_FAMILY: &str = "reference";

/// **The hub's figure is the runner's figure.**
///
/// `FromHub::Welcome`'s `keeper_for` carries keys and no count, so a runner
/// welcomed as a keeper applies the constant it shipped with; `PinPacks`
/// carries one. A hub sending a different number would move a keeper's
/// retention the moment its own policy changed — five packs of a family on
/// dialling, three after an unrelated `--keeper-for` was added — for a
/// reason no operator asked for.
///
/// The two constants live in two crates because `herd-proto` holds the frame
/// and not the policy, and `herd-hub` cannot see `herd-runner` outside its
/// tests. So the equality is asserted here, which is the one place both are
/// visible.
#[test]
fn the_count_the_hub_asks_a_keeper_for_is_the_count_the_runner_keeps() {
    assert_eq!(
        KEEPER_KEEP_PER_KEY,
        herd_runner::packstore::KEEP_PER_KEY,
        "the hub asks a keeper for the newest {} pack(s) of a pinned family and the runner \
         keeps the newest {}. Whichever of the two constants has moved, the frame now says \
         something the runner was not built for: `herd_hub::KEEPER_KEEP_PER_KEY` and \
         `herd_runner::packstore::KEEP_PER_KEY` are one figure in two crates",
        KEEPER_KEEP_PER_KEY,
        herd_runner::packstore::KEEP_PER_KEY,
    );
}

/// A real hub on a free loopback port, with the custody policy it is given
/// and nothing connected to it.
struct AHub {
    url: String,
    handle: HubHandle,
    store: Arc<JobDirStore>,
    _state: tempfile::TempDir,
    serving: tokio::task::JoinHandle<()>,
}

impl Drop for AHub {
    /// The serving task is **aborted**, not left to find its own state root
    /// gone: the temporary directory is removed as this struct drops, and a
    /// hub still publishing a status document into it would be writing into
    /// a path that no longer exists.
    fn drop(&mut self) {
        self.serving.abort();
    }
}

impl AHub {
    async fn naming(keepers: Keepers) -> Self {
        let state = tempfile::tempdir().expect("a temporary state directory");
        let store = Arc::new(JobDirStore::open(state.path()).expect("the state root opens"));
        let hub = Hub::bind(
            "127.0.0.1:0".parse().expect("a literal loopback address"),
            Arc::clone(&store) as Arc<dyn JobStore>,
        )
        .await
        .expect("the hub binds a free port")
        .naming_keepers(keepers);
        let url = format!("ws://{}", hub.local_addr());
        let handle = hub.handle();
        let serving = tokio::spawn(async move {
            let _ = hub.serve().await;
        });
        Self {
            url,
            handle,
            store,
            _state: state,
            serving,
        }
    }

    /// Approve one runner and answer the token that approval minted (E-48).
    fn approve(&self, name: &str) -> String {
        self.store
            .approve_runner(name)
            .expect("the state root mints a token")
    }

    /// File a pack under one key family, by calling the registry rather than
    /// sending a frame.
    ///
    /// The *default* keeper's share is "every family nobody else keeps", and
    /// that has no spelling until the hub knows which families exist — so a
    /// test of the default keeper needs a registry with something in it, and
    /// what it needs is the key and nothing else about the pack.
    fn files_a_pack_under(&self, key: &str) {
        let manifest = a_manifest(key);
        // `JobDirStore`'s own `packs()`, which answers the registry itself:
        // the `JobStore` trait's is the `Option` one, for a store that keeps
        // none, and this test's store is the real root.
        let registry = self.store.packs();
        let registered = registry
            .register(Registration {
                commit: "4f1b2c".to_owned(),
                key: key.to_owned(),
                announced: PackId::of_manifest_bytes(&manifest),
                manifest,
                meta: None,
                runner: THE_KEEPER.to_owned(),
                job: "j0000000001".to_owned(),
                attempt: 1,
                at: Timestamp::at_unix_seconds(1_793_145_600),
            })
            .expect("the registry writes the pack");
        if let Some(why) = registered.refusal() {
            panic!("the fixture pack is refused: {why}");
        }
    }
}

/// A fixture manifest, distinct per key family so that two of them are two
/// packs.
///
/// Two `kind f` rows, one `d` and one `l`, as every other pack fixture in
/// this suite has: the `d` and `l` rows carry a sha256 of something that is
/// not a file's bytes, so a walk that does not filter on `kind == f` counts
/// them as objects.
fn a_manifest(key: &str) -> Vec<u8> {
    let mut text = String::from("# chatons pack 1\n# profile debug\n");
    text.push_str(&format!(
        "{}\t4096\t0644\tf\t{key}\tbin/tool\n",
        "1".repeat(64)
    ));
    text.push_str(&format!(
        "{}\t8192\t0644\tf\t{key}\tlib/libthing.rlib\n",
        "4".repeat(64)
    ));
    text.push_str(&format!("{}\t0\t0755\td\t{key}\tbin\n", "2".repeat(64)));
    text.push_str(&format!(
        "{}\t7\t0777\tl\t{key}\tbin/tool-latest\tbin/tool\n",
        "3".repeat(64)
    ));
    text.into_bytes()
}

/// One runner, driven frame by frame, keeping the **text** of everything the
/// hub said to it.
struct Peer {
    socket: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    /// The welcome, verbatim, as it came off the socket.
    welcome: String,
}

impl Peer {
    /// Dial, say hello as a runner, and keep the welcome's own bytes.
    async fn dials(hub: &AHub, name: &str) -> Self {
        let token = hub.approve(name);
        let (mut socket, _response) = tokio_tungstenite::connect_async(&hub.url)
            .await
            .expect("the hub accepts a runner");
        let hello = ToHub::Hello {
            protocol_version: herd_proto::PROTOCOL_VERSION,
            peer_version: herd_hub::hub_version().to_owned(),
            role: Role::Runner,
            token: Some(token),
            name: Some(name.to_owned()),
            instance: Some(format!("{name}-run-1")),
            state_root: None,
            health_every_ms: None,
            capabilities: Default::default(),
            host: herd_proto::HostSize {
                memory_mib: Some(8192),
                threads: Some(4),
                memory_floor_mib: Some(1024),
                tmpfs_mib: Some(4096),
            },
            preference: 0,
            load_milli: None,
            switch: herd_proto::Switch::On,
            jobs_at_once: None,
            mesh_endpoint: None,
        };
        socket
            .send(Message::text(to_frame(&hello).expect("the hello encodes")))
            .await
            .expect("the hello is sent");
        let welcome = loop {
            let frame = socket
                .next()
                .await
                .expect("the hub answers a hello")
                .expect("the frame arrives whole");
            let text = frame.to_text().expect("a text frame").to_owned();
            if text.is_empty() {
                continue;
            }
            break text;
        };
        // Decoded to be sure of *which* frame this is, and kept as text
        // because an absent field is a fact about the bytes and not about
        // the value they decode to.
        match from_frame::<FromHub>(&welcome) {
            Ok(FromHub::Welcome { .. }) => {}
            other => panic!("an approved runner is welcomed, and this hub said: {other:?}"),
        }
        Self { socket, welcome }
    }

    /// Everything the hub sent between now and its answer to one health
    /// report, in order, with the answer itself left out.
    ///
    /// This is what makes "and nothing else" an assertion about the hub
    /// rather than about a clock: the report crosses the link and its answer
    /// comes back, so a frame the hub meant to send before it has been sent.
    async fn frames_up_to_a_health_answer(&mut self) -> Vec<FromHub> {
        self.socket
            .send(Message::text(
                to_frame(&ToHub::Health {
                    jobs: Vec::new(),
                    draining: false,
                    load_milli: None,
                    switch: herd_proto::Switch::On,
                    pack_store_bytes: None,
                    pack_store_budget_bytes: None,
                    packs_held: None,
                })
                .expect("the report encodes"),
            ))
            .await
            .expect("the report is sent");
        let mut seen = Vec::new();
        let read = tokio::time::timeout(PATIENCE, async {
            loop {
                let frame = self
                    .socket
                    .next()
                    .await
                    .expect("the hub keeps talking")
                    .expect("the frame arrives whole");
                let Ok(text) = frame.to_text() else { continue };
                if text.is_empty() {
                    continue;
                }
                let message: FromHub = from_frame(text).expect("the hub speaks the protocol");
                if matches!(message, FromHub::HealthRecorded { .. }) {
                    return seen;
                }
                seen.push(message);
            }
        })
        .await;
        read.unwrap_or_else(|_| panic!("a real hub answers a health report"))
    }
}

/// **Two runners, one of them a named keeper: the keeper's welcome carries
/// the keys and the other's omits the field entirely** (the milestone's own
/// clause).
///
/// One policy, two runners, and the only difference between them is which
/// one the policy names. The absent field is asserted on the frame's text,
/// for the reason this file's own doc gives.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_named_keeper_is_welcomed_with_its_keys_and_nobody_else_carries_the_field() {
    let hub = AHub::naming(Keepers::nobody().naming(A_KEY_FAMILY, THE_KEEPER)).await;
    let keeper = Peer::dials(&hub, THE_KEEPER).await;
    let other = Peer::dials(&hub, THE_OTHER).await;

    assert!(
        keeper.welcome.contains("\"keeper_for\"")
            && keeper.welcome.contains(&format!("\"{A_KEY_FAMILY}\"")),
        "the welcome of the runner this hub's policy names has to carry the key family it \
         keeps. It said: {}",
        keeper.welcome
    );
    assert!(
        !other.welcome.contains("keeper_for"),
        "and the welcome of a runner the policy does not name must OMIT the field, not \
         carry it empty: an empty list and an absent one decode to the same thing, and the \
         skip is what lets a runner of an older build read this frame at all. It said: {}",
        other.welcome
    );
    assert_eq!(
        hub.handle.custody_of(THE_KEEPER),
        vec![A_KEY_FAMILY.to_owned()],
        "and the hub says the same thing when it is asked rather than when it is dialled"
    );
    assert!(
        hub.handle.custody_of(THE_OTHER).is_empty(),
        "the runner the policy does not name keeps nothing"
    );
}

/// **A hub that names no keeper sends `keeper_for` to nobody.**
///
/// The control, and the half that proves the field above was the policy's
/// doing: without it, a hub that put every runner's name in every welcome
/// would pass the test above. It is also this build's behaviour before the
/// setting existed, which is what a hub given neither flag must keep.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_hub_that_names_no_keeper_welcomes_every_runner_without_the_field() {
    let hub = AHub::naming(Keepers::nobody()).await;
    let keeper = Peer::dials(&hub, THE_KEEPER).await;
    let other = Peer::dials(&hub, THE_OTHER).await;
    for welcome in [&keeper.welcome, &other.welcome] {
        assert!(
            !welcome.contains("keeper_for"),
            "a hub told about no custody names no keeper, and this welcome carries the \
             field: {welcome}"
        );
    }
    assert!(hub.handle.custody_of(THE_KEEPER).is_empty());
}

/// **Changing the setting sends `PinPacks`, and nothing else** (the second
/// half of the clause).
///
/// Three changes in one connection, because each of them is a different
/// thing the frame has to be able to say:
///
/// 1. a family **added** to a keeper — the frame carries the whole new set,
///    because `PinPacks` replaces rather than adds;
/// 2. a change that moves **nothing** for anybody — no frame at all, and the
///    hub says it told nobody;
/// 3. custody **retracted** — the empty frame, which is the only thing that
///    can take a pin back.
///
/// And through all three, the runner the policy never names receives
/// **nothing**: that is the "nothing else" that matters, and it is asserted
/// on a link that has carried a frame in both directions since.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn changing_the_setting_sends_pin_packs_and_nothing_else() {
    let hub = AHub::naming(Keepers::nobody().naming(A_KEY_FAMILY, THE_KEEPER)).await;
    let mut keeper = Peer::dials(&hub, THE_KEEPER).await;
    let mut other = Peer::dials(&hub, THE_OTHER).await;

    // 1. One family more for the keeper, nothing for anybody else.
    let told = hub.handle.name_keepers(
        Keepers::nobody()
            .naming(A_KEY_FAMILY, THE_KEEPER)
            .naming(A_SECOND_KEY_FAMILY, THE_KEEPER),
    );
    assert_eq!(
        told.len(),
        1,
        "one runner's custody moved, so one runner is told: {told:?}"
    );
    match keeper.frames_up_to_a_health_answer().await.as_slice() {
        [FromHub::PinPacks(pins)] => {
            assert_eq!(
                pins.keys,
                vec![A_KEY_FAMILY.to_owned(), A_SECOND_KEY_FAMILY.to_owned()],
                "the frame carries the WHOLE set and not the difference, because applying \
                 it replaces what is in force"
            );
            assert_eq!(
                pins.keep_per_key, KEEPER_KEEP_PER_KEY,
                "and it carries this hub's own figure, which is the runner's"
            );
        }
        others => panic!(
            "changing the setting sends PinPacks and nothing else, and this keeper was sent \
             {others:?}"
        ),
    }
    assert!(
        other.frames_up_to_a_health_answer().await.is_empty(),
        "and the runner the policy does not name is sent NOTHING: a hub that told every \
         runner about every change would be writing on an owner's stderr about an event \
         that did not concern her machine"
    );

    // 2. The same policy again, which moves nothing.
    let told = hub.handle.name_keepers(
        Keepers::nobody()
            .naming(A_KEY_FAMILY, THE_KEEPER)
            .naming(A_SECOND_KEY_FAMILY, THE_KEEPER),
    );
    assert!(
        told.is_empty(),
        "a policy that changes nothing tells nobody: {told:?}"
    );
    for peer in [&mut keeper, &mut other] {
        assert!(
            peer.frames_up_to_a_health_answer().await.is_empty(),
            "and sends no frame to anybody"
        );
    }

    // 3. Nobody at all, which has to be able to take the pins back.
    let told = hub.handle.name_keepers(Keepers::nobody());
    assert_eq!(
        told.len(),
        1,
        "the keeper is told it keeps nothing: {told:?}"
    );
    assert!(
        told[0].contains("keeps nothing any more"),
        "and it is said in those words, because an operator reading this line is reading \
         the moment custody ended: {}",
        told[0]
    );
    match keeper.frames_up_to_a_health_answer().await.as_slice() {
        [FromHub::PinPacks(pins)] => assert!(
            pins.keys.is_empty(),
            "custody is retracted by the empty frame and nothing else: a hub that simply \
             stopped sending could never take a pin back, and the runner would keep the \
             family for ever. It sent {:?}",
            pins.keys
        ),
        others => panic!("retracting custody sends one PinPacks, and this sent {others:?}"),
    }
    assert!(
        other.frames_up_to_a_health_answer().await.is_empty(),
        "and still nothing to the runner the policy never named"
    );
}

/// **The default keeper keeps every key family nobody else is named for,
/// and a named family is moved off it rather than added to it** (D10's
/// sentence, as far as a hub can carry it).
///
/// The default keeper's share is the one that cannot be written out as a
/// list of names: *every family nobody else keeps* has no spelling until the
/// hub knows which families exist, so it is filled from the families this
/// hub has a pack registered under. Two packs are filed, one of each family,
/// and the policy names the second family to the other runner.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_default_keeper_keeps_the_families_this_hub_has_packs_under() {
    let hub = AHub::naming(
        Keepers::nobody()
            .with_default_keeper(THE_KEEPER)
            .naming(A_SECOND_KEY_FAMILY, THE_OTHER),
    )
    .await;
    assert!(
        hub.handle.custody_of(THE_KEEPER).is_empty(),
        "a default keeper on a hub with no pack registered keeps nothing, because there is \
         no family to keep. That is a measurement and not a defect, and it is why a family \
         an operator names explicitly is pinned whether or not anything is filed under it"
    );
    assert_eq!(
        hub.handle.custody_of(THE_OTHER),
        vec![A_SECOND_KEY_FAMILY.to_owned()],
        "while a family named explicitly is kept from the first moment"
    );

    hub.files_a_pack_under(A_KEY_FAMILY);
    hub.files_a_pack_under(A_SECOND_KEY_FAMILY);
    assert_eq!(
        hub.handle.custody_of(THE_KEEPER),
        vec![A_KEY_FAMILY.to_owned()],
        "the default keeper keeps the family nobody else is named for, and the family that \
         IS named to somebody else is moved off it rather than held by both"
    );
    assert_eq!(
        hub.handle.custody_of(THE_OTHER),
        vec![A_SECOND_KEY_FAMILY.to_owned()],
    );

    // And a runner dialling now is welcomed with it, which is the only
    // thing in this test a hub really promises a runner.
    let keeper = Peer::dials(&hub, THE_KEEPER).await;
    assert!(
        keeper.welcome.contains(&format!("\"{A_KEY_FAMILY}\""))
            && !keeper.welcome.contains(A_SECOND_KEY_FAMILY),
        "the default keeper's welcome carries the family it keeps and not the one it does \
         not. It said: {}",
        keeper.welcome
    );
}
