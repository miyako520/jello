use jello::{format, parse, FormatOptions, MAX_NESTING_DEPTH};

#[test]
fn accepts_representative_rfc_8259_documents() {
    for source in [
        "null",
        " true ",
        "\r\n\t[0,-0,1.25,1e10,1E-2]\r\n",
        r#"{"escaped":"\"\\\/\b\f\n\r\t","unicode":"\u20AC"}"#,
        r#"{"emoji":"\uD83D\uDE00","duplicate":1,"duplicate":2}"#,
    ] {
        assert!(parse(source).is_ok(), "expected valid JSON: {source:?}");
    }
}

#[test]
fn rejects_representative_non_json_documents() {
    for source in [
        "\u{00a0}{}",
        "\"raw\ttab\"",
        r#""\uD83D""#,
        r#""\uDE00""#,
        "01",
        "+1",
        ".5",
        "1.",
        "[1,]",
        r#"{"a":1} trailing"#,
    ] {
        assert!(parse(source).is_err(), "expected invalid JSON: {source:?}");
    }
}

#[test]
fn enforces_the_documented_nesting_limit() {
    let valid = format!(
        "{}0{}",
        "[".repeat(MAX_NESTING_DEPTH),
        "]".repeat(MAX_NESTING_DEPTH)
    );
    let invalid = format!(
        "{}0{}",
        "[".repeat(MAX_NESTING_DEPTH + 1),
        "]".repeat(MAX_NESTING_DEPTH + 1)
    );

    assert!(parse(&valid).is_ok());
    assert!(parse(&invalid).is_err());
}

#[test]
fn formatted_output_round_trips_to_an_equivalent_document() {
    let source = r#"{"message":"😀","items":[1,true,null,{"nested":"value"}]}"#;
    let document = parse(source).unwrap();

    for options in [
        FormatOptions::default(),
        FormatOptions::pretty(4).unwrap(),
        FormatOptions::compact(),
    ] {
        let output = format(&document, options).unwrap();
        assert_eq!(parse(&output).unwrap(), document);
    }
}
