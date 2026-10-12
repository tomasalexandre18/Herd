//! Every message must survive a trip through JSON unchanged. The protocol is
//! the contract: a field that silently fails to round-trip is a lost field.

use herd_proto::{from_frame, to_frame, FromHub, JobId, LogStream, Role, ToHub, PROTOCOL_VERSION};

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
    round_trip_to_hub(ToHub::JobStarted {
        job_id: JobId::new("j1"),
    });
    for stream in [LogStream::Stdout, LogStream::Stderr] {
        round_trip_to_hub(ToHub::LogChunk {
            job_id: JobId::new("j1"),
            stream,
            seq: 0,
            data: "a line\n".to_owned(),
        });
    }
    round_trip_to_hub(ToHub::LogChunk {
        job_id: JobId::new("j1"),
        stream: LogStream::Stdout,
        seq: u64::MAX,
        data: "quotes \" braces {} newline \n tab \t accents eau emoji sheep".to_owned(),
    });
    for exit_code in [0, 1, 75, -1, i32::MIN, i32::MAX] {
        round_trip_to_hub(ToHub::JobFinished {
            job_id: JobId::new("j1"),
            exit_code,
        });
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
    round_trip_from_hub(FromHub::AssignJob {
        job_id: JobId::new("j1"),
        command: vec!["true".to_owned()],
    });
    for stream in [LogStream::Stdout, LogStream::Stderr] {
        round_trip_from_hub(FromHub::LogChunk {
            job_id: JobId::new("j1"),
            stream,
            seq: 7,
            data: String::new(),
        });
    }
    for exit_code in [0, 1, 75, -1] {
        round_trip_from_hub(FromHub::JobFinished {
            job_id: JobId::new("j1"),
            exit_code,
        });
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

#[test]
fn an_unknown_message_type_is_a_decode_error_not_a_panic() {
    let err = from_frame::<ToHub>(r#"{"type":"deploy_to_production"}"#)
        .expect_err("unknown types must be rejected");
    assert!(err.to_string().contains("deploy_to_production"), "{err}");
}
