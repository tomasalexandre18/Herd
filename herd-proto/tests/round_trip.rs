//! Every message must survive a trip through JSON unchanged. The protocol is
//! the contract: a field that silently fails to round-trip is a lost field.

use herd_proto::{
    cap_exceeded_reason, cap_stop_reason, from_frame, to_frame, Cap, FromHub, JobId, JobMetrics,
    JobOutcome, LogStream, Role, RunningJob, ToHub, PROTOCOL_VERSION,
};

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
        },
        JobMetrics {
            host_load_milli: Some(0),
            peak_memory_bytes: Some(0),
            bytes_written: Some(0),
            log_bytes: u64::MAX,
            wall_time_ms: u64::MAX,
        },
    ]
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
            role,
            token: Some("a-token".to_owned()),
            name: Some("wsl-pc".to_owned()),
            instance: Some("run-4242-17a3f9".to_owned()),
            state_root: Some("root-0123456789abcdef".to_owned()),
            health_every_ms: Some(15_000),
        });
        // The optional fields really are optional.
        round_trip_to_hub(ToHub::Hello {
            protocol_version: PROTOCOL_VERSION,
            role,
            token: None,
            name: None,
            instance: None,
            state_root: None,
            health_every_ms: None,
        });
    }
    round_trip_to_hub(ToHub::SubmitJob {
        command: vec!["sh".to_owned(), "-c".to_owned(), "echo hi".to_owned()],
    });
    round_trip_to_hub(ToHub::SubmitJob { command: vec![] });
    round_trip_to_hub(ToHub::WaitForJob {
        job_id: JobId::new("j0000000001"),
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
    });
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
        },
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

#[test]
fn an_unknown_message_type_is_a_decode_error_not_a_panic() {
    let err = from_frame::<ToHub>(r#"{"type":"deploy_to_production"}"#)
        .expect_err("unknown types must be rejected");
    assert!(err.to_string().contains("deploy_to_production"), "{err}");
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
            job_id: JobId::new("j0000000042")
        })
        .expect("encodes"),
        r#"{"type":"wait_for_job","job_id":"j0000000042"}"#
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
        })
        .expect("encodes"),
        r#"{"type":"health","jobs":[{"job_id":"j0000000007","attempt":2,"chunks_sent":412}],"draining":false}"#
    );
    assert_eq!(
        to_frame(&ToHub::Health {
            jobs: vec![],
            draining: true,
        })
        .expect("encodes"),
        r#"{"type":"health","jobs":[],"draining":true}"#
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
    for draining in [false, true] {
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
        });
    }
    round_trip_to_hub(ToHub::CancelJob {
        job_id: JobId::new("j1"),
    });
    round_trip_to_hub(ToHub::AttemptAbandoned {
        job_id: JobId::new("j1"),
        attempt: 1,
        reason: String::new(),
    });
    round_trip_from_hub(FromHub::HealthRecorded {});
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

    // And a cadence of nothing is not a cadence to judge anything against: it
    // is refused where it is configured, not reinterpreted here.
    assert!(
        herd_proto::silence_threshold_refusal("t", Duration::ZERO, "i", Duration::ZERO).is_none()
    );
}
