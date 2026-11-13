use seiya_observe::macos::*;

const VM: &str = include_str!("fixtures/vm_stat.txt");

#[test]
fn rss_accepts_one_expected_pid_with_kib_not_identity_or_bytes() {
    assert_eq!(parse_rss(42, include_str!("fixtures/rss.txt")), Ok(1234));
    assert_eq!(parse_rss(42, "42 0"), Ok(0));
    assert_eq!(parse_rss(43, "42 1"), Err(ParseError::PidMismatch));
    assert_eq!(parse_rss(0, "0 1"), Err(ParseError::InvalidData));
}

#[test]
fn rss_rejects_ambiguity_empty_headers_units_and_overflow() {
    for text in [
        "",
        "PID RSS\n42 12",
        "42 1\n43 2",
        "42 -1",
        "42 1.5",
        "42 1M",
        "42 1 extra",
        "+42 1",
        "42 18446744073709551615",
        "42 18446744073709551616",
    ] {
        assert!(parse_rss(42, text).is_err(), "{text}");
    }
}

#[test]
fn vm_stat_preserves_explicit_page_size_and_selected_page_counts() {
    let parsed = parse_vm_stat(VM).unwrap();
    assert_eq!(parsed.page_size_bytes, 16384);
    assert_eq!(parsed.pages.len(), 6);
    assert_eq!(parsed.pages[&PageCounter::Free], 10);
    assert_eq!(parsed.pages[&PageCounter::Inactive], 0);
    let small = parse_vm_stat(&VM.replace("16384", "4096")).unwrap();
    assert_eq!(small.pages, parsed.pages);
    assert_eq!(small.page_size_bytes, 4096);
}

#[test]
fn missing_vm_counters_are_absent_and_bad_counters_or_duplicates_fail() {
    let header = "Mach Virtual Memory Statistics: (page size of 4096 bytes)\n";
    let partial = parse_vm_stat(&format!("{header}Pages free: 7.\n")).unwrap();
    assert!(!partial.pages.contains_key(&PageCounter::Active));
    for rest in [
        "",
        "Pages free: 1.\nPages free: 2.",
        "Pages free: -1.",
        "Pages free: 1,234.",
        "Pages free: 1.5.",
        "Pages free: 18446744073709551615.",
        "Pages free: 1.\nunknown-private: secret",
    ] {
        assert!(parse_vm_stat(&format!("{header}{rest}")).is_err());
    }
    for size in ["0", "1000", "65537", "18446744073709551616"] {
        assert!(parse_vm_stat(&VM.replace("16384", size)).is_err());
    }
}

#[test]
fn swap_binary_units_and_two_decimal_resolution_are_explicit() {
    let swap = parse_swap(include_str!("fixtures/swap.txt")).unwrap();
    assert_eq!(swap.total_bytes, 1024 * 1024 * 1024);
    assert_eq!(swap.used_bytes, 268697600);
    assert_eq!(swap.free_bytes, swap.total_bytes - swap.used_bytes);
    for (unit, factor) in [
        ("B", 1),
        ("K", 1024),
        ("M", 1024_u64.pow(2)),
        ("G", 1024_u64.pow(3)),
        ("T", 1024_u64.pow(4)),
    ] {
        let parsed = parse_swap(&format!(
            "vm.swapusage: total = 2{unit} used = 1{unit} free = 1{unit}"
        ))
        .unwrap();
        assert_eq!(parsed.used_bytes, factor);
        assert_eq!(parsed.total_bytes, 2 * factor);
    }
    let rounded = parse_swap("vm.swapusage: total = 1.00M used = 0.01M free = 0.99M").unwrap();
    assert_eq!(rounded.used_bytes, 10485); // floor, not precision invented beyond text.
}

#[test]
fn swap_rejects_unknown_units_locales_ambiguity_and_inconsistent_totals() {
    for text in [
        "",
        "vm.swapusage: total = 0M used = 1M free = 0M",
        "vm.swapusage: total = 5M used = 1M free = 1M",
        "vm.swapusage: total = 2MB used = 1MB free = 1MB",
        "vm.swapusage: total = 2,00M used = 1M free = 1M",
        "vm.swapusage: total = 2M used = 1M free = 1M extra",
        "vm.swapusage: total = 2M used = 1M free = 1M\nextra",
        "vm.swapusage: total = 2.000M used = 1M free = 1M",
        "vm.swapusage: total = 2.M used = 1M free = 1M",
        "vm.swapusage: total = 18446744073709551615T used = 0T free = 0T",
    ] {
        assert!(parse_swap(text).is_err(), "{text}");
    }
}

#[test]
fn bounded_inputs_and_non_ascii_cannot_be_parsed_as_host_data() {
    let large = "x".repeat(MAX_BYTES + 1);
    let many_lines = "\n".repeat(MAX_LINES + 1);
    for text in [&large, &many_lines] {
        assert_eq!(parse_rss(42, text), Err(ParseError::Limit));
        assert_eq!(parse_vm_stat(text), Err(ParseError::Limit));
        assert_eq!(parse_swap(text), Err(ParseError::Limit));
    }
    for text in ["é", "42 ０", "42 1\0", "42 1\r"] {
        assert!(parse_rss(42, text).is_err());
        assert!(parse_vm_stat(text).is_err());
        assert!(parse_swap(text).is_err());
    }
}

#[test]
fn arbitrary_synthetic_short_ascii_inputs_return_errors_without_panics() {
    // Deterministic malformed corpus, not a claim of complete fuzz coverage.
    for length in 0..128 {
        let text: String = (0..length)
            .map(|i| char::from(32 + ((i * 17 + length) % 95) as u8))
            .collect();
        let _ = parse_rss(42, &text);
        let _ = parse_vm_stat(&text);
        let _ = parse_swap(&text);
    }
}
