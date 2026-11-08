use structureweaver::{
    domain::{Cell, CellKind, CellStyle},
    number_format::{builtin_num_format, color_override, display_value, format_number},
};

fn number(value: f64, format: &str) -> String {
    format_number(value, format, false).expect("supported number format")
}

#[test]
fn grouping_precision_and_optional_digits_follow_the_format() {
    assert_eq!(number(12345.678, "#,##0.00"), "12,345.68");
    assert_eq!(number(1.5, "0.000"), "1.500");
    assert_eq!(number(1.5, "0.###"), "1.5");
    assert_eq!(number(1.0, "0.###"), "1");
    assert_eq!(number(0.0, "#"), "");
    assert_eq!(number(1234.0, "000000"), "001234");
    assert_eq!(number(12.3, "0.0?"), "12.3 ");
    assert_eq!(number(0.25, ".00"), ".25");
    assert_eq!(number(123456789.0, "000-00-0000"), "123-45-6789");
}

#[test]
fn percentages_currency_and_scaling_do_not_change_raw_values() {
    assert_eq!(number(0.125, "0.0%"), "12.5%");
    assert_eq!(number(123456789.0, "#,##0,,.0\"M\""), "123.5M");
    assert_eq!(number(12500.0, "#,##0,"), "13");
    assert_eq!(number(12500.0, "[$₩-412]#,##0"), "₩12,500");
    assert_eq!(number(12.34, "\"USD \"0.00"), "USD 12.34");
    assert_eq!(number(1.0, "0\\%"), "1%");
    let cell = Cell {
        value: "0.125".into(),
        kind: CellKind::Number,
        raw_number: Some(0.125),
        style: CellStyle {
            number_format: Some("0.0%".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(display_value(&cell, false), "12.5%");
    assert_eq!(cell.value, "0.125");
}

#[test]
fn sections_conditions_and_literals_are_not_confused() {
    let format = "#,##0.00;[Red](#,##0.00);\"—\";\"이름: \"@";
    assert_eq!(number(-1234.0, format), "(1,234.00)");
    assert_eq!(number(0.0, format), "—");
    assert_eq!(number(-2.0, "0.0"), "-2.0");
    assert_eq!(
        number(40.0, "[>=100]0\" large\";[<100]0\" small\""),
        "40 small"
    );
    assert_eq!(
        number(120.0, "[>=100]0\" large\";[<100]0\" small\""),
        "120 large"
    );
    assert_eq!(number(4.0, "0\"; units\""), "4; units");
    assert_eq!(number(3.0, "0\" days\""), "3 days");
    let cell = Cell {
        value: "서울".into(),
        kind: CellKind::Text,
        style: CellStyle {
            number_format: Some(format.into()),
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(display_value(&cell, false), "이름: 서울");
}

#[test]
fn scientific_and_fraction_formats_keep_their_structure() {
    assert_eq!(number(12345.0, "0.00E+00"), "1.23E+04");
    assert_eq!(number(0.00125, "0.00E+00"), "1.25E-03");
    assert_eq!(number(12345.0, "##0.0E+0"), "12.3E+3");
    assert_eq!(number(12345.0, "00.0E+0"), "12.3E+3");
    assert_eq!(number(10000.0, "0.0#E+00"), "1.0E+04");
    assert_eq!(number(0.0, "0.00E+00"), "0.00E+00");
    assert_eq!(number(1.5, "# ?/?"), "1 1/2");
    assert_eq!(number(0.25, "# ??/??"), "1/4");
    assert_eq!(number(0.26, "# ?/8"), "2/8");
    assert_eq!(number(2.0, "# ?/?"), "2");
}

#[test]
fn dates_honor_excel_epoch_and_leap_day_compatibility() {
    assert_eq!(number(1.0, "yyyy-mm-dd"), "1900-01-01");
    assert_eq!(number(59.0, "yyyy-mm-dd"), "1900-02-28");
    assert_eq!(number(60.0, "yyyy-mm-dd"), "1900-02-29");
    assert_eq!(number(61.0, "yyyy-mm-dd"), "1900-03-01");
    assert_eq!(number(45292.0, "yyyy-mm-dd"), "2024-01-01");
    assert_eq!(
        format_number(43830.0, "yyyy-mm-dd", true).unwrap(),
        "2024-01-01"
    );
    assert_eq!(
        format_number(0.0, "yyyy-mm-dd", true).unwrap(),
        "1904-01-01"
    );
    assert_eq!(
        number(45292.0, "dddd, mmmm d, yyyy"),
        "Monday, January 1, 2024"
    );
    assert_eq!(
        number(45292.0, "yyyy\"년\" m\"월\" d\"일\""),
        "2024년 1월 1일"
    );
}

#[test]
fn minutes_months_and_elapsed_hours_are_distinct() {
    assert_eq!(
        number(45292.5, "yyyy-mm-dd h:mm AM/PM"),
        "2024-01-01 12:00 PM"
    );
    assert_eq!(number(0.25, "h:mm:ss AM/PM"), "6:00:00 AM");
    assert_eq!(number(1.5, "[h]:mm:ss"), "36:00:00");
    assert_eq!(number(1.5, "[mm]:ss"), "2160:00");
    assert_eq!(number(0.5, "mm:ss"), "00:00");
    assert_eq!(number(0.000017361111111, "ss.000"), "01.500");
}

#[test]
fn older_saved_analysis_dates_and_cached_displays_still_work() {
    let mut cell = Cell {
        value: "2024-01-01 13:05:30".into(),
        kind: CellKind::Date,
        style: CellStyle {
            number_format: Some("yyyy-mm-dd hh:mm:ss".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(display_value(&cell, false), "2024-01-01 13:05:30");
    cell.display_value = Some("원본 캐시".into());
    assert_eq!(display_value(&cell, false), "원본 캐시");
    cell.display_value = None;
    cell.value = "unparsed-date".into();
    assert_eq!(display_value(&cell, false), "unparsed-date");
}

#[test]
fn color_sections_and_builtin_formats_are_available_to_the_renderer() {
    let mut cell = Cell {
        value: "-1".into(),
        raw_number: Some(-1.0),
        kind: CellKind::Number,
        style: CellStyle {
            number_format: Some("0;[Red](0)".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(color_override(&cell).as_deref(), Some("FFFF0000"));
    cell.raw_number = Some(1.0);
    assert_eq!(color_override(&cell), None);
    assert_eq!(builtin_num_format(14).as_deref(), Some("mm-dd-yy"));
    assert!(builtin_num_format(999).is_none());
    assert_eq!(number(0.0, &builtin_num_format(44).unwrap()).trim(), "$ -");
}
