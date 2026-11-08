//! Presentation-only Excel number formats. Raw values remain available for analysis.
//!
//! This implements common OOXML number/date formats without evaluating formulas or
//! changing locale settings. Width-dependent `*` fill and `_` spacing are approximate;
//! localized month/day names use English unless literals are supplied by the file.
use crate::domain::{Cell, CellKind};

#[derive(Clone, Debug)]
enum Token {
    Literal(String),
    Symbol(char),
    Elapsed(char, usize),
    AmPm(bool),
}

/// Common, locale-independent built-in OOXML format IDs.
pub fn builtin_num_format(id: u32) -> Option<String> {
    Some(
        match id {
            0 => "General",
            1 => "0",
            2 => "0.00",
            3 => "#,##0",
            4 => "#,##0.00",
            5 => "$#,##0;($#,##0)",
            6 => "$#,##0;[Red]($#,##0)",
            7 => "$#,##0.00;($#,##0.00)",
            8 => "$#,##0.00;[Red]($#,##0.00)",
            9 => "0%",
            10 => "0.00%",
            11 => "0.00E+00",
            12 => "# ?/?",
            13 => "# ??/??",
            14 => "mm-dd-yy",
            15 => "d-mmm-yy",
            16 => "d-mmm",
            17 => "mmm-yy",
            18 => "h:mm AM/PM",
            19 => "h:mm:ss AM/PM",
            20 => "h:mm",
            21 => "h:mm:ss",
            22 => "m/d/yy h:mm",
            37 => "#,##0 ;(#,##0)",
            38 => "#,##0 ;[Red](#,##0)",
            39 => "#,##0.00;(#,##0.00)",
            40 => "#,##0.00;[Red](#,##0.00)",
            41 => "_(* #,##0_);_(* (#,##0);_(* \"-\"_);_(@_)",
            42 => "_($* #,##0_);_($* (#,##0);_($* \"-\"_);_(@_)",
            43 => "_(* #,##0.00_);_(* (#,##0.00);_(* \"-\"??_);_(@_)",
            44 => "_($* #,##0.00_);_($* (#,##0.00);_($* \"-\"??_);_(@_)",
            45 => "mm:ss",
            46 => "[h]:mm:ss",
            47 => "mmss.0",
            48 => "##0.0E+0",
            49 => "@",
            _ => return None,
        }
        .into(),
    )
}

/// Use a cached presentation value when supplied; otherwise apply the stored format.
pub fn display_value(cell: &Cell, date1904: bool) -> String {
    if let Some(display) = &cell.display_value {
        return display.clone();
    }
    if cell.kind == CellKind::Empty {
        return String::new();
    }
    if cell.kind == CellKind::Boolean {
        return cell.value.to_uppercase();
    }
    let Some(format) = cell.style.number_format.as_deref() else {
        return cell.value.clone();
    };
    if format.eq_ignore_ascii_case("general") {
        return cell.value.clone();
    }
    let number = cell
        .raw_number
        .or_else(|| {
            if matches!(cell.kind, CellKind::Number | CellKind::Date) {
                cell.value.parse().ok()
            } else {
                None
            }
        })
        .or_else(|| {
            (cell.kind == CellKind::Date)
                .then(|| iso_serial(&cell.value, date1904))
                .flatten()
        });
    if let Some(number) = number {
        format_number(number, format, date1904).unwrap_or_else(|| cell.value.clone())
    } else if cell.kind == CellKind::Text {
        let sections = split_sections(format);
        let section = if sections.len() >= 4 {
            sections[3]
        } else {
            format
        };
        let tokens = tokenize(section);
        if tokens.iter().any(|t| matches!(t, Token::Symbol('@'))) {
            tokens
                .iter()
                .map(|t| match t {
                    Token::Symbol('@') => cell.value.clone(),
                    _ => literal(t),
                })
                .collect()
        } else if sections.len() >= 4 {
            render_literals(&tokens)
        } else {
            cell.value.clone()
        }
    } else {
        cell.value.clone()
    }
}

/// A color embedded in the selected number-format section, if any (ARGB).
pub fn color_override(cell: &Cell) -> Option<String> {
    let number = cell.raw_number.or_else(|| cell.value.parse().ok())?;
    let format = cell.style.number_format.as_deref()?;
    let (section, _) = choose_section(format, number)?;
    for bracket in brackets(section) {
        let color = match bracket.to_ascii_lowercase().as_str() {
            "black" => "FF000000",
            "blue" => "FF0000FF",
            "cyan" => "FF00FFFF",
            "green" => "FF008000",
            "magenta" => "FFFF00FF",
            "red" => "FFFF0000",
            "white" => "FFFFFFFF",
            "yellow" => "FFFFFF00",
            _ => continue,
        };
        return Some(color.into());
    }
    None
}

pub fn format_number(value: f64, format: &str, date1904: bool) -> Option<String> {
    if !value.is_finite() || format.len() > 4096 {
        return None;
    }
    let (section, explicit_negative) = choose_section(format, value)?;
    if section.eq_ignore_ascii_case("general") || section == "@" {
        return Some(value.to_string());
    }
    if section.is_empty() {
        return Some(String::new());
    }
    let tokens = tokenize(section);
    if is_date(&tokens) {
        return date_format(value, &tokens, date1904);
    }
    let absolute = value.abs();
    let result = if tokens.iter().any(|t| matches!(t, Token::Symbol('/'))) {
        fraction_format(absolute, &tokens)
    } else if tokens.iter().any(|t| matches!(t, Token::Symbol('E' | 'e'))) {
        scientific_format(absolute, &tokens)
    } else {
        fixed_format(absolute, &tokens)
    }?;
    let minus = value < 0.0 && !explicit_negative && tokens.iter().any(is_placeholder);
    Some(if minus { format!("-{result}") } else { result })
}

fn split_sections(format: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let (mut quoted, mut escaped, mut bracket, mut start) = (false, false, false, 0);
    for (offset, ch) in format.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
        }
        if quoted {
            continue;
        }
        if ch == '[' {
            bracket = true;
        }
        if ch == ']' {
            bracket = false;
        }
        if ch == ';' && !bracket {
            result.push(&format[start..offset]);
            start = offset + 1;
        }
    }
    result.push(&format[start..]);
    result
}

fn brackets(section: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut quoted = false;
    let mut escaped = false;
    let mut start = None;
    for (i, ch) in section.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
        }
        if quoted {
            continue;
        }
        if ch == '[' {
            start = Some(i + 1);
        }
        if ch == ']'
            && let Some(s) = start.take()
        {
            result.push(&section[s..i]);
        }
    }
    result
}

fn condition(section: &str) -> Option<(&str, f64)> {
    for content in brackets(section) {
        for operator in ["<=", ">=", "<>", "<", ">", "="] {
            if let Some(rest) = content.strip_prefix(operator)
                && let Ok(value) = rest.parse()
            {
                return Some((operator, value));
            }
        }
    }
    None
}

fn choose_section(format: &str, value: f64) -> Option<(&str, bool)> {
    let sections = split_sections(format);
    let numeric = &sections[..sections.len().min(3)];
    if numeric.iter().any(|s| condition(s).is_some()) {
        for section in numeric {
            let matches = condition(section).is_none_or(|(op, n)| match op {
                "<" => value < n,
                ">" => value > n,
                "<=" => value <= n,
                ">=" => value >= n,
                "=" => value == n,
                "<>" => value != n,
                _ => false,
            });
            if matches {
                return Some((section, false));
            }
        }
        return None;
    }
    let index = if value < 0.0 && numeric.len() >= 2 {
        1
    } else if value == 0.0 && numeric.len() >= 3 {
        2
    } else {
        0
    };
    Some((numeric[index], index == 1))
}

fn tokenize(section: &str) -> Vec<Token> {
    let chars: Vec<_> = section.chars().collect();
    let mut result = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '"' => {
                let mut text = String::new();
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    text.push(chars[i]);
                    i += 1;
                }
                result.push(Token::Literal(text));
            }
            '\\' => {
                i += 1;
                if i < chars.len() {
                    result.push(Token::Literal(chars[i].to_string()));
                }
            }
            '_' => {
                i += 1;
                result.push(Token::Literal(" ".into()));
            }
            '*' => {
                i += 1;
                if i < chars.len() {
                    result.push(Token::Literal(chars[i].to_string()));
                }
            }
            '[' => {
                let start = i + 1;
                i += 1;
                while i < chars.len() && chars[i] != ']' {
                    i += 1;
                }
                let content: String = chars[start..i].iter().collect();
                let lower = content.to_ascii_lowercase();
                if !lower.is_empty() && lower.chars().all(|c| c == 'h' || c == 'm' || c == 's') {
                    result.push(Token::Elapsed(
                        lower.chars().next().unwrap_or('h'),
                        lower.len(),
                    ));
                } else if let Some(currency) = content.strip_prefix('$') {
                    let symbol = currency.split('-').next().unwrap_or("");
                    if !symbol.is_empty() {
                        result.push(Token::Literal(symbol.into()));
                    }
                }
            }
            'A' | 'a'
                if chars[i..]
                    .iter()
                    .take(5)
                    .collect::<String>()
                    .eq_ignore_ascii_case("AM/PM") =>
            {
                result.push(Token::AmPm(false));
                i += 4;
            }
            'A' | 'a'
                if chars[i..]
                    .iter()
                    .take(3)
                    .collect::<String>()
                    .eq_ignore_ascii_case("A/P") =>
            {
                result.push(Token::AmPm(true));
                i += 2;
            }
            ch => result.push(Token::Symbol(ch)),
        }
        i += 1;
    }
    result
}
fn is_placeholder(token: &Token) -> bool {
    matches!(token, Token::Symbol('0' | '#' | '?'))
}
fn literal(token: &Token) -> String {
    match token {
        Token::Literal(text) => text.clone(),
        Token::Symbol('@') => String::new(),
        Token::Symbol(ch) => ch.to_string(),
        _ => String::new(),
    }
}
fn render_literals(tokens: &[Token]) -> String {
    tokens.iter().map(literal).collect()
}
fn is_date(tokens: &[Token]) -> bool {
    tokens.iter().any(|t| {
        matches!(
            t,
            Token::Symbol('y' | 'Y' | 'd' | 'D' | 'h' | 'H' | 'm' | 'M' | 's' | 'S')
                | Token::Elapsed(..)
                | Token::AmPm(_)
        )
    })
}

fn grouped(digits: &str) -> String {
    digits
        .chars()
        .enumerate()
        .flat_map(|(i, ch)| {
            let comma = i > 0 && (digits.len() - i).is_multiple_of(3);
            comma.then_some(',').into_iter().chain(std::iter::once(ch))
        })
        .collect()
}

fn fixed_format(value: f64, tokens: &[Token]) -> Option<String> {
    let Some(mut first) = tokens.iter().position(is_placeholder) else {
        return Some(render_literals(tokens));
    };
    let last = tokens.iter().rposition(is_placeholder)?;
    if first > 0 && matches!(tokens[first - 1], Token::Symbol('.')) {
        first -= 1;
    }
    let decimal = tokens[first..=last]
        .iter()
        .position(|t| matches!(t, Token::Symbol('.')))
        .map(|i| i + first);
    let integer_end = decimal.unwrap_or(last + 1);
    let integer_tokens = &tokens[first..integer_end];
    let trailing_before_decimal = integer_tokens
        .iter()
        .rev()
        .take_while(|t| matches!(t, Token::Symbol(',')))
        .count();
    let trailing_after = tokens[last + 1..]
        .iter()
        .take_while(|t| matches!(t, Token::Symbol(',')))
        .count();
    let scale = trailing_before_decimal + trailing_after;
    let percent = tokens
        .iter()
        .filter(|t| matches!(t, Token::Symbol('%')))
        .count();
    if scale > 10 || percent > 10 {
        return None;
    }
    let number = value * 100f64.powi(percent.min(10) as i32) / 1000f64.powi(scale.min(10) as i32);
    if !number.is_finite() {
        return None;
    }
    let decimals: Vec<char> = decimal
        .map(|i| {
            tokens[i + 1..=last]
                .iter()
                .filter_map(|t| {
                    if let Token::Symbol(ch @ ('0' | '#' | '?')) = t {
                        Some(*ch)
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    if decimals.len() > 15 {
        return None;
    }
    let multiplier = 10f64.powi(decimals.len() as i32);
    let scaled = number * multiplier;
    let rounded_number = if scaled.is_finite() {
        scaled.round() / multiplier
    } else {
        number
    };
    let rounded = format!("{:.*}", decimals.len(), rounded_number);
    let (whole, fractional) = rounded.split_once('.').unwrap_or((&rounded, ""));
    let required = integer_tokens
        .iter()
        .filter(|t| matches!(t, Token::Symbol('0')))
        .count();
    let mut digits = if whole == "0" && required == 0 {
        String::new()
    } else {
        whole.to_owned()
    };
    while digits.len() < required {
        digits.insert(0, '0');
    }
    let grouping = integer_tokens[..integer_tokens.len().saturating_sub(trailing_before_decimal)]
        .iter()
        .any(|t| matches!(t, Token::Symbol(',')));
    let masked = integer_tokens
        .iter()
        .any(|t| !is_placeholder(t) && !matches!(t, Token::Symbol(',')));
    let mut integer = if masked {
        let mut source = digits.chars().rev();
        let mut output: Vec<String> = Vec::new();
        for token in integer_tokens.iter().rev() {
            output.push(if is_placeholder(token) {
                source.next().map(|c| c.to_string()).unwrap_or_else(|| {
                    if matches!(token, Token::Symbol('?')) {
                        " ".into()
                    } else {
                        String::new()
                    }
                })
            } else {
                literal(token)
            });
        }
        let remaining: String = source.collect::<Vec<_>>().into_iter().rev().collect();
        format!(
            "{remaining}{}",
            output.into_iter().rev().collect::<String>()
        )
    } else if grouping {
        grouped(&digits)
    } else {
        digits
    };
    let spaces = integer_tokens
        .iter()
        .filter(|t| matches!(t, Token::Symbol('?')))
        .count();
    let placeholders = integer_tokens.iter().filter(|t| is_placeholder(t)).count();
    if spaces > 0 && !masked {
        integer = format!(
            "{}{}",
            " ".repeat(placeholders.saturating_sub(whole.len()).min(spaces)),
            integer
        );
    }
    let mut fraction = fractional.to_owned();
    let mut significant = fraction.trim_end_matches('0').len();
    for (i, ch) in decimals.iter().enumerate() {
        if *ch == '0' {
            significant = significant.max(i + 1);
        }
    }
    fraction.truncate(significant);
    for ch in decimals.iter().skip(significant) {
        if *ch == '?' {
            fraction.push(' ');
        }
    }
    let separator = if decimal.is_some() && (!fraction.is_empty() || decimals.is_empty()) {
        "."
    } else {
        ""
    };
    Some(format!(
        "{}{}{}{}{}",
        render_literals(&tokens[..first]),
        integer,
        separator,
        fraction,
        render_literals(&tokens[last + 1 + trailing_after..])
    ))
}

fn scientific_format(value: f64, tokens: &[Token]) -> Option<String> {
    let e = tokens
        .iter()
        .position(|t| matches!(t, Token::Symbol('E' | 'e')))?;
    let first = tokens[..e].iter().position(is_placeholder)?;
    let decimal = tokens[first..e]
        .iter()
        .position(|t| matches!(t, Token::Symbol('.')))
        .map(|i| i + first);
    let integer_places = tokens[first..decimal.unwrap_or(e)]
        .iter()
        .filter(|t| is_placeholder(t))
        .count()
        .max(1);
    let precision = decimal
        .map(|d| {
            tokens[d + 1..e]
                .iter()
                .filter(|t| is_placeholder(t))
                .count()
        })
        .unwrap_or(0)
        .min(14);
    let mut exponent = if value == 0.0 {
        0
    } else {
        value.log10().floor() as i32
    };
    if integer_places > 15 {
        return None;
    }
    let engineering = tokens[first..decimal.unwrap_or(e)]
        .iter()
        .any(|t| matches!(t, Token::Symbol('#')));
    // Optional integer positions select engineering notation, while 00.0 keeps
    // two significant integer digits at every exponent.
    let exponent_step = if engineering {
        integer_places as i32
    } else {
        1
    };
    if value != 0.0 {
        exponent = if engineering {
            exponent.div_euclid(exponent_step) * exponent_step
        } else {
            exponent - integer_places as i32 + 1
        };
    }
    let mut mantissa = value / 10f64.powi(exponent);
    let multiplier = 10f64.powi(precision as i32);
    if (mantissa * multiplier).round() / multiplier >= 10f64.powi(integer_places as i32) {
        exponent += exponent_step;
        mantissa = value / 10f64.powi(exponent);
    }
    let mut end = e + 1;
    let positive_sign = matches!(tokens.get(end), Some(Token::Symbol('+')));
    if matches!(tokens.get(end), Some(Token::Symbol('+' | '-'))) {
        end += 1;
    }
    let digit_start = end;
    while end < tokens.len() && is_placeholder(&tokens[end]) {
        end += 1;
    }
    let width = end.saturating_sub(digit_start).max(1);
    let sign = if exponent < 0 {
        "-"
    } else if positive_sign {
        "+"
    } else {
        ""
    };
    let mantissa = fixed_format(mantissa, &tokens[first..e])?;
    Some(format!(
        "{}{}E{}{:0width$}{}",
        render_literals(&tokens[..first]),
        mantissa,
        sign,
        exponent.unsigned_abs(),
        render_literals(&tokens[end..])
    ))
}

fn fraction_format(value: f64, tokens: &[Token]) -> Option<String> {
    if value >= u64::MAX as f64 {
        return None;
    }
    let slash = tokens
        .iter()
        .position(|t| matches!(t, Token::Symbol('/')))?;
    let mut numerator_start = slash;
    while numerator_start > 0 && is_placeholder(&tokens[numerator_start - 1]) {
        numerator_start -= 1;
    }
    if numerator_start == slash {
        return None;
    }
    let mut end = slash + 1;
    while end < tokens.len() && matches!(tokens[end], Token::Symbol('#' | '?' | '0'..='9')) {
        end += 1;
    }
    if end == slash + 1 {
        return None;
    }
    let denominator_tokens = &tokens[slash + 1..end];
    let fixed: String = denominator_tokens
        .iter()
        .filter_map(|t| {
            if let Token::Symbol(ch @ '0'..='9') = t {
                Some(*ch)
            } else {
                None
            }
        })
        .collect();
    let fixed_denominator = if fixed.len() == denominator_tokens.len() {
        fixed.parse::<u64>().ok().filter(|&n| n > 0)
    } else {
        None
    };
    let first = tokens[..=numerator_start].iter().position(is_placeholder)?;
    let mixed = first < numerator_start;
    let mut whole = if mixed { value.floor() as u64 } else { 0 };
    let fraction = value - whole as f64;
    let (mut numerator, denominator) = if let Some(denominator) = fixed_denominator {
        ((fraction * denominator as f64).round() as u64, denominator)
    } else {
        let max = 10u64.pow(denominator_tokens.len().min(4) as u32) - 1;
        let mut best = (0, 1, f64::INFINITY);
        for denominator in 1..=max {
            let numerator = (fraction * denominator as f64).round() as u64;
            let error = (fraction - numerator as f64 / denominator as f64).abs();
            if error < best.2 {
                best = (numerator, denominator, error);
            }
            if error < 1e-12 {
                break;
            }
        }
        (best.0, best.1)
    };
    if mixed && numerator >= denominator {
        whole += numerator / denominator;
        numerator %= denominator;
    }
    let body = if numerator == 0 && mixed {
        whole.to_string()
    } else if mixed && whole != 0 {
        format!("{whole} {numerator}/{denominator}")
    } else {
        format!("{numerator}/{denominator}")
    };
    Some(format!(
        "{}{}{}",
        render_literals(&tokens[..first]),
        body,
        render_literals(&tokens[end..])
    ))
}

#[derive(Clone, Copy)]
struct DateParts {
    year: i64,
    month: u32,
    day: u32,
    weekday: usize,
    hour: u64,
    minute: u64,
    second: u64,
    fraction: u64,
    elapsed_seconds: u64,
}

fn date_format(serial: f64, tokens: &[Token], date1904: bool) -> Option<String> {
    if !(0.0..=2_958_465.999_999_99).contains(&serial) {
        return None;
    }
    let precision = tokens
        .windows(2)
        .enumerate()
        .find_map(|(i, pair)| {
            if matches!(pair[0], Token::Symbol('.')) && matches!(pair[1], Token::Symbol('0')) {
                Some(
                    tokens[i + 1..]
                        .iter()
                        .take_while(|t| matches!(t, Token::Symbol('0')))
                        .count()
                        .min(6),
                )
            } else {
                None
            }
        })
        .unwrap_or(0);
    let has_seconds = tokens
        .iter()
        .any(|t| matches!(t, Token::Symbol('s' | 'S') | Token::Elapsed('s', _)));
    let multiplier = 10u64.pow(precision as u32);
    let day_ticks = 86_400 * multiplier;
    let mut day = serial.floor() as i64;
    let fractional = (serial - serial.floor()) * day_ticks as f64;
    let ticks = if has_seconds {
        fractional.round() as u64
    } else {
        fractional.floor() as u64
    };
    day += (ticks / day_ticks) as i64;
    let seconds = (ticks % day_ticks) / multiplier;
    let unix = if date1904 {
        days_from_civil(1904, 1, 1) + day
    } else {
        days_from_civil(1899, 12, 31) + day - i64::from(day >= 60)
    };
    let (mut year, mut month, mut date) = civil_from_days(unix);
    if !date1904 && day == 60 {
        year = 1900;
        month = 2;
        date = 29;
    }
    if !date1904 && day == 0 {
        year = 1900;
        month = 1;
        date = 0;
    }
    let parts = DateParts {
        year,
        month,
        day: date,
        weekday: (unix + 4).rem_euclid(7) as usize,
        hour: seconds / 3600,
        minute: seconds / 60 % 60,
        second: seconds % 60,
        fraction: ticks % multiplier,
        elapsed_seconds: day as u64 * 86_400 + seconds,
    };
    let ampm = tokens.iter().any(|t| matches!(t, Token::AmPm(_)));
    let mut output = String::new();
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Literal(text) => output.push_str(text),
            Token::AmPm(short) => output.push_str(match (parts.hour >= 12, short) {
                (false, true) => "A",
                (true, true) => "P",
                (false, false) => "AM",
                (true, false) => "PM",
            }),
            Token::Elapsed(unit, width) => {
                let value = parts.elapsed_seconds
                    / match unit {
                        'h' => 3600,
                        'm' => 60,
                        _ => 1,
                    };
                output.push_str(&format!("{value:0width$}"));
            }
            Token::Symbol('.')
                if precision > 0 && matches!(tokens.get(i + 1), Some(Token::Symbol('0'))) =>
            {
                output.push_str(&format!(".{:0precision$}", parts.fraction));
                while matches!(tokens.get(i + 1), Some(Token::Symbol('0'))) {
                    i += 1;
                }
            }
            Token::Symbol(ch) if matches!(ch.to_ascii_lowercase(), 'y' | 'm' | 'd' | 'h' | 's') => {
                let kind = ch.to_ascii_lowercase();
                let start = i;
                while matches!(tokens.get(i + 1), Some(Token::Symbol(next)) if next.eq_ignore_ascii_case(ch))
                {
                    i += 1;
                }
                let count = i - start + 1;
                let minute = kind == 'm'
                    && count <= 2
                    && (previous_time_token(tokens, start) == Some('h')
                        || next_time_token(tokens, i + 1) == Some('s'));
                let text = match kind {
                    'y' if count <= 2 => format!("{:02}", parts.year.rem_euclid(100)),
                    'y' => format!("{:04}", parts.year),
                    'm' if minute => padded(parts.minute, count),
                    'm' if count <= 2 => padded(u64::from(parts.month), count),
                    'm' => {
                        let name = [
                            "January",
                            "February",
                            "March",
                            "April",
                            "May",
                            "June",
                            "July",
                            "August",
                            "September",
                            "October",
                            "November",
                            "December",
                        ][(parts.month - 1) as usize];
                        if count == 3 {
                            name[..3].into()
                        } else if count >= 5 {
                            name[..1].into()
                        } else {
                            name.into()
                        }
                    }
                    'd' if count <= 2 => padded(u64::from(parts.day), count),
                    'd' => {
                        let name = [
                            "Sunday",
                            "Monday",
                            "Tuesday",
                            "Wednesday",
                            "Thursday",
                            "Friday",
                            "Saturday",
                        ][parts.weekday];
                        if count == 3 {
                            name[..3].into()
                        } else {
                            name.into()
                        }
                    }
                    'h' => padded(
                        if ampm {
                            (parts.hour + 11) % 12 + 1
                        } else {
                            parts.hour
                        },
                        count,
                    ),
                    's' => padded(parts.second, count),
                    _ => String::new(),
                };
                output.push_str(&text);
            }
            Token::Symbol(ch) => output.push(*ch),
        }
        i += 1;
    }
    Some(output)
}
fn padded(value: u64, count: usize) -> String {
    if count >= 2 {
        format!("{value:02}")
    } else {
        value.to_string()
    }
}
fn previous_time_token(tokens: &[Token], position: usize) -> Option<char> {
    tokens[..position].iter().rev().find_map(|t| match t {
        Token::Symbol(ch) if ch.is_ascii_alphabetic() => Some(ch.to_ascii_lowercase()),
        Token::Elapsed(ch, _) => Some(*ch),
        _ => None,
    })
}
fn next_time_token(tokens: &[Token], position: usize) -> Option<char> {
    tokens[position..].iter().find_map(|t| match t {
        Token::Symbol(ch) if ch.is_ascii_alphabetic() => Some(ch.to_ascii_lowercase()),
        Token::Elapsed(ch, _) => Some(*ch),
        _ => None,
    })
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let mp = i64::from(month) + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month as u32, day as u32)
}
fn iso_serial(value: &str, date1904: bool) -> Option<f64> {
    let date = value.get(..10)?;
    let mut pieces = date.split('-');
    let year = pieces.next()?.parse().ok()?;
    let month = pieces.next()?.parse().ok()?;
    let day = pieces.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let unix = days_from_civil(year, month, day);
    let serial = if date1904 {
        unix - days_from_civil(1904, 1, 1)
    } else {
        unix - days_from_civil(1899, 12, 31) + i64::from((year, month, day) >= (1900, 3, 1))
    };
    let time = value.get(11..).unwrap_or("");
    let mut parts = time.trim_end_matches('Z').split(':');
    let hour: f64 = parts.next().unwrap_or("0").parse().unwrap_or(0.0);
    let minute: f64 = parts.next().unwrap_or("0").parse().unwrap_or(0.0);
    let second: f64 = parts.next().unwrap_or("0").parse().unwrap_or(0.0);
    Some(serial as f64 + (hour * 3600.0 + minute * 60.0 + second) / 86_400.0)
}
