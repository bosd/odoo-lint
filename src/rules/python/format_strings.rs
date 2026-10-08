//! Python format-string parsing, matching what pylint and pylint-odoo use:
//! `%`-style strings (`pylint.checkers.utils.parse_format_string`),
//! `str.format` fields (`string.Formatter().parse`) and the printf pattern
//! of pylint-odoo's `translation-positional-used`.

use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, PartialEq, Eq)]
pub enum PercentError {
    /// The string ends inside a conversion specifier.
    Incomplete,
    /// Unknown conversion character at this (character) index.
    Unsupported(usize),
}

/// Mapping keys and number of positional arguments of a `%`-style string.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PercentFormat {
    pub keys: Vec<String>,
    pub num_args: usize,
}

/// Parses a `%`-style format string the way pylint does.
pub fn parse_percent(format: &str) -> Result<PercentFormat, PercentError> {
    let chars: Vec<char> = format.chars().collect();
    let mut result = PercentFormat::default();
    let next = |i: usize| -> Result<(usize, char), PercentError> {
        let i = i + 1;
        chars.get(i).map(|c| (i, *c)).ok_or(PercentError::Incomplete)
    };
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' {
            let (mut j, mut c) = next(i)?;
            let mut key = None;
            if c == '(' {
                let mut depth = 1;
                (j, c) = next(j)?;
                let start = j;
                while depth != 0 {
                    if c == '(' {
                        depth += 1;
                    } else if c == ')' {
                        depth -= 1;
                    }
                    (j, c) = next(j)?;
                }
                key = Some(chars[start..j - 1].iter().collect::<String>());
            }
            while "#0- +".contains(c) {
                (j, c) = next(j)?;
            }
            if c == '*' {
                result.num_args += 1;
                (j, c) = next(j)?;
            } else {
                while c.is_ascii_digit() {
                    (j, c) = next(j)?;
                }
            }
            if c == '.' {
                (j, c) = next(j)?;
                if c == '*' {
                    result.num_args += 1;
                    (j, c) = next(j)?;
                } else {
                    while c.is_ascii_digit() {
                        (j, c) = next(j)?;
                    }
                }
            }
            if "hlL".contains(c) {
                (j, c) = next(j)?;
            }
            if !"diouxXeEfFgGcrs%a".contains(c) {
                return Err(PercentError::Unsupported(j));
            }
            match key {
                Some(key) if !key.is_empty() => result.keys.push(key),
                _ if c != '%' => result.num_args += 1,
                _ => {}
            }
            i = j;
        }
        i += 1;
    }
    Ok(result)
}

/// One replacement field of a `str.format` string.
#[derive(Debug, PartialEq, Eq)]
pub struct FormatField {
    pub name: String,
    pub spec: String,
}

/// Replacement fields of a `str.format` string, like
/// `string.Formatter().parse`; `None` where Python raises `ValueError`.
pub fn parse_format_fields(s: &str) -> Option<Vec<FormatField>> {
    let chars: Vec<char> = s.chars().collect();
    let mut fields = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '{' if chars.get(i + 1) == Some(&'{') => i += 2,
            '}' if chars.get(i + 1) == Some(&'}') => i += 2,
            '}' => return None,
            '{' => {
                // Find the matching '}', allowing nested fields in the spec.
                let mut depth = 1;
                let mut j = i + 1;
                while j < chars.len() && depth > 0 {
                    match chars[j] {
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        _ => {}
                    }
                    j += 1;
                }
                if depth != 0 {
                    return None;
                }
                let inner: Vec<char> = chars[i + 1..j - 1].to_vec();
                // The field name ends at ':' or '!' outside square brackets.
                let mut k = 0;
                let mut in_brackets = false;
                while k < inner.len() {
                    match inner[k] {
                        '[' => in_brackets = true,
                        ']' => in_brackets = false,
                        ':' | '!' if !in_brackets => break,
                        _ => {}
                    }
                    k += 1;
                }
                let name: String = inner[..k].iter().collect();
                let rest: String = inner[k..].iter().collect();
                if let Some(conversion) = rest.strip_prefix('!') {
                    let mut conv = conversion.chars();
                    let valid = conv.next().is_some() && matches!(conv.next(), None | Some(':'));
                    if !valid {
                        return None;
                    }
                }
                let spec = rest
                    .split_once(':')
                    .map(|(_, spec)| spec.to_string())
                    .unwrap_or_default();
                fields.push(FormatField { name, spec });
                i = j;
            }
            _ => i += 1,
        }
    }
    Some(fields)
}

/// Lines as Python's `str.splitlines()` splits them (common separators).
fn split_lines(s: &str) -> Vec<&str> {
    s.split("\r\n").flat_map(|part| part.split(['\n', '\r'])).collect()
}

/// pylint-odoo's `_get_format_str_args_kwargs(...)[0]`: number of positional
/// `str.format` arguments. Faithful to its accumulation over lines.
pub fn format_positional_count(s: &str) -> usize {
    let mut placeholders: Vec<String> = Vec::new();
    let mut args: Vec<usize> = Vec::new();
    for line in split_lines(s) {
        let Some(fields) = parse_format_fields(line) else {
            continue;
        };
        placeholders.extend(fields.into_iter().map(|f| f.name));
        for placeholder in &placeholders {
            if placeholder.is_empty() {
                args.push(0);
            } else if placeholder.chars().all(|c| c.is_ascii_digit()) {
                args.push(placeholder.parse::<usize>().unwrap_or(0) + 1);
            }
        }
    }
    match args.iter().max() {
        None => 0,
        Some(0) => args.len(),
        Some(max) => *max,
    }
}

static PRINTF_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"%((?P<boost_ord>\d+)%|(?:(?P<ord>\d+)\$|\((?P<key>\w+)\))?(?P<fullvar>[+#-]*(?:\d+)?(?:\.\d+)?(hh\|h\|l\|ll)?(?P<type>[\w@])))",
    )
    .unwrap()
});

/// pylint-odoo's `_get_printf_str_args_kwargs`: the number of positional
/// printf placeholders, or `None` when there are none (it returns kwargs).
pub fn printf_positional_count(s: &str) -> Option<usize> {
    let s = s.replace("%%", "");
    let count = split_lines(&s)
        .into_iter()
        .flat_map(|line| PRINTF_PATTERN.captures_iter(line).collect::<Vec<_>>())
        .filter(|caps| caps.name("key").is_none())
        .count();
    (count > 0).then_some(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_strings() {
        assert_eq!(parse_percent("%s and %d").unwrap().num_args, 2);
        assert_eq!(parse_percent("100%% sure").unwrap().num_args, 0);
        assert_eq!(parse_percent("%(name)s").unwrap().keys, vec!["name"]);
        assert_eq!(parse_percent("%*.*f").unwrap().num_args, 3);
        assert_eq!(parse_percent("50%"), Err(PercentError::Incomplete));
        assert_eq!(parse_percent("%y"), Err(PercentError::Unsupported(1)));
        assert_eq!(parse_percent("ab %-5.2lf").unwrap().num_args, 1);
    }

    #[test]
    fn format_fields() {
        let names = |s| {
            parse_format_fields(s)
                .unwrap()
                .into_iter()
                .map(|f| f.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names("{} {}"), vec!["", ""]);
        assert_eq!(names("{0} {name!r} {a[0]:>{1}}"), vec!["0", "name", "a[0]"]);
        assert_eq!(names("{{literal}}"), Vec::<String>::new());
        assert!(parse_format_fields("{").is_none());
        assert!(parse_format_fields("}").is_none());
        assert_eq!(parse_format_fields("{:.2f}").unwrap()[0].spec, ".2f");
    }

    #[test]
    fn positional_counts() {
        assert_eq!(format_positional_count("{} {}"), 2);
        assert_eq!(format_positional_count("{0} {1} {0}"), 2);
        assert_eq!(format_positional_count("{name}"), 0);
        assert_eq!(printf_positional_count("'%s %d'"), Some(2));
        assert_eq!(printf_positional_count("'%(a)s %(b)s'"), None);
        assert_eq!(printf_positional_count("'100%% %s'"), Some(1));
    }
}
