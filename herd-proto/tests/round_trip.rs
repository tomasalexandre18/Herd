//! Every message must survive a trip through JSON unchanged. The protocol is
//! the contract: a field that silently fails to round-trip is a lost field.

use herd_proto::{
    cap_exceeded_reason, from_frame, to_frame, Cap, FromHub, JobId, JobMetrics, JobOutcome,
    LogStream, Role, ToHub, PROTOCOL_VERSION,
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
    outcomes.extend(
        EVERY_CAP
            .into_iter()
            .map(|cap| JobOutcome::StoppedByCap { cap, limit: 0 }),
    );
    outcomes.extend(EVERY_CAP.into_iter().map(|cap| JobOutcome::StoppedByCap {
        cap,
        limit: u64::MAX,
    }));
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
        });
        // The optional fields really are optional.
        round_trip_to_hub(ToHub::Hello {
            protocol_version: PROTOCOL_VERSION,
            role,
            token: None,
            name: None,
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
    });
    for stream in [LogStream::Stdout, LogStream::Stderr] {
        for data in every_payload() {
            round_trip_to_hub(ToHub::LogChunk {
                job_id: JobId::new("j1"),
                stream,
                seq: 0,
                data,
            });
        }
    }
    round_trip_to_hub(ToHub::LogChunk {
        job_id: JobId::new("j1"),
        stream: LogStream::Stdout,
        seq: u64::MAX,
        data: b"quotes \" braces {} newline \n tab \t".to_vec(),
    });
    for outcome in every_outcome() {
        for metrics in every_metrics() {
            round_trip_to_hub(ToHub::JobFinished {
                job_id: JobId::new("j1"),
                outcome: outcome.clone(),
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
    })
    .expect("encodes");
    assert_eq!(frame, r#"{"type":"job_started","job_id":"j42"}"#);
}

/// The outcome's wire shape, pinned variant by variant. A job's end is a
/// tagged object nested in the frame, so M3, M8 and M9 can add a variant
/// without touching any of these three.
#[test]
fn each_outcome_is_a_tagged_object_inside_the_frame() {
    let frame = |outcome| {
        to_frame(&ToHub::JobFinished {
            job_id: JobId::new("j1"),
            outcome,
            metrics: JobMetrics::default(),
        })
        .expect("encodes")
    };
    // No measurement was taken, so every optional figure is absent from the
    // frame rather than present as a zero.
    let empty = r#","metrics":{"log_bytes":0,"wall_time_ms":0}}"#;
    assert_eq!(
        frame(JobOutcome::Exited { code: 42 }),
        format!(
            r#"{{"type":"job_finished","job_id":"j1","outcome":{{"type":"exited","code":42}}{empty}"#
        )
    );
    assert_eq!(
        frame(JobOutcome::Signalled { signal: 15 }),
        format!(
            r#"{{"type":"job_finished","job_id":"j1","outcome":{{"type":"signalled","signal":15}}{empty}"#
        )
    );
    assert_eq!(
        frame(JobOutcome::CouldNotStart {
            reason: "no-such-tool: not found".to_owned()
        }),
        format!(
            r#"{{"type":"job_finished","job_id":"j1","outcome":{{"type":"could_not_start","reason":"no-such-tool: not found"}}{empty}"#
        )
    );
    assert_eq!(
        frame(JobOutcome::StoppedByCap {
            cap: Cap::Memory,
            limit: 2_147_483_648
        }),
        format!(
            r#"{{"type":"job_finished","job_id":"j1","outcome":{{"type":"stopped_by_cap","cap":"memory","limit":2147483648}}{empty}"#
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
        outcome: JobOutcome::Exited { code: 0 },
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
    }
}

/// A peer from a later milestone reporting an end this one does not know is a
/// decode error naming it, never a job silently read as having exited.
#[test]
fn an_unknown_outcome_is_a_decode_error_not_a_panic() {
    let err = from_frame::<ToHub>(
        r#"{"type":"job_finished","job_id":"j1","outcome":{"type":"lost","since":"m3"}}"#,
    )
    .expect_err("an unknown way to end must be rejected");
    assert!(err.to_string().contains("lost"), "{err}");
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
        stream: LogStream::Stdout,
        seq: 3,
        data: written.clone(),
    })
    .expect("encodes");
    assert_eq!(
        frame,
        r#"{"type":"log_chunk","job_id":"j1","stream":"stdout","seq":3,"data":"YQBi/2MK"}"#
    );
    let ToHub::LogChunk { data, .. } = from_frame(&frame).expect("decodes") else {
        panic!("a log chunk decodes as a log chunk");
    };
    assert_eq!(data, written, "the bytes the process wrote were rewritten");

    // And a payload that is not base64 at all is a decode error, not an empty
    // log: a chunk Herd cannot read is never read as nothing.
    let error = from_frame::<ToHub>(
        r#"{"type":"log_chunk","job_id":"j1","stream":"stdout","seq":0,"data":"not base64!"}"#,
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
