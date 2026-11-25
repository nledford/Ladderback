use super::*;

/// The names decision 120 gives, and the two it leaves to this module.
#[test]
fn each_key_file_has_its_name() {
    let names = KeyFile::ALL.map(KeyFile::file_name);
    assert_eq!(
        names,
        [
            "machine-key",
            "hub-key",
            "hub-tls-key",
            "hub-recovery-key",
            "hub-recovery-seed",
            "connection-counter",
            "hub-pin",
        ]
    );
    assert_eq!(KeyFile::HubPin.to_string(), "hub-pin");
}

#[test]
fn each_seed_file_is_its_key_file() {
    assert_eq!(
        SeedFile::ALL.map(SeedFile::key_file),
        [KeyFile::AgentKey, KeyFile::HubKey, KeyFile::RecoverySeed]
    );
}

#[test]
fn a_backup_leaves_out_the_secret_key_files_and_what_is_staged_beside_them() {
    assert_eq!(
        left_out_globs("/home/demo/.local/share/ladderback"),
        [
            "!/home/demo/.local/share/ladderback/hub-key*",
            "!/home/demo/.local/share/ladderback/hub-tls-key*",
            "!/home/demo/.local/share/ladderback/hub-recovery-seed*",
            "!/home/demo/.local/share/ladderback/machine-key*",
        ]
    );
}

/// A folder named with a slash at the end, or the root itself, gets one
/// slash before the name, and its pattern characters are escaped.
#[test]
fn the_folder_is_read_as_written() {
    assert_eq!(
        left_out_globs("/srv/demo/")[0],
        "!/srv/demo/hub-key*",
        "one slash"
    );
    assert_eq!(left_out_globs("/")[3], "!/machine-key*");
    assert_eq!(
        left_out_globs("/srv/[demo]*?{a,b}\\")[1],
        "!/srv/\\[demo\\]\\*\\?\\{a,b\\}\\\\/hub-tls-key*"
    );
}

/// A name the globs leave out: a secret key file's, what is staged beside
/// it, in any case, and whatever bytes follow; never the public files, nor
/// a name that only holds a secret one's further in.
#[test]
fn the_names_the_globs_leave_out() {
    let left_out: [&[u8]; 7] = [
        b"hub-key",
        b"hub-key.0123.tmp",
        b"HUB-TLS-KEY",
        b"hub-recovery-seed.new",
        b"Machine-Key",
        b"machine-key\xff",
        b"hub-tls-key",
    ];
    for name in left_out {
        assert!(is_left_out(name), "{}", name.escape_ascii());
    }
    let kept: [&[u8]; 9] = [
        b"",
        b"hub-ke",
        b"hub-recovery-key",
        b"hub-pin",
        b"connection-counter",
        b"ladderback.db",
        b"old-hub-key",
        b"\xffhub-key",
        b"machine-id",
    ];
    for name in kept {
        assert!(!is_left_out(name), "{}", name.escape_ascii());
    }
}

/// The machine's secret key is `machine-key` (§16, decision 163), and a
/// backup leaves it out by that name.
#[test]
fn the_machines_key_file_is_machine_key() {
    assert_eq!(KeyFile::AgentKey.file_name(), "machine-key");
    assert!(is_left_out(b"machine-key"));
    assert!(is_left_out(b"machine-key.0123.tmp"));
}

#[test]
fn one_line_takes_off_one_line_break() {
    assert_eq!(one_line("abc\n"), Some("abc"));
    assert_eq!(one_line("abc"), Some("abc"));
    assert_eq!(one_line(""), Some(""));
    for text in ["abc\n\n", "a\nbc", "abc\r\n", "a\rbc"] {
        assert_eq!(one_line(text), None, "{text:?}");
    }
}

#[test]
fn a_counter_rises_by_one_and_stops_at_the_most() {
    assert_eq!(ConnectionCounter::NONE.get(), 0);
    assert_eq!(ConnectionCounter::new(907).get(), 907);
    assert_eq!(ConnectionCounter::default(), ConnectionCounter::NONE);
    assert_eq!(
        ConnectionCounter::NONE.next(),
        Some(ConnectionCounter::new(1))
    );
    assert_eq!(
        ConnectionCounter::new(41).next(),
        Some(ConnectionCounter::new(42))
    );
    assert_eq!(ConnectionCounter::new(u64::MAX).next(), None);
}

#[test]
fn a_counter_reads_back_from_its_file() {
    for value in [0, 1, 907, u64::MAX] {
        let counter = ConnectionCounter::new(value);
        let text = counter.to_file_text();
        assert_eq!(text, format!("{value}\n"));
        assert_eq!(ConnectionCounter::parse(&text), Ok(counter));
    }
    assert_eq!(
        ConnectionCounter::parse("12"),
        Ok(ConnectionCounter::new(12)),
        "without a line break"
    );
}

#[test]
fn other_text_is_no_counter() {
    for text in [
        "",
        "\n",
        "-1\n",
        "+1\n",
        " 1\n",
        "1 \n",
        "1\n2\n",
        "0x10\n",
        "18446744073709551616\n",
        "000000000000000000001\n",
        "٣\n",
    ] {
        assert_eq!(
            ConnectionCounter::parse(text),
            Err(CounterRefused),
            "{text:?}"
        );
    }
    assert_eq!(
        ConnectionCounter::parse("00000000000000000001\n"),
        Ok(ConnectionCounter::new(1)),
        "20 digits"
    );
}

#[test]
fn a_tls_key_holds_one_to_the_most_bytes() {
    let key = TlsKey::from_der(vec![0x30; MAX_TLS_KEY_BYTES]).unwrap();
    assert_eq!(key.der().len(), MAX_TLS_KEY_BYTES);
    assert_eq!(TlsKey::from_der(vec![0x30]).unwrap().der(), [0x30]);
    assert_eq!(TlsKey::from_der(Vec::new()).unwrap_err(), TlsKeyRefused);
    assert_eq!(
        TlsKey::from_der(vec![0x30; MAX_TLS_KEY_BYTES + 1]).unwrap_err(),
        TlsKeyRefused
    );
}

/// I4: the key's bytes never reach a log through `Debug`.
#[test]
fn a_tls_key_never_shows_its_bytes() {
    let key = TlsKey::from_der(b"canary-tls-key".to_vec()).unwrap();
    assert_eq!(format!("{key:?}"), "TlsKey(***)");
}
