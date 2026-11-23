use super::*;

fn seq(number: u64) -> ReportSeq {
    ReportSeq::new(number).unwrap()
}

fn report(happening: Happening) -> AgentReport {
    AgentReport {
        op_id: OperationId::from_uuid(Uuid::from_u128(1)),
        happening,
        at: EpochSeconds::from_seconds(1_800_000_000),
    }
}

fn start() -> AgentReport {
    report(Happening::Started)
}

fn failure() -> AgentReport {
    report(Happening::Failed {
        code: WireName::parse("engine-failed").unwrap(),
    })
}

const EPOCH: OutboxEpoch = OutboxEpoch::from_uuid(Uuid::from_u128(7));

#[test]
fn places_count_up_from_one_and_stop_at_the_last_number() {
    assert_eq!(ReportSeq::FIRST.get(), 1);
    assert_eq!(ReportSeq::new(0), None);
    assert_eq!(ReportSeq::FIRST.next(), Some(seq(2)));
    assert_eq!(seq(41).next(), Some(seq(42)));
    assert_eq!(seq(u64::MAX).next(), None);
}

#[test]
fn only_an_end_is_kept_however_full_the_outbox_is() {
    assert!(!start().kept());
    assert!(report(Happening::Succeeded).kept());
    assert!(failure().kept());
}

#[test]
fn a_report_is_appended_while_the_outbox_has_room() {
    let below = MAX_OUTBOX_REPORTS - 1;
    assert_eq!(
        admit(below, Some(seq(1)), Some(seq(9))),
        OutboxAdmission::Append
    );
    assert_eq!(admit(0, None, Some(seq(1))), OutboxAdmission::Append);
}

#[test]
fn a_full_outbox_gives_up_its_oldest_start_for_any_report() {
    assert_eq!(
        admit(MAX_OUTBOX_REPORTS, Some(seq(3)), Some(seq(9))),
        OutboxAdmission::EvictThenAppend(seq(3))
    );
}

#[test]
fn an_outbox_full_of_ends_refuses_rather_than_give_one_up() {
    assert_eq!(
        admit(MAX_OUTBOX_REPORTS, None, Some(seq(9))),
        OutboxAdmission::Refuse
    );
}

#[test]
fn an_outbox_out_of_numbers_refuses_every_report() {
    assert_eq!(admit(0, None, None), OutboxAdmission::Refuse);
    assert_eq!(
        admit(MAX_OUTBOX_REPORTS, Some(seq(1)), None),
        OutboxAdmission::Refuse
    );
}

#[test]
fn an_ack_of_a_report_sent_on_the_connection_is_taken() {
    let sent = Some(seq(5));
    for through in [1, 4, 5] {
        let ack = Ack {
            epoch: EPOCH,
            seq: seq(through),
        };
        assert_eq!(check_ack(EPOCH, sent, ack), Ok(()));
    }
}

#[test]
fn an_ack_past_what_was_sent_or_of_another_epoch_is_refused() {
    let ahead = Ack {
        epoch: EPOCH,
        seq: seq(6),
    };
    assert_eq!(
        check_ack(EPOCH, Some(seq(5)), ahead),
        Err(AckRefused::NotSent)
    );
    let first = Ack {
        epoch: EPOCH,
        seq: seq(1),
    };
    assert_eq!(check_ack(EPOCH, None, first), Err(AckRefused::NotSent));
    let other = Ack {
        epoch: OutboxEpoch::from_uuid(Uuid::from_u128(8)),
        seq: seq(1),
    };
    assert_eq!(
        check_ack(EPOCH, Some(seq(5)), other),
        Err(AckRefused::OtherEpoch)
    );
}

#[test]
fn the_hub_takes_a_report_past_the_last_it_took_once() {
    assert_eq!(ingest(None, seq(1)), Ingest::New);
    assert_eq!(ingest(None, seq(9)), Ingest::New);
    assert_eq!(ingest(Some(seq(3)), seq(4)), Ingest::New);
    assert_eq!(ingest(Some(seq(3)), seq(7)), Ingest::New);
    assert_eq!(ingest(Some(seq(3)), seq(3)), Ingest::Duplicate);
    assert_eq!(ingest(Some(seq(3)), seq(1)), Ingest::Duplicate);
}

/// A report sent again lies less than a send window behind the last the
/// hub took: the agent sends at most that many before their
/// acknowledgements. One further behind comes from an agent numbering its
/// reports again from an older place (#1387).
#[test]
fn a_report_a_send_window_behind_the_last_taken_is_behind() {
    let window = u64::try_from(SEND_WINDOW).unwrap();
    let last = seq(window + 1);
    assert_eq!(ingest(Some(last), seq(2)), Ingest::Duplicate);
    assert_eq!(ingest(Some(last), seq(1)), Ingest::Behind);
}

#[test]
fn a_hubs_message_holds_until_its_expiry_on_the_hubs_time() {
    let signed_at = EpochSeconds::from_seconds(1_000);
    let at = EpochSeconds::from_seconds;
    let after = SignedDuration::from_millis;
    assert!(fresh_on_hub_time(signed_at, after(0), at(1_600)));
    assert!(fresh_on_hub_time(signed_at, after(599_999), at(1_600)));
    assert!(!fresh_on_hub_time(signed_at, after(600_000), at(1_600)));
    assert!(!fresh_on_hub_time(signed_at, after(0), at(1_000)));
    assert!(fresh_on_hub_time(signed_at, after(0), at(1_900)));
    assert!(
        !fresh_on_hub_time(signed_at, after(0), at(1_901)),
        "further ahead than a hub signs"
    );
    assert!(fresh_on_hub_time(signed_at, after(10_000), at(1_910)));
    assert!(
        fresh_on_hub_time(signed_at, after(-5_000), at(1_900)),
        "a monotonic clock read backwards counts as no time"
    );
}
