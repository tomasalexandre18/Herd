//! Every message must survive a trip through JSON unchanged. The protocol is
//! the contract: a field that silently fails to round-trip is a lost field.

use std::collections::BTreeMap;

use herd_proto::{
    cap_exceeded_reason, cap_stop_reason, from_frame, to_frame, Asked, Availability, BusyProbe,
    Cap, Capability, CapabilityOffer, CapabilityOrigins, CapabilityState, Declaration, Executor,
    FactReason, FromHub, HostSize, JobId, JobMetrics, JobOutcome, LabelSet, LogStream,
    OfferedCapability, Pressure, Priority, Role, RunnerSwitch, RunningJob, Step, StepRun, Switch,
    SwitchPersistence, ToHub, PROTOCOL_VERSION,
};

/// A declared command as a submission carries it (E-11): the name their own
/// file gave it, a commit resolved in the requester's checkout, the arguments
/// as typed, and their four hooks with `{args}` and `{commit}` already filled
/// (D106).
///
/// The hooks are shaped like the consumer's own, with no consumer command in
/// them (E-11): what crosses the wire has to be tested against the shape they
/// asked for — a shell line each, an environment, and the tokens Herd fills
/// later still in place.
fn a_declaration() -> Declaration {
    Declaration {
        command: "a-declared-command".to_owned(),
        commit: "cb5a2e8b4f1d9c7a3e05b8d6f2a1c4e79b0d3f68".to_owned(),
        args: vec!["--only".to_owned(), "one,two".to_owned()],
        executor: Executor {
            up: "tool ws up {job} cb5a2e8b4f1d9c7a3e05b8d6f2a1c4e79b0d3f68".to_owned(),
            fetch: "tool ws run {job} --no-nix -- 'git fetch -q origin && git switch -q --detach \
                    cb5a2e8b4f1d9c7a3e05b8d6f2a1c4e79b0d3f68'"
                .to_owned(),
            run: "tool ws run {job} -- {line}".to_owned(),
            down: "tool ws down {job} --purge".to_owned(),
            env: BTreeMap::from([("RESULTS".to_owned(), "{out}/results.json".to_owned())]),
        },
    }
}

/// Every step of an attempt, as a runner reports them (D77): one that exited,
/// and one a cap stopped with no code to report.
fn every_step() -> Vec<StepRun> {
    vec![
        StepRun {
            step: Step::Up,
            started_unix_ms: 1_791_878_153_000,
            duration_ms: 95_400,
            exit_code: Some(0),
            ended: None,
            ran_commit: None,
        },
        StepRun {
            step: Step::Fetch,
            started_unix_ms: 1_791_878_248_400,
            duration_ms: 1_200,
            exit_code: Some(0),
            ended: None,
            ran_commit: None,
        },
        StepRun {
            step: Step::Run,
            started_unix_ms: 1_791_878_249_600,
            duration_ms: 2_700_000,
            exit_code: None,
            ended: Some("stopped at the time cap, 2700 s".to_owned()),
            ran_commit: None,
        },
        StepRun {
            step: Step::Down,
            started_unix_ms: 1_791_880_949_600,
            duration_ms: 400,
            exit_code: Some(1),
            ended: None,
            ran_commit: None,
        },
    ]
}

/// An offer carrying one of each kind, each origin and each probe, so that
/// nothing in the algebra crosses the wire untested.
fn an_offer() -> CapabilityOffer {
    CapabilityOffer::new(vec![
        OfferedCapability::new(
            Capability::counted("build", 2)
                .excluding(["metal-pc"])
                .expect("a valid exclusion"),
            CapabilityOrigins::declared(),
        ),
        OfferedCapability::new(
            Capability::lease("metal-pc", BusyProbe::lock_file("/run/herd/metal-pc")),
            CapabilityOrigins::declared(),
        ),
        OfferedCapability::new(
            Capability::exclusive("nightly").probed_by(BusyProbe::Command {
                command: vec!["/usr/bin/leased".to_owned(), "--check".to_owned()],
                busy_exit_code: 3,
                free_exit_code: 4,
            }),
            CapabilityOrigins::declared(),
        ),
        OfferedCapability::new(
            Capability::shared("reference"),
            CapabilityOrigins::declared(),
        ),
        OfferedCapability::new(
            Capability::shared("kvm"),
            CapabilityOrigins {
                probed: true,
                declared: false,
                denied: true,
            },
        ),
    ])
    .expect("a valid offer")
}

/// Every cap a runner can stop a job at (M4).
pub const EVERY_CAP: [Cap; 4] = [Cap::Time, Cap::Memory, Cap::Log, Cap::Disk];

/// Every way a job can end, including the codes E-02 depends on and the 127 a
/// command may produce on its own.
fn every_outcome() -> Vec<JobOutcome> {
    let mut outcomes: Vec<JobOutcome> = [0, 1, 42, 75, 127, -1, i32::MIN, i32::MAX]
        .into_iter()
        .map(|code| JobOutcome::Exited { code })
        .collect();
    outcomes.extend([9, 15].map(|signal| JobOutcome::Signalled { signal }));
    outcomes.push(JobOutcome::CouldNotStart {
        reason: "no-such-tool: No such file or directory (os error 2)".to_owned(),
    });
    outcomes.push(JobOutcome::CouldNotStart {
        reason: String::new(),
    });
    outcomes.extend(EVERY_CAP.into_iter().map(|cap| JobOutcome::StoppedByCap {
        cap,
        limit: 0,
        detail: None,
    }));
    outcomes.extend(EVERY_CAP.into_iter().map(|cap| JobOutcome::StoppedByCap {
        cap,
        limit: u64::MAX,
        detail: Some("the workspace could not be measured".to_owned()),
    }));
    // M3: a job can also end because somebody asked for it to, or because the
    // machine it was on stopped answering (E-06, E-21).
    outcomes.push(JobOutcome::Cancelled {
        reason: "a requester cancelled this job".to_owned(),
    });
    outcomes.push(JobOutcome::Lost {
        reason: "runner wsl-pc stopped answering: nothing from it for 61s".to_owned(),
    });
    outcomes.push(JobOutcome::Impossible {
        reason: "gpu-venus and igpu-vfio cannot be held together".to_owned(),
    });
    outcomes
}

/// Every shape of log payload a job can produce. A compiler's output is not
/// text: it can hold a NUL, a byte sequence that is no UTF-8 at all, or a
/// multi-byte character cut in half by a chunk boundary. None of it may be
/// rewritten on the way through (D18, E-15).
fn every_payload() -> Vec<Vec<u8>> {
    vec![
        Vec::new(),
        b"a line\n".to_vec(),
        b"a\0b\xff\xfec\n".to_vec(),
        vec![0, 1, 2, 253, 254, 255],
        // The first two bytes of "é" in UTF-8, with the third in the chunk
        // that follows: lossy decoding would turn this into U+FFFD.
        vec![b'e', 0xc3],
        "accents é, a sheep 🐑".as_bytes().to_vec(),
        (0u8..=255).collect(),
    ]
}

/// Every shape a measurement set can have: nothing readable, everything
/// readable, and the extremes of the two figures that are never optional.
fn every_metrics() -> Vec<JobMetrics> {
    vec![
        JobMetrics::default(),
        JobMetrics {
            host_load_milli: Some(12_340),
            peak_memory_bytes: Some(1_073_741_824),
            bytes_written: Some(4_096),
            log_bytes: 65_536,
            wall_time_ms: 1_234,
            pressure: Some(Pressure {
                host_cpu_milli: Some(1_120),
                host_memory_milli: Some(0),
                host_io_milli: Some(100_000),
                job_cpu_milli: Some(12_340),
                job_memory_milli: None,
                job_io_milli: Some(5),
            }),
        },
        JobMetrics {
            host_load_milli: Some(0),
            peak_memory_bytes: Some(0),
            bytes_written: Some(0),
            log_bytes: u64::MAX,
            wall_time_ms: u64::MAX,
            // A reading with nothing in it: every figure absent, which is
            // what a kernel without PSI reports, and not a row of zeroes.
            pressure: Some(Pressure::default()),
        },
    ]
}

/// Every shape a job's declared outputs can have (D131): nothing at all, a
/// file under a directory, bytes that are not text, a refusal with figures,
/// and something skipped.
///
/// `every_payload` is reused for the bytes on purpose: an output file is as
/// much "not text" as a log chunk is — a `results.json` is UTF-8 only if the
/// consumer made it so, and Herd promises the bytes it was given (D18, E-15).
fn every_output_set() -> Vec<herd_proto::JobOutputs> {
    let mut sets = vec![
        herd_proto::JobOutputs::default(),
        herd_proto::JobOutputs::refusing(
            "this job's output is not in its record: 1 GiB in 3 file(s), and this runner's \
             output cap is 8 MiB"
                .to_owned(),
            vec!["files/link is a symlink, and a symlink is never followed".to_owned()],
        ),
        herd_proto::JobOutputs {
            skipped: vec!["out/pipe is not a regular file".to_owned()],
            ..herd_proto::JobOutputs::default()
        },
    ];
    for payload in every_payload() {
        sets.push(herd_proto::JobOutputs {
            files: vec![
                // The path the first consumer's contract is about, with its
                // directory kept.
                herd_proto::OutputFile::collected("files/results.json".to_owned(), payload.clone()),
                herd_proto::OutputFile::collected(
                    "deep/under/here.bin".to_owned(),
                    payload.clone(),
                ),
            ],
            ..herd_proto::JobOutputs::default()
        });
        // And the same collection by the other carriage: the manifest alone,
        // every entry announced and none carried. Both shapes have to survive
        // the wire, because which one a job's verdict holds is the runner's
        // choice and a decoder never gets to insist on one (D148).
        sets.push(herd_proto::JobOutputs {
            files: vec![
                herd_proto::OutputFile::announced(
                    "files/results.json".to_owned(),
                    payload.len() as u64,
                    herd_proto::sha256_hex(&payload),
                ),
                herd_proto::OutputFile::announced(
                    "deep/under/here.bin".to_owned(),
                    payload.len() as u64,
                    herd_proto::sha256_hex(&payload),
                ),
            ],
            streamed: true,
            ..herd_proto::JobOutputs::default()
        });
    }
    sets
}

fn round_trip_to_hub(message: ToHub) {
    let frame = to_frame(&message).expect("message encodes");
    let back: ToHub = from_frame(&frame).unwrap_or_else(|e| panic!("{frame} decodes: {e}"));
    assert_eq!(message, back, "round trip changed the message: {frame}");
}

fn round_trip_from_hub(message: FromHub) {
    let frame = to_frame(&message).expect("message encodes");
    let back: FromHub = from_frame(&frame).unwrap_or_else(|e| panic!("{frame} decodes: {e}"));
    assert_eq!(message, back, "round trip changed the message: {frame}");
}

#[test]
fn every_message_to_the_hub_round_trips() {
    for role in [Role::Runner, Role::Requester] {
        round_trip_to_hub(ToHub::Hello {
            protocol_version: PROTOCOL_VERSION,
            peer_version: "0.1.0".to_owned(),
            role,
            token: Some("a-token".to_owned()),
            name: Some("wsl-pc".to_owned()),
            instance: Some("run-4242-17a3f9".to_owned()),
            state_root: Some("root-0123456789abcdef".to_owned()),
            health_every_ms: Some(15_000),
            capabilities: an_offer(),
            host: HostSize::new(Some(17 * 1024), Some(20))
                .with_floor(Some(3481))
                .with_tmpfs(Some(8192)),
            jobs_at_once: Some(6),
            preference: 10,
            load_milli: Some(1420),
            switch: Switch::Off {
                reason: "she is gaming".to_owned(),
            },
            mesh_endpoint: None,
        });
        // The optional fields really are optional.
        round_trip_to_hub(ToHub::Hello {
            peer_version: "0.1.0".to_owned(),
            protocol_version: PROTOCOL_VERSION,
            role,
            token: None,
            name: None,
            instance: None,
            state_root: None,
            health_every_ms: None,
            capabilities: CapabilityOffer::empty(),
            host: HostSize::default(),
            jobs_at_once: None,
            preference: 0,
            load_milli: None,
            switch: Switch::On,
            mesh_endpoint: None,
        });
    }
    round_trip_to_hub(ToHub::CapabilitiesChanged {
        capabilities: vec![
            CapabilityState::new("metal-pc", Availability::Free),
            CapabilityState::new("kvm", Availability::busy("the lock file /run/k is there")),
            CapabilityState::new("vfio", Availability::unknown("the probe /bin/p exited 7")),
        ],
    });
    round_trip_to_hub(ToHub::CapabilitiesChanged {
        capabilities: Vec::new(),
    });
    for priority in Priority::ALL {
        let full = Asked {
            command: vec!["sh".to_owned(), "-c".to_owned(), "echo hi".to_owned()],
            labels: LabelSet::new(["build", "reference"]).expect("valid labels"),
            priority,
            build_thread_mib: Some(1792),
            memory_mib: None,
            tmpfs_mib: Some(0),
            deadline_unix_s: Some(1_792_000_000),
            after: Some(JobId::new("j0000000003")),
            requeue_exit_code: Some(75),
            declaration: Some(a_declaration()),
            labels_from: Some(herd_proto::LabelsFrom {
                line: "shape --labels".to_owned(),
                baseline: LabelSet::of("build").expect("a label"),
            }),
            timeout_s: Some(2700),
            needs: Vec::new(),
            leaves: std::collections::BTreeMap::new(),
        };
        let bare = Asked {
            command: vec![],
            labels: LabelSet::default(),
            priority,
            build_thread_mib: None,
            memory_mib: Some(3584),
            tmpfs_mib: Some(4096),
            deadline_unix_s: None,
            after: None,
            requeue_exit_code: None,
            declaration: None,
            labels_from: None,
            timeout_s: None,
            needs: Vec::new(),
            leaves: std::collections::BTreeMap::new(),
        };
        // A dry run carries exactly what a submission carries (E-07).
        round_trip_to_hub(ToHub::SubmitJob {
            asked: full.clone(),
            wants_files: true,
            announce_end: true,
        });
        round_trip_to_hub(ToHub::DryRun { asked: full });
        round_trip_to_hub(ToHub::SubmitJob {
            asked: bare.clone(),
            wants_files: false,
            announce_end: false,
        });
        round_trip_to_hub(ToHub::DryRun { asked: bare });
        round_trip_to_hub(ToHub::RaiseJob {
            job_id: JobId::new("j0000000007"),
            priority,
        });
    }
    round_trip_to_hub(ToHub::WaitForJob {
        job_id: JobId::new("j0000000001"),
        wants_files: true,
    });
    round_trip_to_hub(ToHub::JobStarted {
        job_id: JobId::new("j1"),
        attempt: 1,
    });
    for stream in [LogStream::Stdout, LogStream::Stderr] {
        for data in every_payload() {
            round_trip_to_hub(ToHub::LogChunk {
                job_id: JobId::new("j1"),
                attempt: 1,
                stream,
                seq: 0,
                data,
            });
        }
    }
    round_trip_to_hub(ToHub::LogChunk {
        job_id: JobId::new("j1"),
        attempt: u32::MAX,
        stream: LogStream::Stdout,
        seq: u64::MAX,
        data: b"quotes \" braces {} newline \n tab \t".to_vec(),
    });
    for outcome in every_outcome() {
        for metrics in every_metrics() {
            round_trip_to_hub(ToHub::JobFinished {
                job_id: JobId::new("j1"),
                attempt: 2,
                outcome: outcome.clone(),
                chunks_sent: 97,
                metrics,
                steps: every_step(),
                outputs: herd_proto::JobOutputs::default(),
            });
        }
        // The same verdict from a job that is not a declared command: no
        // step ran, and an empty list must survive the trip as an empty list.
        round_trip_to_hub(ToHub::JobFinished {
            job_id: JobId::new("j1"),
            attempt: 2,
            outcome: outcome.clone(),
            chunks_sent: 0,
            metrics: JobMetrics::default(),
            steps: Vec::new(),
            outputs: herd_proto::JobOutputs::default(),
        });
        // And every shape a job's declared outputs can have, byte for byte
        // (D131): a path with its directories kept, a payload that is not
        // text, a refusal, a skip.
        for outputs in every_output_set() {
            round_trip_to_hub(ToHub::JobFinished {
                job_id: JobId::new("j1"),
                attempt: 2,
                outcome: outcome.clone(),
                chunks_sent: 1,
                metrics: JobMetrics::default(),
                steps: Vec::new(),
                outputs,
            });
        }
    }
}

#[test]
fn every_message_from_the_hub_round_trips() {
    round_trip_from_hub(FromHub::Welcome {
        protocol_version: PROTOCOL_VERSION,
        hub_version: "0.1.0".to_owned(),
        state_root: "root-0123456789abcdef0123456789abcdef".to_owned(),
        version_note: None,
        keeper_for: Vec::new(),
    });
    round_trip_from_hub(FromHub::Refused {
        reason: herd_proto::version_mismatch_reason(PROTOCOL_VERSION + 1),
    });
    round_trip_from_hub(FromHub::JobAccepted {
        job_id: JobId::new("j1"),
    });
    round_trip_from_hub(FromHub::JobResultAck {
        job_id: JobId::new("j1"),
    });
    round_trip_from_hub(FromHub::AssignJob {
        job_id: JobId::new("j1"),
        attempt: 1,
        command: vec!["true".to_owned()],
        declaration: None,
        timeout_s: None,
        packs_in: Vec::new(),
        packs_out: std::collections::BTreeMap::new(),
    });
    // The same assignment for a declared command: the four hooks, the commit,
    // the arguments and the cap the declaration asked for (E-11, D108).
    round_trip_from_hub(FromHub::AssignJob {
        job_id: JobId::new("j1"),
        attempt: 2,
        command: vec!["a-line --only {j}".to_owned()],
        declaration: Some(a_declaration()),
        timeout_s: Some(7200),
        packs_in: Vec::new(),
        packs_out: std::collections::BTreeMap::new(),
    });
    for step in every_step() {
        round_trip_to_hub(ToHub::StepFinished {
            job_id: JobId::new("j1"),
            attempt: 2,
            step,
        });
    }
    for stream in [LogStream::Stdout, LogStream::Stderr] {
        for data in every_payload() {
            round_trip_from_hub(FromHub::LogChunk {
                job_id: JobId::new("j1"),
                stream,
                seq: 7,
                data,
            });
        }
    }
    for data in every_payload() {
        round_trip_from_hub(FromHub::JobLog {
            job_id: JobId::new("j1"),
            data,
        });
    }
    round_trip_from_hub(FromHub::JobNote {
        job_id: JobId::new("j1"),
        note: "its attempt was interrupted".to_owned(),
    });
    for outcome in every_outcome() {
        for metrics in every_metrics() {
            round_trip_from_hub(FromHub::JobFinished {
                job_id: JobId::new("j1"),
                outcome: outcome.clone(),
                metrics,
                outputs: herd_proto::JobOutputs::default(),
                commit: None,
            });
        }
        // The requester's half of D131: whatever the record holds reaches the
        // requester unchanged, including a payload that is not text.
        for outputs in every_output_set() {
            round_trip_from_hub(FromHub::JobFinished {
                job_id: JobId::new("j1"),
                outcome: outcome.clone(),
                metrics: JobMetrics::default(),
                outputs,
                commit: None,
            });
        }
    }
}

#[test]
fn frames_are_tagged_json_objects() {
    let frame = to_frame(&ToHub::JobStarted {
        job_id: JobId::new("j42"),
        attempt: 1,
    })
    .expect("encodes");
    assert_eq!(
        frame,
        r#"{"type":"job_started","job_id":"j42","attempt":1}"#
    );
}

/// The outcome's wire shape, pinned variant by variant. A job's end is a
/// tagged object nested in the frame, so M3, M8 and M9 can add a variant
/// without touching any of these three.
#[test]
fn each_outcome_is_a_tagged_object_inside_the_frame() {
    let frame = |outcome| {
        to_frame(&ToHub::JobFinished {
            job_id: JobId::new("j1"),
            attempt: 1,
            outcome,
            chunks_sent: 0,
            metrics: JobMetrics::default(),
            steps: Vec::new(),
            outputs: herd_proto::JobOutputs::default(),
        })
        .expect("encodes")
    };
    // No measurement was taken, so every optional figure is absent from the
    // frame rather than present as a zero.
    let empty = r#","chunks_sent":0,"metrics":{"log_bytes":0,"wall_time_ms":0}}"#;
    assert_eq!(
        frame(JobOutcome::Exited { code: 42 }),
        format!(
            r#"{{"type":"job_finished","job_id":"j1","attempt":1,"outcome":{{"type":"exited","code":42}}{empty}"#
        )
    );
    assert_eq!(
        frame(JobOutcome::Signalled { signal: 15 }),
        format!(
            r#"{{"type":"job_finished","job_id":"j1","attempt":1,"outcome":{{"type":"signalled","signal":15}}{empty}"#
        )
    );
    assert_eq!(
        frame(JobOutcome::CouldNotStart {
            reason: "no-such-tool: not found".to_owned()
        }),
        format!(
            r#"{{"type":"job_finished","job_id":"j1","attempt":1,"outcome":{{"type":"could_not_start","reason":"no-such-tool: not found"}}{empty}"#
        )
    );
    assert_eq!(
        frame(JobOutcome::Cancelled {
            reason: "a requester cancelled this job".to_owned()
        }),
        format!(
            r#"{{"type":"job_finished","job_id":"j1","attempt":1,"outcome":{{"type":"cancelled","reason":"a requester cancelled this job"}}{empty}"#
        )
    );
    assert_eq!(
        frame(JobOutcome::Lost {
            reason: "runner wsl-pc stopped answering".to_owned()
        }),
        format!(
            r#"{{"type":"job_finished","job_id":"j1","attempt":1,"outcome":{{"type":"lost","reason":"runner wsl-pc stopped answering"}}{empty}"#
        )
    );
    assert_eq!(
        frame(JobOutcome::Impossible {
            reason: "gpu-venus and igpu-vfio cannot be held together".to_owned()
        }),
        format!(
            r#"{{"type":"job_finished","job_id":"j1","attempt":1,"outcome":{{"type":"impossible","reason":"gpu-venus and igpu-vfio cannot be held together"}}{empty}"#
        )
    );
    assert_eq!(
        frame(JobOutcome::StoppedByCap {
            cap: Cap::Memory,
            limit: 2_147_483_648,
            detail: None
        }),
        format!(
            r#"{{"type":"job_finished","job_id":"j1","attempt":1,"outcome":{{"type":"stopped_by_cap","cap":"memory","limit":2147483648}}{empty}"#
        )
    );
    // A cap whose figure alone would mislead carries its reason, and a cap
    // whose figure says it all does not carry an empty field (D30).
    assert_eq!(
        frame(JobOutcome::StoppedByCap {
            cap: Cap::Disk,
            limit: 21_474_836_480,
            detail: Some("the workspace could not be measured".to_owned())
        }),
        format!(
            r#"{{"type":"job_finished","job_id":"j1","attempt":1,"outcome":{{"type":"stopped_by_cap","cap":"disk","limit":21474836480,"detail":"the workspace could not be measured"}}{empty}"#
        )
    );
}

/// A measurement that was taken appears; one that could not be read does not.
/// E-15's record is read by a consumer, so "absent" and "zero" must not be
/// the same bytes on the wire.
#[test]
fn a_figure_that_was_not_measured_is_absent_rather_than_zero() {
    let frame = to_frame(&ToHub::JobFinished {
        job_id: JobId::new("j1"),
        attempt: 1,
        outcome: JobOutcome::Exited { code: 0 },
        chunks_sent: 2,
        metrics: JobMetrics {
            host_load_milli: None,
            peak_memory_bytes: Some(0),
            bytes_written: Some(4_096),
            log_bytes: 12,
            wall_time_ms: 34,
            // The same rule, one level down: a stall figure nobody could
            // read is not in the frame, and a measured zero is.
            pressure: Some(Pressure {
                host_cpu_milli: None,
                host_memory_milli: Some(0),
                ..Pressure::default()
            }),
        },
        steps: Vec::new(),
        outputs: herd_proto::JobOutputs::default(),
    })
    .expect("encodes");
    assert!(
        !frame.contains("host_load_milli"),
        "an unread figure must not be in the frame at all: {frame}"
    );
    assert!(
        frame.contains(r#""peak_memory_bytes":0"#),
        "a figure measured as zero must be in the frame: {frame}"
    );
    assert!(frame.contains(r#""bytes_written":4096"#), "{frame}");
}

/// Every cap's sentence names the cap and carries its figure, so a human
/// reading a record never sees a bare "killed".
#[test]
fn every_cap_s_sentence_names_the_cap_and_its_figure() {
    for cap in EVERY_CAP {
        let reason = cap_exceeded_reason(cap, 600);
        assert!(reason.contains(cap.name()), "{reason}");
        assert!(reason.contains("600"), "{reason}");
        assert!(reason.contains(cap.unit()), "{reason}");
        // With no detail it is that same sentence, so there is one wording
        // for one event (D24) however the caller asks for it.
        assert_eq!(cap_stop_reason(cap, 600, None), reason);
        // With one, the cap is what the runner could not hold the job *to*,
        // and the reason is in the sentence: "stopped at its disk cap of
        // 21474836480 bytes" for a job of four kilobytes was a true number
        // in a false sentence (D30).
        let said = cap_stop_reason(cap, 600, Some("the workspace could not be measured"));
        assert!(said.contains(cap.name()), "{said}");
        assert!(said.contains("could not hold it"), "{said}");
        assert!(said.contains("could not be measured"), "{said}");
    }
}

/// A peer from a later milestone reporting an end this one does not know is a
/// decode error naming it, never a job silently read as having exited. The
/// end used here is M9's deadline, because M3's own 'lost' is a variant now.
#[test]
fn an_unknown_outcome_is_a_decode_error_not_a_panic() {
    let err = from_frame::<ToHub>(
        r#"{"type":"job_finished","job_id":"j1","attempt":1,"chunks_sent":0,"outcome":{"type":"deadline_passed","by":"m9"}}"#,
    )
    .expect_err("an unknown way to end must be rejected");
    assert!(err.to_string().contains("deadline_passed"), "{err}");
}

/// A frame of an unknown type **decodes**, into the catch-all, on both enums.
///
/// This test used to assert the opposite, and the change is the reason
/// `PROTOCOL_VERSION` moved to 2 (D148). An unknown `type` was a decode
/// error, and a decode error on a runner's link killed the link, after which
/// the runner redialled **into the same undecodable frame** — it looped
/// rather than shrugged (D128's recorded defect). What decides whether that
/// is safe is not the decoder but each peer, and the three peers answer
/// differently; those three answers are asserted in `herd-runner`,
/// `herdctl` and `herd-hub` respectively, where the behaviour lives.
///
/// What is asserted here is the one thing that belongs to the wire: it
/// decodes, and it decodes to a value that says nothing else.
#[test]
fn an_unknown_message_type_decodes_to_the_catch_all_on_both_enums() {
    assert_eq!(
        from_frame::<ToHub>(r#"{"type":"deploy_to_production"}"#)
            .expect("an unknown type decodes rather than killing a link"),
        ToHub::Unknown
    );
    assert_eq!(
        from_frame::<FromHub>(r#"{"type":"pack_capability","blob":"abc","peer":"pc"}"#)
            .expect("an unknown type decodes, payload and all, and is discarded"),
        FromHub::Unknown
    );
}

/// And the type is still readable, because a catch-all that loses it leaves
/// a log line nobody can act on.
///
/// `Unknown` is a unit variant, so serde discards the tag on the way to it:
/// a peer matching on the enum alone would know it had met something it did
/// not know and not **what**. `frame_type` reads it back off the text, which
/// is the difference between `ignoring a frame of type "pack_capability"` and
/// `ignoring a frame`.
#[test]
fn an_unknown_frame_can_still_be_named() {
    assert_eq!(
        herd_proto::frame_type(r#"{"type":"pack_capability","blob":"abc"}"#).as_deref(),
        Some("pack_capability")
    );
    // A known frame too, because the function is called on whatever arrived
    // and must not be a special case for the unknown path.
    assert_eq!(
        herd_proto::frame_type(r#"{"type":"job_result_ack","job_id":"j1"}"#).as_deref(),
        Some("job_result_ack")
    );
    // And something that is not a tagged frame at all names nothing rather
    // than inventing a name.
    assert_eq!(herd_proto::frame_type("[]"), None);
    assert_eq!(herd_proto::frame_type(r#"{"no_type":1}"#), None);
}

/// A **known** frame that is malformed is still a decode error.
///
/// The catch-all must not become a swallower. `job_finished` with a missing
/// required field is a broken frame of a type this build knows, and reading
/// it as "something I do not know, ignore it" would turn a protocol bug into
/// a job that silently never finishes — strictly worse than the loop the
/// catch-all was added to stop.
#[test]
fn a_malformed_frame_of_a_known_type_is_still_an_error() {
    let err = from_frame::<ToHub>(r#"{"type":"job_finished","job_id":"j1"}"#)
        .expect_err("a known type with a field missing is not an unknown type");
    assert!(
        !err.to_string().contains("unknown variant"),
        "it must fail on the missing field, not be read as an unknown type: {err}"
    );
}

/// A log payload's wire shape, pinned: base64 in a JSON string, so that bytes
/// no JSON string could hold travel whole (D18).
#[test]
fn a_log_payload_is_base64_in_the_frame_and_comes_back_byte_for_byte() {
    // What `printf 'a\000b\377c\n'` writes.
    let written = b"a\0b\xffc\n".to_vec();
    let frame = to_frame(&ToHub::LogChunk {
        job_id: JobId::new("j1"),
        attempt: 1,
        stream: LogStream::Stdout,
        seq: 3,
        data: written.clone(),
    })
    .expect("encodes");
    assert_eq!(
        frame,
        r#"{"type":"log_chunk","job_id":"j1","attempt":1,"stream":"stdout","seq":3,"data":"YQBi/2MK"}"#
    );
    let ToHub::LogChunk { data, .. } = from_frame(&frame).expect("decodes") else {
        panic!("a log chunk decodes as a log chunk");
    };
    assert_eq!(data, written, "the bytes the process wrote were rewritten");

    // And a payload that is not base64 at all is a decode error, not an empty
    // log: a chunk Herd cannot read is never read as nothing.
    let error = from_frame::<ToHub>(
        r#"{"type":"log_chunk","job_id":"j1","attempt":1,"stream":"stdout","seq":0,"data":"not base64!"}"#,
    )
    .expect_err("a payload that is no base64 must be refused");
    assert!(!error.to_string().is_empty(), "{error}");
}

/// The two frames M2 added, pinned on the wire. The acknowledgement is what a
/// runner waits for before forgetting a result (E-44), and the wait is what
/// makes a submission detachable (E-03); both are read by the other side from
/// their tag alone.
#[test]
fn the_durability_frames_are_tagged_json_objects() {
    assert_eq!(
        to_frame(&ToHub::WaitForJob {
            job_id: JobId::new("j0000000042"),
            wants_files: true,
        })
        .expect("encodes"),
        r#"{"type":"wait_for_job","job_id":"j0000000042","wants_files":true}"#
    );
    assert_eq!(
        to_frame(&FromHub::JobResultAck {
            job_id: JobId::new("j0000000042")
        })
        .expect("encodes"),
        r#"{"type":"job_result_ack","job_id":"j0000000042"}"#
    );
}

/// The frames M3 added, pinned on the wire.
///
/// Four of them carry an attempt number, and that is the point: a runner that
/// comes back after being declared lost says things about an attempt the hub
/// has written off, and the number is the only way to tell that from a frame
/// about the attempt running now (E-21).
#[test]
fn the_link_frames_are_tagged_json_objects() {
    assert_eq!(
        to_frame(&ToHub::Health {
            jobs: vec![RunningJob {
                job_id: JobId::new("j0000000007"),
                attempt: 2,
                chunks_sent: 412,
            }],
            draining: false,
            load_milli: Some(1420),
            switch: Switch::On,
            pack_store_bytes: None,
            pack_store_budget_bytes: None,
            packs_held: None,
        })
        .expect("encodes"),
        r#"{"type":"health","jobs":[{"job_id":"j0000000007","attempt":2,"chunks_sent":412}],"draining":false,"load_milli":1420,"switch":{"position":"on"}}"#
    );
    assert_eq!(
        to_frame(&ToHub::Health {
            jobs: vec![],
            draining: true,
            load_milli: None,
            switch: Switch::Off {
                reason: "she is gaming".to_owned(),
            },
            pack_store_bytes: None,
            pack_store_budget_bytes: None,
            packs_held: None,
        })
        .expect("encodes"),
        r#"{"type":"health","jobs":[],"draining":true,"switch":{"position":"off","reason":"she is gaming"}}"#
    );
    assert_eq!(
        to_frame(&ToHub::CancelJob {
            job_id: JobId::new("j1")
        })
        .expect("encodes"),
        r#"{"type":"cancel_job","job_id":"j1"}"#
    );
    assert_eq!(
        to_frame(&ToHub::AttemptAbandoned {
            job_id: JobId::new("j1"),
            attempt: 1,
            reason: "the runner was asked to stop".to_owned(),
            at_its_ceiling: None,
        })
        .expect("encodes"),
        r#"{"type":"attempt_abandoned","job_id":"j1","attempt":1,"reason":"the runner was asked to stop"}"#
    );
    assert_eq!(
        to_frame(&FromHub::StandBy {
            reason: "runner wsl-pc is connected".to_owned(),
            retry_in_ms: 2000,
        })
        .expect("encodes"),
        r#"{"type":"stand_by","reason":"runner wsl-pc is connected","retry_in_ms":2000}"#
    );
    assert_eq!(
        to_frame(&FromHub::HealthRecorded {}).expect("encodes"),
        r#"{"type":"health_recorded"}"#
    );
    // The hub's breath: a tag, a job, and deliberately nothing else. The shape
    // is pinned here because "nothing else" is the promise — a field added to
    // it later would be a field a client might print, and this frame is one
    // nothing prints.
    assert_eq!(
        to_frame(&FromHub::StillThere {
            job_id: JobId::new("j0000000007"),
        })
        .expect("encodes"),
        r#"{"type":"still_there","job_id":"j0000000007"}"#
    );
    assert_eq!(
        to_frame(&FromHub::LogAck {
            job_id: JobId::new("j1"),
            attempt: 1,
            through_seq: 9,
        })
        .expect("encodes"),
        r#"{"type":"log_ack","job_id":"j1","attempt":1,"through_seq":9}"#
    );
    assert_eq!(
        to_frame(&FromHub::ResendLog {
            job_id: JobId::new("j1"),
            attempt: 1,
            from_seq: 4,
        })
        .expect("encodes"),
        r#"{"type":"resend_log","job_id":"j1","attempt":1,"from_seq":4}"#
    );
}

#[test]
fn every_link_frame_round_trips() {
    for (draining, switch) in [
        (false, Switch::On),
        (true, Switch::On),
        // Every position, because a hub that read a missing or mangled switch
        // as `on` would claim on a withdrawn machine (E-33).
        (
            false,
            Switch::Off {
                reason: "she is gaming".to_owned(),
            },
        ),
        (
            false,
            Switch::Drain {
                reason: "a reboot is due".to_owned(),
            },
        ),
    ] {
        round_trip_to_hub(ToHub::Health {
            jobs: vec![
                RunningJob {
                    job_id: JobId::new("j1"),
                    attempt: 1,
                    chunks_sent: 0,
                },
                RunningJob {
                    job_id: JobId::new("j2"),
                    attempt: u32::MAX,
                    chunks_sent: u64::MAX,
                },
            ],
            draining,
            load_milli: Some(u64::MAX),
            switch,
            pack_store_bytes: None,
            pack_store_budget_bytes: None,
            packs_held: None,
        });
    }
    for runners in [
        Vec::new(),
        vec![RunnerSwitch {
            name: "wsl-pc".to_owned(),
            switch: Switch::Off {
                reason: "she is gaming".to_owned(),
            },
            jobs: 1,
            stopping: false,
            facts: herd_proto::RunnerFacts::default(),
        }],
        // The facts D166 carries to a fleet listing, both of them said.
        vec![RunnerSwitch {
            name: "server".to_owned(),
            switch: Switch::On,
            jobs: 0,
            stopping: false,
            facts: herd_proto::RunnerFacts {
                version: Some("0.3.3".to_owned()),
                suspended: Some(vec![herd_proto::SuspendedCapability {
                    label: "kvm".to_owned(),
                    why: "its probe cannot be started".to_owned(),
                }]),
            },
        }],
    ] {
        round_trip_from_hub(FromHub::FleetSwitches { runners });
    }
    round_trip_to_hub(ToHub::SetSwitch {
        runner: "wsl-pc".to_owned(),
        switch: Switch::Drain {
            reason: "a reboot is due".to_owned(),
        },
    });
    round_trip_to_hub(ToHub::AskFleetSwitches {});
    // M15's operator surface: approving a runner, removing it, and asking who
    // is approved (E-48).
    round_trip_to_hub(ToHub::ApproveRunner {
        runner: "wsl-pc".to_owned(),
    });
    round_trip_to_hub(ToHub::RemoveRunner {
        runner: "wsl-pc".to_owned(),
    });
    round_trip_to_hub(ToHub::AskApprovedRunners {});
    round_trip_from_hub(FromHub::RunnerApproved {
        runner: "wsl-pc".to_owned(),
        token: "herd-runner-0123456789abcdef".to_owned(),
        said: "runner wsl-pc is approved on this hub".to_owned(),
    });
    round_trip_from_hub(FromHub::RunnerRemoved {
        said: "runner wsl-pc is removed and its token is revoked".to_owned(),
    });
    for runners in [
        Vec::new(),
        vec![herd_proto::ApprovedRunner {
            name: "wsl-pc".to_owned(),
            approved_unix_s: 1_760_000_000,
            connected: true,
            facts: herd_proto::RunnerFacts::default(),
        }],
        // What the listing an operator looks at first really carries (D166):
        // the build, and every capability the machine is not handing out.
        vec![herd_proto::ApprovedRunner {
            name: "server".to_owned(),
            approved_unix_s: 1_760_000_000,
            connected: true,
            facts: herd_proto::RunnerFacts {
                version: Some("0.3.3".to_owned()),
                suspended: Some(vec![
                    herd_proto::SuspendedCapability {
                        label: "gpu-venus".to_owned(),
                        why: "its probe has not answered yet".to_owned(),
                    },
                    herd_proto::SuspendedCapability {
                        label: "kvm".to_owned(),
                        why: "its probe cannot be started".to_owned(),
                    },
                ]),
            },
        }],
    ] {
        round_trip_from_hub(FromHub::ApprovedRunners { runners });
    }
    for confirmed in [true, false] {
        round_trip_from_hub(FromHub::SwitchSet {
            said: "runner wsl-pc is switched off (she is gaming)".to_owned(),
            confirmed,
        });
    }
    // The acknowledgement D99 added, both answers it carries.
    for persistence in [
        SwitchPersistence::Written {
            path: "/var/lib/herd/switch".to_owned(),
        },
        SwitchPersistence::NotWritten {
            why: "/var/lib/herd/switch.new cannot be written: Read-only file system".to_owned(),
        },
    ] {
        round_trip_to_hub(ToHub::SwitchApplied {
            switch: Switch::Off {
                reason: "she is gaming".to_owned(),
            },
            persistence,
        });
    }
    round_trip_from_hub(FromHub::SetSwitch {
        switch: Switch::On,
        why: "set through this hub to on".to_owned(),
    });
    round_trip_from_hub(FromHub::ReadProbesAgain {
        why: "j1 exited 75 on wsl-pc".to_owned(),
    });
    round_trip_from_hub(FromHub::WouldRun {
        lines: vec!["would run on: wsl-pc (build)".to_owned()],
    });
    round_trip_from_hub(FromHub::WouldRun { lines: Vec::new() });
    round_trip_to_hub(ToHub::CancelJob {
        job_id: JobId::new("j1"),
    });
    round_trip_to_hub(ToHub::AttemptAbandoned {
        job_id: JobId::new("j1"),
        attempt: 1,
        reason: String::new(),
        at_its_ceiling: None,
    });
    round_trip_from_hub(FromHub::HealthRecorded {});
    round_trip_from_hub(FromHub::StillThere {
        job_id: JobId::new("j1"),
    });
    round_trip_from_hub(FromHub::StandBy {
        reason: "a runner is already connected".to_owned(),
        retry_in_ms: u64::MAX,
    });
    round_trip_from_hub(FromHub::CancelJob {
        job_id: JobId::new("j1"),
        attempt: 3,
        reason: "a requester cancelled this job".to_owned(),
    });
    round_trip_from_hub(FromHub::StaleAttempt {
        job_id: JobId::new("j1"),
        attempt: 1,
        reason: "attempt 1 is not the attempt this hub is waiting for".to_owned(),
    });
    round_trip_from_hub(FromHub::LogAck {
        job_id: JobId::new("j1"),
        attempt: 1,
        through_seq: u64::MAX,
    });
    round_trip_from_hub(FromHub::ResendLog {
        job_id: JobId::new("j1"),
        attempt: 1,
        from_seq: 0,
    });
}

/// The two cadences of a quiet link are **one number**.
///
/// A runner says it is alive to the hub every [`herd_proto::HEALTH_EVERY`],
/// and the hub says it is alive to a follower every
/// [`herd_proto::STILL_THERE_EVERY`]. They are the two directions of one
/// problem, so they are deliberately the same figure: a reader who knows 15 s
/// knows both. Pinned here because the sameness is a decision and not a
/// coincidence — if one is ever moved on purpose, this is where that choice
/// is made rather than discovered.
#[test]
fn a_quiet_link_speaks_at_one_cadence_in_both_directions() {
    assert_eq!(
        herd_proto::STILL_THERE_EVERY,
        herd_proto::HEALTH_EVERY,
        "the cadence the hub keeps a follower warm at and the cadence a runner reports at \
         are one figure on purpose"
    );
}

/// The arithmetic both sides refuse a configuration on, and the figures the
/// refusal has to carry.
///
/// A silence threshold is only about silence if it can outlast a few health
/// reports. `--lost-after 8` against the 15 s cadence of E-40 burnt both
/// attempts of a live, healthy job in nineteen seconds and ended it `lost` —
/// so the figure that cannot work is refused where it is configured, with
/// both figures and the margin in the sentence.
#[test]
fn a_silence_threshold_shorter_than_three_health_reports_is_refused_by_its_figures() {
    use std::time::Duration;

    let refusal = herd_proto::silence_threshold_refusal(
        "the hub's silence threshold",
        Duration::from_secs(8),
        "the cadence it expects",
        herd_proto::HEALTH_EVERY,
    )
    .expect("eight seconds cannot outlast three reports fifteen seconds apart");
    for figure in ["8s", "15s", "45s", "3 health reports"] {
        assert!(
            refusal.contains(figure),
            "the refusal must name {figure}: {refusal}"
        );
    }

    // Exactly the margin is enough; a hair under it is not. The boundary is
    // part of the contract, because it is the number an operator will sit on.
    let margin = herd_proto::HEALTH_EVERY * herd_proto::SILENCE_MARGIN;
    assert!(
        herd_proto::silence_threshold_refusal("t", margin, "i", herd_proto::HEALTH_EVERY).is_none()
    );
    assert!(herd_proto::silence_threshold_refusal(
        "t",
        margin - Duration::from_millis(1),
        "i",
        herd_proto::HEALTH_EVERY
    )
    .is_some());

    // The deployment's own figures: 60 s against a report every 15 s.
    assert!(herd_proto::silence_threshold_refusal(
        "t",
        Duration::from_secs(60),
        "i",
        herd_proto::HEALTH_EVERY
    )
    .is_none());
}

/// A health cadence of zero is not a cadence: nothing is ever reported at it,
/// so no threshold can be judged against it. `--health-every 0` used to be
/// read as "no cadence to judge" and let through — on the hub's own startup
/// check, the runner's own startup check, and the hub's handshake check of a
/// runner's announced cadence, because all three turn to this one function.
/// A runner at `--health-every 0` paired with a hub at
/// `--health-every 1 --lost-after 3` passed every one of them, then never
/// reported, and a quiet job burnt both its attempts and ended `lost` —
/// exactly what this function exists to refuse before a job is ever placed.
#[test]
fn a_health_cadence_of_zero_is_refused_naming_the_figure() {
    use std::time::Duration;

    let refusal = herd_proto::silence_threshold_refusal(
        "the hub's silence threshold",
        Duration::ZERO,
        "its own health cadence",
        Duration::ZERO,
    )
    .expect("a cadence of zero is not a cadence to judge anything against");
    assert!(
        refusal.contains("0s"),
        "the refusal must name the figure: {refusal}"
    );
    assert!(
        refusal.contains("its own health cadence"),
        "and which figure it is: {refusal}"
    );

    // Not exempted by a generous threshold either: zero stays zero no matter
    // what it is being judged against.
    assert!(herd_proto::silence_threshold_refusal(
        "t",
        Duration::from_secs(3600),
        "i",
        Duration::ZERO
    )
    .is_some());
}

/// The capability algebra on the wire (M5). These shapes land in durable
/// queue entries from M6 onwards, so they are pinned here rather than left to
/// be discovered by a migration.
#[test]
fn the_capability_algebra_is_tagged_json_objects() {
    assert_eq!(
        to_frame(&Capability::shared("reference")).expect("encodes"),
        r#"{"name":"reference","kind":{"type":"shared"}}"#
    );
    assert_eq!(
        to_frame(&Capability::counted("build", 2)).expect("encodes"),
        r#"{"name":"build","kind":{"type":"counted","slots":2}}"#
    );
    assert_eq!(
        to_frame(&Capability::exclusive("nightly")).expect("encodes"),
        r#"{"name":"nightly","kind":{"type":"exclusive"}}"#
    );
    // A lease is counted(1) and a probe, with no kind of its own (E-27).
    assert_eq!(
        to_frame(&Capability::lease(
            "metal-pc",
            BusyProbe::lock_file("/run/herd/metal-pc")
        ))
        .expect("encodes"),
        r#"{"name":"metal-pc","kind":{"type":"counted","slots":1},"#.to_owned()
            + r#""busy_probe":{"type":"lock_file","path":"/run/herd/metal-pc"}}"#
    );
    // Each origin is its own flag, so all three can be true at once (E-24).
    assert_eq!(
        to_frame(&CapabilityOrigins {
            probed: true,
            declared: false,
            denied: true,
        })
        .expect("encodes"),
        r#"{"probed":true,"declared":false,"denied":true}"#
    );
    assert_eq!(
        to_frame(&Availability::busy("the lock file /run/x is there")).expect("encodes"),
        r#"{"type":"busy","reason":"the lock file /run/x is there"}"#
    );
    assert_eq!(
        to_frame(&Availability::Free).expect("encodes"),
        r#"{"type":"free"}"#
    );
}

/// A command probe's two exit codes default, so a configuration that gives
/// neither means "0 is busy, 1 is free".
#[test]
fn a_command_probe_s_exit_codes_default() {
    let probe: BusyProbe =
        from_frame(r#"{"type":"command","command":["/bin/test","-e","/run/x"]}"#).expect("decodes");
    assert_eq!(
        probe,
        BusyProbe::Command {
            command: vec!["/bin/test".to_owned(), "-e".to_owned(), "/run/x".to_owned()],
            busy_exit_code: 0,
            free_exit_code: 1,
        }
    );
}

/// A label set is canonical whichever way it arrives: a peer that sends the
/// same two labels in another order, or twice, sends the same set. M6's
/// deduplication keys on this (E-05).
#[test]
fn a_label_set_arrives_canonical() {
    let one: LabelSet = from_frame(r#"["reference","build","build"]"#).expect("decodes");
    let other: LabelSet = from_frame(r#"["build","reference"]"#).expect("decodes");
    assert_eq!(one, other);
    assert_eq!(to_frame(&one).expect("encodes"), r#"["build","reference"]"#);
}

/// A label a peer sent that is not a label is a decode error naming it, never
/// a label that silently matches nothing (the rule D15 set for outcomes).
#[test]
fn a_label_that_is_not_a_label_is_refused_by_name() {
    for bad in [r#"["Build"]"#, r#"["with space"]"#, r#"[""]"#, r#"["a,b"]"#] {
        let error = from_frame::<LabelSet>(bad).expect_err("{bad} must be refused");
        assert!(!error.to_string().is_empty(), "{bad}: {error}");
    }
    // And an offer with two entries under one name is refused too: one runner
    // offers a capability once, with one kind.
    let error = from_frame::<CapabilityOffer>(
        r#"[{"name":"build","kind":{"type":"shared"},"origins":{"declared":true}},
            {"name":"build","kind":{"type":"exclusive"},"origins":{"declared":true}}]"#,
    )
    .expect_err("a duplicate name must be refused");
    assert!(error.to_string().contains("build"), "{error}");
}

/// The symmetric closure is part of what crosses the wire: an offer decoded
/// from a configuration that named one direction carries both.
#[test]
fn the_exclusion_closure_survives_the_wire() {
    let offer: CapabilityOffer = from_frame(
        r#"[{"name":"build","kind":{"type":"shared"},"excludes":["nightly"],
             "origins":{"declared":true}},
            {"name":"nightly","kind":{"type":"exclusive"},"origins":{"declared":true}}]"#,
    )
    .expect("decodes");
    assert!(offer
        .get("nightly")
        .expect("offered")
        .capability
        .excludes
        .contains("build"));
}

/// The claim that keeps `PROTOCOL_VERSION` at 1 while a hub is deployed
/// (D131): a `job_finished` frame written by a build that has never heard of
/// a job's outputs still decodes, and means "this peer collected nothing".
///
/// This is the one test standing between an additive field and D128's redial
/// loop. The frames below are the bytes the **deployed** binaries send — no
/// `outputs` key anywhere — and they are written out literally rather than
/// built from today's types, because a frame built from today's types would
/// pass even if the field had been made required.
#[test]
fn a_verdict_from_a_build_that_predates_a_job_s_outputs_still_decodes() {
    let to_hub = r#"{"type":"job_finished","job_id":"j0000000012","attempt":1,
        "outcome":{"type":"exited","code":0},"chunks_sent":7}"#;
    match from_frame::<ToHub>(to_hub).expect("an older runner's verdict decodes") {
        ToHub::JobFinished { outputs, .. } => assert!(
            outputs.is_silent(),
            "an absent field means this peer collected nothing: {outputs:?}"
        ),
        other => panic!("that is not a verdict: {other:?}"),
    }

    let from_hub = r#"{"type":"job_finished","job_id":"j0000000012",
        "outcome":{"type":"exited","code":1}}"#;
    match from_frame::<FromHub>(from_hub).expect("an older hub's verdict decodes") {
        FromHub::JobFinished { outputs, .. } => assert!(outputs.is_silent(), "{outputs:?}"),
        other => panic!("that is not a verdict: {other:?}"),
    }

    // And the other direction: a job that keeps nothing puts no `outputs` key
    // on the wire at all, so today's build sends an older peer exactly the
    // bytes it sent before. That is what makes this additive in both
    // directions rather than only in one.
    let today = to_frame(&FromHub::JobFinished {
        job_id: JobId::new("j0000000012"),
        outcome: JobOutcome::Exited { code: 0 },
        metrics: JobMetrics::default(),
        outputs: herd_proto::JobOutputs::default(),
        commit: None,
    })
    .expect("encodes");
    assert!(
        !today.contains("outputs"),
        "a job that kept nothing must put nothing on the wire: {today}"
    );
    // The same of `commit`, added later and for the same reasons (D176):
    // a hub with nothing to say about a job's revision sends an older
    // requester the bytes it has always sent.
    assert!(
        !today.contains("commit"),
        "a job with no revision to report must put nothing on the wire: {today}"
    );
}

/// A revision really does cross the wire, both ways, on both frames that
/// carry one (D176).
///
/// The one property worth a test of its own: a divergence is only an oracle
/// if the two strings survive encoding, and a field that silently dropped
/// them would still make every other test here pass.
#[test]
fn a_reported_revision_crosses_the_wire_on_the_step_and_on_the_verdict() {
    let step = StepRun {
        step: Step::Fetch,
        started_unix_ms: 1_791_878_248_400,
        duration_ms: 1_200,
        exit_code: Some(0),
        ended: None,
        ran_commit: Some("9f41".repeat(10)),
    };
    round_trip_to_hub(ToHub::StepFinished {
        job_id: JobId::new("j1"),
        attempt: 1,
        step: step.clone(),
    });
    round_trip_to_hub(ToHub::JobFinished {
        job_id: JobId::new("j1"),
        attempt: 1,
        outcome: JobOutcome::Exited { code: 0 },
        chunks_sent: 1,
        metrics: JobMetrics::default(),
        steps: vec![step],
        outputs: herd_proto::JobOutputs::default(),
    });
    round_trip_from_hub(FromHub::JobFinished {
        job_id: JobId::new("j1"),
        outcome: JobOutcome::Exited { code: 0 },
        metrics: JobMetrics::default(),
        outputs: herd_proto::JobOutputs::default(),
        commit: Some(herd_proto::CommitRun {
            declared: "cb5a".repeat(10),
            ran: Some("9f41".repeat(10)),
        }),
    });
    // And silence, which has to survive as silence rather than as a string
    // nobody said.
    round_trip_from_hub(FromHub::JobFinished {
        job_id: JobId::new("j1"),
        outcome: JobOutcome::Exited { code: 0 },
        metrics: JobMetrics::default(),
        outputs: herd_proto::JobOutputs::default(),
        commit: Some(herd_proto::CommitRun {
            declared: "cb5a".repeat(10),
            ran: None,
        }),
    });
}

/// **The direction an absent `wants_files` has to default in** (D167).
///
/// A requester asks the hub to hold a job's verdict until the job's output
/// files have all reached it, because one that fetches on the verdict while
/// they are still crossing gets a short collection. The field is how a
/// requester that wants no files declines that wait — and the frames that
/// leave it out are the ones from every `herdctl` built before it existed.
///
/// So the default has to be **true**, and this is the test that says so.
/// Defaulted the other way, the upgrade that fixed the defect would have
/// handed every deployed requester the early verdict and put it straight
/// back into the defect; and `bool`'s own `Default` is `false`, so the safe
/// answer here is the one a reader would least expect. The frames are
/// written out literally rather than built from today's types, for the
/// reason the test above gives: a frame built from today's types would pass
/// even if the default had been inverted.
#[test]
fn a_wait_that_predates_wants_files_is_read_as_wanting_them() {
    let wait = r#"{"type":"wait_for_job","job_id":"j0000000042"}"#;
    match from_frame::<ToHub>(wait).expect("an older requester's wait decodes") {
        ToHub::WaitForJob { wants_files, .. } => assert!(
            wants_files,
            "a wait that does not say must be read as wanting this job's files: \
             silence fails towards waiting, never towards a short collection"
        ),
        other => panic!("that is not a wait: {other:?}"),
    }

    let submission = r#"{"type":"submit_job","asked":{"command":["true"],
        "priority":"step"}}"#;
    match from_frame::<ToHub>(submission).expect("an older requester's submission decodes") {
        ToHub::SubmitJob { wants_files, .. } => assert!(wants_files),
        other => panic!("that is not a submission: {other:?}"),
    }

    // And said out loud when it is given, in both directions: an operator
    // reading a frame off a link sees which answer this requester gave,
    // rather than having to know what an absent field means.
    let says_no = to_frame(&ToHub::WaitForJob {
        job_id: JobId::new("j0000000042"),
        wants_files: false,
    })
    .expect("encodes");
    assert!(
        says_no.contains(r#""wants_files":false"#),
        "a requester that declines the wait says so on the wire: {says_no}"
    );
}

/// `jobs_at_once` is additive in **both** directions, which is what the rule
/// at `docs/decisions.md:7293` and `herd-hub/src/job.rs:44-51` actually ask
/// for: the field moved no version of its own.
///
/// A field is additive only when a peer that does not know it and a peer that
/// does can each read what the other wrote, **and** when what is missing
/// means exactly what its default says. Both halves are asserted here,
/// because only one of them is about serde:
///
/// * **A hello from a peer built before the field** — written out literally
///   rather than generated, so that a later edit to the struct cannot quietly
///   change what "older" means — decodes with `jobs_at_once: None`. That is
///   "it did not say", and §1 of the doc comment on the field is that this is
///   never read as unlimited, never as zero and never as a figure derived
///   from `host`.
/// * **A hello from a peer built after it** is read by a deserialiser that
///   has never heard of the field. Modelled by a struct of exactly the fields
///   the older `Hello` had: serde ignores what it does not know, so the older
///   hub reads every figure it used to read and is merely one fact poorer.
///
/// ## What this test says about [`PROTOCOL_VERSION`], and what it does not
///
/// It used to be called *"without moving the version"* and its fixture was
/// labelled *"the frame as `main` emits it"*. **Both became false at D199**,
/// which moved the constant 2 → 3 for M11's pack half — and neither went red,
/// because `herd-proto` validates no version and the last assertion compares
/// the constant against a frame this test built *from* the constant. A green
/// test whose claim is false is worse than a red one, so the claim is stated
/// properly here instead:
///
/// * the literal fixture is a hello from a peer built **before the field**,
///   which was also before the bump, so it announces 2. **That is a frame the
///   hub now refuses at the handshake** ([`herd_proto::version_mismatch_reason`]),
///   and this test asserts nothing about whether it would be admitted — only
///   that serde reads every field of it, which is what additiveness means and
///   is a separate question from admission. The test says so out loud below;
/// * the additiveness claim is then made again at **this build's** version,
///   where it is not entangled with the bump at all: a hello announcing
///   [`PROTOCOL_VERSION`] and carrying no `jobs_at_once` key decodes as
///   `None` exactly the same way.
#[test]
fn a_hello_carries_its_job_ceiling_in_both_directions_without_moving_the_version() {
    // ---- Older peer → today's code.
    //
    // A peer built before the field, and therefore before D199's bump: it
    // announces protocol 2, which the hub refuses at the handshake today.
    // Nothing here is about admission — only about serde reading every field
    // of a frame an older build wrote, which is the whole of what "additive"
    // claims.
    let from_before = r#"{"type":"hello","protocol_version":2,"peer_version":"0.3.7",
        "role":"runner","name":"wsl-pc","instance":"run-4242",
        "host":{"memory_mib":17408,"threads":20,"memory_floor_mib":3481},
        "preference":10,"switch":{"position":"on"}}"#;
    let ToHub::Hello {
        jobs_at_once,
        host,
        preference,
        ..
    } = from_frame::<ToHub>(from_before).expect("an older peer's hello still decodes")
    else {
        panic!("that frame is a hello");
    };
    assert_eq!(
        jobs_at_once, None,
        "a peer that was built before the field said nothing about a ceiling, and \"did not \
         say\" is the only thing that may be read out of its silence"
    );
    // And the rest of the frame is untouched by the new field: an additive
    // change that moved another figure would not be additive.
    assert_eq!(host.memory_mib, Some(17408));
    assert_eq!(preference, 10);

    // Said out loud rather than left as the reader's inference: that fixture
    // is **not** a frame this hub admits. It decodes, which is what this test
    // is about, and it would be refused at the handshake, which this test
    // does not test. The two are different questions and the old name of this
    // test ran them together.
    let ToHub::Hello {
        protocol_version, ..
    } = from_frame::<ToHub>(from_before).expect("decodes")
    else {
        panic!("that frame is a hello");
    };
    assert_ne!(
        protocol_version, PROTOCOL_VERSION,
        "the fixture is a pre-bump peer on purpose; if it ever announces this build's version \
         the fixture has stopped standing in for an older one"
    );

    // ---- And the same claim at this build's own version, where it is not
    // entangled with D199's bump: no `jobs_at_once` key still means "did not
    // say", whatever the number in the handshake is.
    let today = format!(
        r#"{{"type":"hello","protocol_version":{PROTOCOL_VERSION},"peer_version":"0.4.1",
        "role":"runner","name":"wsl-pc","switch":{{"position":"on"}}}}"#
    );
    let ToHub::Hello { jobs_at_once, .. } =
        from_frame::<ToHub>(&today).expect("a hello at this version decodes")
    else {
        panic!("that frame is a hello");
    };
    assert_eq!(
        jobs_at_once, None,
        "the field's default is what it is at every protocol version, and that is the claim"
    );

    // ---- Today's peer → an older deserialiser.
    let with_ceiling = to_frame(&ToHub::Hello {
        protocol_version: PROTOCOL_VERSION,
        peer_version: "0.3.8".to_owned(),
        role: Role::Runner,
        token: None,
        name: Some("wsl-pc".to_owned()),
        instance: Some("run-4242".to_owned()),
        state_root: None,
        health_every_ms: Some(15_000),
        capabilities: CapabilityOffer::empty(),
        host: HostSize::new(Some(17 * 1024), Some(20)).with_floor(Some(3481)),
        jobs_at_once: Some(7),
        preference: 10,
        load_milli: None,
        switch: Switch::On,
        mesh_endpoint: None,
    })
    .expect("encodes");
    assert!(
        with_ceiling.contains(r#""jobs_at_once":7"#),
        "the figure has to be on the wire to be worth anything: {with_ceiling}"
    );

    /// The `Hello` an older hub compiled against: every field it knew, and
    /// not the one it did not. Serde ignores what it has no field for, which
    /// is the whole of why this change is additive.
    #[derive(serde::Deserialize)]
    struct HelloAsAnOlderHubReadsIt {
        protocol_version: u32,
        peer_version: String,
        name: Option<String>,
        #[serde(default)]
        health_every_ms: Option<u64>,
        #[serde(default)]
        host: HostSize,
        #[serde(default)]
        preference: i32,
    }
    let older: HelloAsAnOlderHubReadsIt =
        serde_json::from_str(&with_ceiling).expect("an older hub reads a newer runner's hello");
    // Not a version check: `with_ceiling` was built *from* the constant, so
    // this says serde carried the field across and nothing about what any
    // version is. The version check is the hub's, on equality, at the
    // handshake.
    assert_eq!(older.protocol_version, PROTOCOL_VERSION);
    assert_eq!(older.peer_version, "0.3.8");
    assert_eq!(older.name.as_deref(), Some("wsl-pc"));
    assert_eq!(older.health_every_ms, Some(15_000));
    assert_eq!(older.host.memory_mib, Some(17 * 1024));
    assert_eq!(older.preference, 10);

    // And absent really is absent on the wire, not a zero that an older hub
    // would read as a ceiling of none: `skip_serializing_if` is the half of
    // the contract that the `#[serde(default)]` above cannot state.
    let silent = to_frame(&ToHub::Hello {
        protocol_version: PROTOCOL_VERSION,
        peer_version: "0.3.8".to_owned(),
        role: Role::Requester,
        token: None,
        name: None,
        instance: None,
        state_root: None,
        health_every_ms: None,
        capabilities: CapabilityOffer::empty(),
        host: HostSize::default(),
        jobs_at_once: None,
        preference: 0,
        load_milli: None,
        switch: Switch::On,
        mesh_endpoint: None,
    })
    .expect("encodes");
    assert!(
        !silent.contains("jobs_at_once"),
        "a peer with no ceiling to declare must not put the word on the wire at all, or \
         \"did not say\" and \"said nothing fits\" become the same bytes: {silent}"
    );
}

/// **A job that no machine had room for ends as that, and says so in fields**
/// (D182).
///
/// The one-way cost of a new `JobOutcome` variant, asserted rather than only
/// written down: this build reads every ending an older one wrote, and an
/// older build reads this one as nothing at all. Neither `JobOutcome` nor
/// `JobState` carries `#[serde(other)]`, by design — a state this Herd does
/// not know is a read error naming it rather than a job quietly misread — so
/// the second half of that is what it must be, and the test stands in for the
/// older build with a struct of exactly the variants it had.
#[test]
fn a_job_refused_as_full_travels_with_its_figures_and_is_unreadable_to_an_older_peer() {
    let refused = JobOutcome::RefusedAsFull {
        reason: "no machine had room".to_owned(),
        refusals: 8,
        refusals_allowed: 8,
        runners: vec![
            herd_proto::RunnerThatWasFull {
                name: "capped-runner".to_owned(),
                holding: Some(2),
            },
            herd_proto::RunnerThatWasFull {
                name: "wsl-pc".to_owned(),
                holding: None,
            },
        ],
    };
    // The cause a program reads **is** the serde tag, by construction (D166).
    assert_eq!(refused.cause(), "refused_as_full");
    let wire = serde_json::to_string(&refused).expect("an outcome writes");
    assert!(
        wire.contains(r#""type":"refused_as_full""#),
        "the tag is the cause: {wire}"
    );
    assert_eq!(
        serde_json::from_str::<JobOutcome>(&wire).expect("and reads back"),
        refused,
        "the figures cross unchanged"
    );
    // A machine that could not say how much it was holding is absent from
    // the wire, not zero: a machine holding nothing would have taken the job.
    assert!(
        wire.contains(r#"{"name":"wsl-pc"}"#),
        "an absent figure is absent: {wire}"
    );

    // Today → an older peer. This is the half that does **not** work, and it
    // is the documented cost of the variant rather than a surprise. The
    // fields are never read: what matters is the shape serde is given.
    #[allow(dead_code)]
    #[derive(serde::Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    enum OutcomeAsAnOlderHubReadsIt {
        Exited { code: i32 },
        Lost { reason: String },
        Impossible { reason: String },
        SetupFailed { reason: String },
    }
    let read_back = serde_json::from_str::<OutcomeAsAnOlderHubReadsIt>(&wire);
    assert!(
        read_back.is_err(),
        "a hub older than this one must refuse to read this record and name what it found, \
         not read it as something else; if this ever succeeds, something grew a catch-all"
    );
    let why = read_back.err().expect("an error").to_string();
    assert!(
        why.contains("refused_as_full"),
        "and the error names the ending it does not know: {why}"
    );

    // And the direction that does work, which is the one that matters for a
    // state root: every ending an older hub wrote is still read by this one.
    for older in [
        r#"{"type":"exited","code":0}"#,
        r#"{"type":"lost","reason":"its runner went away"}"#,
        r#"{"type":"impossible","reason":"two labels that exclude each other"}"#,
        r#"{"type":"setup_failed","reason":"a path it may not read"}"#,
    ] {
        serde_json::from_str::<JobOutcome>(older)
            .unwrap_or_else(|why| panic!("this build must read {older}: {why}"));
    }
}

// ---------------------------------------------------------------------------
// `ToHub::JobFact`: the one frame six silences need
// ---------------------------------------------------------------------------

/// Every reason code round-trips, and each one is on the wire as the name the
/// hub matches on.
///
/// The codes were enumerated against the six places in this build where a
/// runner or a requester concludes something about a job and has only its own
/// stderr to say it with — one code per silence, with the sites named in
/// `FactReason`'s own doc comment. **Nothing sends one yet**: what this test
/// asserts is that the contract for ending each of them exists and is
/// numbered, not that any of them has been ended.
#[test]
fn every_fact_reason_round_trips_under_the_code_the_hub_matches_on() {
    for (reason, code) in [
        (FactReason::JobFilesNotHeld, "job_files_not_held"),
        (FactReason::JobFilesLost, "job_files_lost"),
        (
            FactReason::JobFilesTransferStopped,
            "job_files_transfer_stopped",
        ),
        (FactReason::VerdictUnattributable, "verdict_unattributable"),
        (
            FactReason::VerdictDiscardedAfterRefusal,
            "verdict_discarded_after_refusal",
        ),
        (FactReason::SubmissionSkipped, "submission_skipped"),
    ] {
        assert_eq!(reason.code(), code);
        assert!(reason.is_known(), "{code} is one this build was taught");
        let fact = ToHub::JobFact {
            job_id: Some(JobId::new("j0000000042")),
            attempt: Some(2),
            reason: reason.clone(),
            detail: "files/out.tar: No such file or directory".to_owned(),
        };
        let wire = to_frame(&fact).expect("encodes");
        assert!(
            wire.contains(&format!(r#""reason":"{code}""#)),
            "a plain string on the wire, so a log line and a record read the same: {wire}"
        );
        assert!(wire.contains(r#""type":"job_fact""#), "{wire}");
        assert_eq!(
            from_frame::<ToHub>(&wire).expect("decodes"),
            fact,
            "a reason that does not round-trip is a fact the hub acts on wrongly: {wire}"
        );
    }
}

/// **A seventh reason costs nothing**, and that is the point of
/// [`FactReason::Other`].
///
/// A code this build has not been taught decodes into `Other` carrying the
/// code itself, so an older hub reads the whole frame, has the sentence to
/// log and the code to name, and acts on nothing — the same shrug
/// `ToHub::Unknown` is, one level down. That is why the reason set is *not*
/// the one-shot half of this frame: only the variant is.
#[test]
fn a_reason_this_build_was_not_taught_decodes_and_carries_its_code() {
    let wire = format!(
        r#"{{"type":"job_fact","job_id":"j0000000042","reason":"pack_store_full","detail":"{}"}}"#,
        "the store is at its budget and this pack is 401 MiB"
    );
    let decoded: ToHub = from_frame(&wire).expect(
        "an unknown reason must not be a decode error: that kills the link and the peer redials \
         into the same frame (D128)",
    );
    match decoded {
        ToHub::JobFact {
            reason,
            detail,
            job_id,
            attempt,
        } => {
            assert_eq!(reason, FactReason::Other("pack_store_full".to_owned()));
            assert!(!reason.is_known(), "so the hub logs and acts on nothing");
            assert_eq!(
                reason.code(),
                "pack_store_full",
                "the code is carried verbatim, or the log line cannot name it"
            );
            assert!(detail.contains("401 MiB"), "and the sentence survives");
            assert_eq!(job_id, Some(JobId::new("j0000000042")));
            assert_eq!(attempt, None, "absent, because this fact is about the job");
        }
        other => panic!("that is a job_fact: {other:?}"),
    }

    // It goes back out under the code it came in with, so relaying one does
    // not rename it.
    let out = to_frame(&ToHub::JobFact {
        job_id: None,
        attempt: None,
        reason: FactReason::Other("pack_store_full".to_owned()),
        detail: "x".to_owned(),
    })
    .expect("encodes");
    assert!(out.contains(r#""reason":"pack_store_full""#), "{out}");
}

/// **No job id is a real case**, and it is the one that makes a single frame
/// cover all six.
///
/// E-41's skip is a requester saying a job was *not* submitted: there is no
/// id, because the hub never minted one, and `coverage.md` records the
/// deviation as *"not closeable by hub work"*. It is closeable by protocol
/// work, and this is the shape of it — so `job_id` is absent from the wire
/// rather than an empty string the hub would have to read as either.
#[test]
fn a_fact_about_a_job_that_was_never_submitted_carries_no_id() {
    let skipped = ToHub::JobFact {
        job_id: None,
        attempt: None,
        reason: FactReason::SubmissionSkipped,
        detail: "chatons-os is at f3a19c2 and the last run was f3a19c2: nothing moved".to_owned(),
    };
    let wire = to_frame(&skipped).expect("encodes");
    assert!(
        !wire.contains(r#""job_id":"#),
        "there is no job to name: {wire}"
    );
    assert!(!wire.contains(r#""attempt":"#), "{wire}");
    assert_eq!(from_frame::<ToHub>(&wire).expect("decodes"), skipped);
    assert_eq!(
        wire,
        r#"{"type":"job_fact","reason":"submission_skipped","detail":"chatons-os is at f3a19c2 and the last run was f3a19c2: nothing moved"}"#,
        "the whole frame, pinned: four fields and two of them absent"
    );
}

// ---------------------------------------------------------------------------
// The nine fields the pack half adds to five existing messages
// ---------------------------------------------------------------------------

/// Each of the nine is **absent** from the wire when it says nothing, and
/// each decodes from a frame that never carried it.
///
/// Both halves are the additiveness claim, and neither is worth anything
/// without the other: absent-when-empty is what keeps every pinned frame in
/// this file true, and decoding-when-absent is what lets a field be added
/// without moving `PROTOCOL_VERSION` again (see the constant's own doc).
#[test]
fn the_pack_fields_are_absent_when_silent_and_decode_when_they_were_never_there() {
    let hello = to_frame(&ToHub::Hello {
        protocol_version: PROTOCOL_VERSION,
        peer_version: "0.4.1".to_owned(),
        role: Role::Runner,
        token: None,
        name: None,
        instance: None,
        state_root: None,
        health_every_ms: None,
        capabilities: CapabilityOffer::empty(),
        host: HostSize::default(),
        jobs_at_once: None,
        preference: 0,
        load_milli: None,
        switch: Switch::On,
        mesh_endpoint: None,
    })
    .expect("encodes");
    assert!(
        !hello.contains("mesh_endpoint"),
        "an outbound-only runner says nothing, it does not say \"\": {hello}"
    );

    let health = to_frame(&ToHub::Health {
        jobs: Vec::new(),
        draining: false,
        load_milli: None,
        switch: Switch::On,
        pack_store_bytes: None,
        pack_store_budget_bytes: None,
        packs_held: None,
    })
    .expect("encodes");
    for field in ["pack_store_bytes", "pack_store_budget_bytes", "packs_held"] {
        assert!(
            !health.contains(field),
            "absent is not zero — zero is a measurement, and it is the one an empty store \
             makes: {health}"
        );
    }

    let welcome = to_frame(&FromHub::Welcome {
        protocol_version: PROTOCOL_VERSION,
        hub_version: "0.4.1".to_owned(),
        state_root: "root-abc".to_owned(),
        version_note: None,
        keeper_for: Vec::new(),
    })
    .expect("encodes");
    assert!(!welcome.contains("keeper_for"), "{welcome}");

    let assignment = to_frame(&FromHub::AssignJob {
        job_id: JobId::new("j1"),
        attempt: 1,
        command: vec!["true".to_owned()],
        declaration: None,
        timeout_s: None,
        packs_in: Vec::new(),
        packs_out: BTreeMap::new(),
    })
    .expect("encodes");
    assert!(
        !assignment.contains("packs_in") && !assignment.contains("packs_out"),
        "every job this build runs needs no pack and leaves none: {assignment}"
    );

    let asked = to_frame(&ToHub::SubmitJob {
        wants_files: false,
        announce_end: false,
        asked: Asked {
            command: vec!["true".to_owned()],
            labels: LabelSet::default(),
            priority: Priority::Step,
            build_thread_mib: None,
            memory_mib: None,
            tmpfs_mib: None,
            deadline_unix_s: None,
            after: None,
            requeue_exit_code: None,
            declaration: None,
            labels_from: None,
            timeout_s: None,
            needs: Vec::new(),
            leaves: BTreeMap::new(),
        },
    })
    .expect("encodes");
    assert!(
        !asked.contains("needs") && !asked.contains("leaves"),
        "{asked}"
    );

    // And the other direction: frames as a peer built before any of this
    // emits them. Each must decode, with the new field at its default.
    let before = format!(
        r#"{{"type":"hello","protocol_version":{PROTOCOL_VERSION},"peer_version":"0.4.1",
        "role":"runner","switch":{{"position":"on"}}}}"#
    );
    let ToHub::Hello { mesh_endpoint, .. } = from_frame(&before).expect("an older hello decodes")
    else {
        panic!("that is a hello");
    };
    assert_eq!(mesh_endpoint, None);

    let before = r#"{"type":"health","jobs":[],"draining":false,"switch":{"position":"on"}}"#;
    let ToHub::Health {
        pack_store_bytes,
        pack_store_budget_bytes,
        packs_held,
        ..
    } = from_frame(before).expect("an older health report decodes")
    else {
        panic!("that is a health report");
    };
    assert_eq!(
        (pack_store_bytes, pack_store_budget_bytes, packs_held),
        (None, None, None)
    );

    let before = format!(
        r#"{{"type":"welcome","protocol_version":{PROTOCOL_VERSION},"hub_version":"0.4.1",
        "state_root":"r"}}"#
    );
    let FromHub::Welcome { keeper_for, .. } =
        from_frame(&before).expect("an older welcome decodes")
    else {
        panic!("that is a welcome");
    };
    assert!(
        keeper_for.is_empty(),
        "no custody, which is what it has now"
    );

    let before = r#"{"type":"assign_job","job_id":"j1","attempt":1,"command":["true"]}"#;
    let FromHub::AssignJob {
        packs_in,
        packs_out,
        ..
    } = from_frame(before).expect("an older assignment decodes")
    else {
        panic!("that is an assignment");
    };
    assert!(packs_in.is_empty() && packs_out.is_empty());

    let before = r#"{"type":"submit_job","asked":{"command":["true"],"priority":"step"}}"#;
    let ToHub::SubmitJob { asked, .. } = from_frame(before).expect("an older submission decodes")
    else {
        panic!("that is a submission");
    };
    assert!(asked.needs.is_empty() && asked.leaves.is_empty());
}

/// And each of the nine carries what it is given, there and back.
///
/// `leaves` and `packs_out` are maps, so the ordering is the `BTreeMap`'s and
/// not an insertion order a reader would have to hope about: two keys go in
/// out of order and come back sorted, which is what makes a frame carrying
/// them comparable byte for byte.
#[test]
fn the_pack_fields_carry_what_they_are_given() {
    let hello = ToHub::Hello {
        protocol_version: PROTOCOL_VERSION,
        peer_version: "0.4.1".to_owned(),
        role: Role::Runner,
        token: None,
        name: Some("wsl-pc".to_owned()),
        instance: None,
        state_root: None,
        health_every_ms: None,
        capabilities: CapabilityOffer::empty(),
        host: HostSize::default(),
        jobs_at_once: None,
        preference: 0,
        load_milli: None,
        switch: Switch::On,
        mesh_endpoint: Some("100.71.4.9:7711".to_owned()),
    };
    let wire = to_frame(&hello).expect("encodes");
    assert!(wire.contains("100.71.4.9:7711"), "{wire}");
    assert_eq!(from_frame::<ToHub>(&wire).expect("decodes"), hello);

    let health = ToHub::Health {
        jobs: Vec::new(),
        draining: false,
        load_milli: None,
        switch: Switch::On,
        pack_store_bytes: Some(421_138_432),
        pack_store_budget_bytes: Some(40 << 30),
        packs_held: Some(7),
    };
    let wire = to_frame(&health).expect("encodes");
    assert_eq!(from_frame::<ToHub>(&wire).expect("decodes"), health);

    // Zero is a measurement and reaches the hub as one: an empty store is
    // not a store that did not say.
    let empty_store = ToHub::Health {
        jobs: Vec::new(),
        draining: false,
        load_milli: None,
        switch: Switch::On,
        pack_store_bytes: Some(0),
        pack_store_budget_bytes: Some(40 << 30),
        packs_held: Some(0),
    };
    let wire = to_frame(&empty_store).expect("encodes");
    assert!(
        wire.contains(r#""pack_store_bytes":0"#) && wire.contains(r#""packs_held":0"#),
        "an empty store is a figure, not a silence: {wire}"
    );

    let welcome = FromHub::Welcome {
        protocol_version: PROTOCOL_VERSION,
        hub_version: "0.4.1".to_owned(),
        state_root: "root-abc".to_owned(),
        version_note: None,
        keeper_for: vec!["pack".to_owned(), "reference".to_owned()],
    };
    let wire = to_frame(&welcome).expect("encodes");
    assert_eq!(from_frame::<FromHub>(&wire).expect("decodes"), welcome);

    let mut leaves = BTreeMap::new();
    leaves.insert("reference".to_owned(), "ref".to_owned());
    leaves.insert("pack".to_owned(), "pack".to_owned());
    let asked = Asked {
        command: vec!["true".to_owned()],
        labels: LabelSet::default(),
        priority: Priority::Step,
        build_thread_mib: None,
        memory_mib: None,
        tmpfs_mib: None,
        deadline_unix_s: None,
        after: None,
        requeue_exit_code: None,
        declaration: None,
        labels_from: None,
        timeout_s: None,
        needs: vec!["pack".to_owned()],
        leaves: leaves.clone(),
    };
    let wire = to_frame(&ToHub::SubmitJob {
        asked: asked.clone(),
        wants_files: false,
        announce_end: false,
    })
    .expect("encodes");
    assert!(
        wire.find(r#""pack":"pack""#) < wire.find(r#""reference":"ref""#),
        "sorted by the map and not by the order they were inserted: {wire}"
    );
    let ToHub::SubmitJob { asked: back, .. } = from_frame(&wire).expect("decodes") else {
        panic!("that is a submission");
    };
    assert_eq!(back, asked);

    let assignment = FromHub::AssignJob {
        job_id: JobId::new("j1"),
        attempt: 1,
        command: vec!["true".to_owned()],
        declaration: None,
        timeout_s: None,
        packs_in: Vec::new(),
        packs_out: leaves,
    };
    let wire = to_frame(&assignment).expect("encodes");
    assert_eq!(from_frame::<FromHub>(&wire).expect("decodes"), assignment);
}

// ---------------------------------------------------------------------------
// The one field E-37's detached end adds, and why it moved no version
// ---------------------------------------------------------------------------

/// `announce_end` is **absent** from a submission that does not ask, and a
/// submission that never carried it decodes with it `false`.
///
/// Both halves, for the reason the pack fields' pair is both halves: absent
/// when silent is what keeps every byte-pinned frame in this file true and is
/// the whole of the claim that [`PROTOCOL_VERSION`] did not have to move for
/// it; decoding when absent is what lets a `herdctl` built before the field
/// go on submitting to a hub built after it.
///
/// The precedent is `FromHub::Welcome`'s `keeper_for`, asserted the same way
/// a few tests above. This one is a `bool` rather than a list, which is the
/// case worth asserting separately: `false` is the one value a careless
/// `#[serde]` would have written out as `"announce_end":false` on every
/// submission in the fleet, and that would have changed the bytes of a frame
/// two pinned tests compare literally.
#[test]
fn an_end_nobody_asked_about_is_absent_from_the_wire_and_decodes_as_not_asked() {
    let bare = Asked {
        command: vec!["true".to_owned()],
        labels: LabelSet::default(),
        priority: Priority::Step,
        build_thread_mib: None,
        memory_mib: None,
        tmpfs_mib: None,
        deadline_unix_s: None,
        after: None,
        requeue_exit_code: None,
        declaration: None,
        labels_from: None,
        timeout_s: None,
        needs: Vec::new(),
        leaves: BTreeMap::new(),
    };
    let silent = to_frame(&ToHub::SubmitJob {
        asked: bare.clone(),
        wants_files: false,
        announce_end: false,
    })
    .expect("encodes");
    assert!(
        !silent.contains("announce_end"),
        "a submitter who asked for no announcement says nothing, which is E-37's \
         \"never per step job by default\" on the wire itself: {silent}"
    );

    // And the asking is said out loud when it is given: an operator reading a
    // frame off a link sees which answer this submitter gave rather than
    // having to know what an absent field means.
    let asking = to_frame(&ToHub::SubmitJob {
        asked: bare,
        wants_files: false,
        announce_end: true,
    })
    .expect("encodes");
    assert!(
        asking.contains(r#""announce_end":true"#),
        "the asking is on the wire when it is made: {asking}"
    );

    // The other direction: a submission built by a peer from before this
    // field existed. It decodes, and it decodes as a submitter who asked for
    // nothing — which is what such a peer meant, since there was no way for
    // it to ask.
    let before = r#"{"type":"submit_job","asked":{"command":["true"],"priority":"step"}}"#;
    let ToHub::SubmitJob { announce_end, .. } =
        from_frame(before).expect("an older submission decodes")
    else {
        panic!("that is a submission");
    };
    assert!(
        !announce_end,
        "no field is no asking, never an asking nobody made"
    );
}
