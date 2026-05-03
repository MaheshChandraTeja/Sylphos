//! Script-side CSSOM effect extraction.
#![allow(dead_code)]
//!
//! This module is intentionally conservative. Until Syphos embeds a full JS
//! runtime, it extracts common CSSOM mutations from script source and feeds them
//! into the engine's CSSOM/invalidation pipeline.

use present::{CssDeclarationLite, CssPropertyName, CssomMutation};

/// CSSOM effects detected during a script pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ScriptCssomEffects {
    pub(crate) mutations: Vec<CssomMutation>,
    pub(crate) computed_style_queries: usize,
    pub(crate) style_sheet_reads: usize,
    pub(crate) class_mutations: usize,
}

impl ScriptCssomEffects {
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.mutations.is_empty()
            && self.computed_style_queries == 0
            && self.style_sheet_reads == 0
            && self.class_mutations == 0
    }

    pub(crate) fn merge(&mut self, other: Self) {
        self.mutations.extend(other.mutations);
        self.computed_style_queries = self
            .computed_style_queries
            .saturating_add(other.computed_style_queries);
        self.style_sheet_reads = self
            .style_sheet_reads
            .saturating_add(other.style_sheet_reads);
        self.class_mutations = self.class_mutations.saturating_add(other.class_mutations);
    }
}

/// Extracts CSSOM-like effects from a script source.
#[must_use]
pub(crate) fn extract_cssom_effects(script_source: &str) -> ScriptCssomEffects {
    let mut effects = ScriptCssomEffects {
        computed_style_queries: count_occurrences(script_source, "getComputedStyle("),
        style_sheet_reads: count_occurrences(script_source, "document.styleSheets"),
        ..ScriptCssomEffects::default()
    };

    extract_style_assignments(script_source, &mut effects);
    extract_set_property_calls(script_source, &mut effects);
    extract_insert_rule_calls(script_source, &mut effects);
    extract_class_list_calls(script_source, &mut effects);

    effects
}

fn extract_style_assignments(source: &str, effects: &mut ScriptCssomEffects) {
    for property in [
        ("backgroundColor", "background-color"),
        ("background", "background"),
        ("color", "color"),
        ("display", "display"),
        ("fontSize", "font-size"),
        ("margin", "margin"),
        ("padding", "padding"),
        ("border", "border"),
        ("width", "width"),
        ("height", "height"),
    ] {
        let pattern = format!(".style.{}", property.0);
        let mut rest = source;

        while let Some(index) = rest.find(&pattern) {
            let before = &rest[..index];
            let selector = infer_selector_from_prefix(before).unwrap_or_else(|| "body".to_owned());
            let after = &rest[index + pattern.len()..];

            if let Some((value, consumed)) = parse_assignment_value(after) {
                effects.mutations.push(CssomMutation::SetInlineStyle {
                    selector,
                    property: CssPropertyName::new(property.1),
                    value,
                    important: false,
                });
                rest = &after[consumed..];
            } else {
                rest = &after[after.len().min(1)..];
            }
        }
    }
}

fn extract_set_property_calls(source: &str, effects: &mut ScriptCssomEffects) {
    let mut rest = source;
    let pattern = ".style.setProperty(";

    while let Some(index) = rest.find(pattern) {
        let before = &rest[..index];
        let selector = infer_selector_from_prefix(before).unwrap_or_else(|| "body".to_owned());
        let after = &rest[index + pattern.len()..];

        if let Some((property, value, consumed)) = parse_two_string_arguments(after) {
            effects.mutations.push(CssomMutation::SetInlineStyle {
                selector,
                property: CssPropertyName::new(property),
                value,
                important: false,
            });
            rest = &after[consumed..];
        } else {
            rest = &after[after.len().min(1)..];
        }
    }
}

fn extract_insert_rule_calls(source: &str, effects: &mut ScriptCssomEffects) {
    let mut rest = source;
    let pattern = ".insertRule(";

    while let Some(index) = rest.find(pattern) {
        let after = &rest[index + pattern.len()..];

        if let Some((rule_source, consumed)) = parse_first_string_argument(after) {
            if let Some((selector, declaration_source)) = split_rule(&rule_source) {
                let declarations = declaration_source
                    .split(';')
                    .filter_map(|entry| {
                        let (property, value) = entry.split_once(':')?;
                        Some(CssDeclarationLite::new(property, value))
                    })
                    .collect::<Vec<_>>();

                effects.mutations.push(CssomMutation::InsertRule {
                    selector,
                    declarations,
                });
            }
            rest = &after[consumed..];
        } else {
            rest = &after[after.len().min(1)..];
        }
    }
}

fn extract_class_list_calls(source: &str, effects: &mut ScriptCssomEffects) {
    extract_class_list_method(
        source,
        ".classList.add(",
        effects,
        |selector, class_name| CssomMutation::AddClass {
            selector,
            class_name,
        },
    );
    extract_class_list_method(
        source,
        ".classList.remove(",
        effects,
        |selector, class_name| CssomMutation::RemoveClass {
            selector,
            class_name,
        },
    );
    extract_class_list_method(
        source,
        ".classList.toggle(",
        effects,
        |selector, class_name| CssomMutation::ToggleClass {
            selector,
            class_name,
        },
    );
}

fn extract_class_list_method(
    source: &str,
    pattern: &str,
    effects: &mut ScriptCssomEffects,
    make: impl Fn(String, String) -> CssomMutation,
) {
    let mut rest = source;

    while let Some(index) = rest.find(pattern) {
        let before = &rest[..index];
        let selector = infer_selector_from_prefix(before).unwrap_or_else(|| "body".to_owned());
        let after = &rest[index + pattern.len()..];

        if let Some((class_name, consumed)) = parse_first_string_argument(after) {
            effects.mutations.push(make(selector, class_name));
            effects.class_mutations = effects.class_mutations.saturating_add(1);
            rest = &after[consumed..];
        } else {
            rest = &after[after.len().min(1)..];
        }
    }
}

fn infer_selector_from_prefix(prefix: &str) -> Option<String> {
    if prefix.ends_with("document.body") || prefix.ends_with("document.documentElement") {
        return Some("body".to_owned());
    }

    for needle in ["querySelector(", "getElementById("] {
        if let Some(index) = prefix.rfind(needle) {
            let after = &prefix[index + needle.len()..];
            if let Some((value, _)) = parse_first_string_argument(after) {
                if needle == "getElementById(" {
                    return Some(format!("#{value}"));
                }
                return Some(value);
            }
        }
    }

    None
}

fn parse_assignment_value(input: &str) -> Option<(String, usize)> {
    let trimmed = input.trim_start();
    let leading_whitespace = input.len().saturating_sub(trimmed.len());
    let after_equal = trimmed.strip_prefix('=')?;
    let trimmed_start = leading_whitespace + 1;
    let whitespace = after_equal
        .len()
        .saturating_sub(after_equal.trim_start().len());
    let start = trimmed_start + whitespace;
    let (value, consumed) = parse_string_literal(&input[start..])?;
    Some((value, start + consumed))
}

fn parse_first_string_argument(input: &str) -> Option<(String, usize)> {
    let trimmed = input.trim_start();
    let skipped = input.len().saturating_sub(trimmed.len());
    let (value, consumed) = parse_string_literal(trimmed)?;
    Some((value, skipped + consumed))
}

fn parse_two_string_arguments(input: &str) -> Option<(String, String, usize)> {
    let (first, first_consumed) = parse_first_string_argument(input)?;
    let after_first = &input[first_consumed..];
    let comma = after_first.find(',')?;
    let after_comma = &after_first[comma + 1..];
    let (second, second_consumed) = parse_first_string_argument(after_comma)?;
    Some((first, second, first_consumed + comma + 1 + second_consumed))
}

fn parse_string_literal(input: &str) -> Option<(String, usize)> {
    let mut chars = input.char_indices();
    let (_, quote) = chars.next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }

    let mut value = String::new();
    let mut escaped = false;

    for (index, ch) in chars {
        if escaped {
            value.push(ch);
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            continue;
        }

        if ch == quote {
            return Some((value, index + ch.len_utf8()));
        }

        value.push(ch);
    }

    None
}

fn split_rule(source: &str) -> Option<(String, String)> {
    let open = source.find('{')?;
    let close = source.rfind('}')?;
    if close <= open {
        return None;
    }

    Some((
        source[..open].trim().to_owned(),
        source[open + 1..close].trim().to_owned(),
    ))
}

fn count_occurrences(source: &str, needle: &str) -> usize {
    source.match_indices(needle).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_style_assignment() {
        let effects = extract_cssom_effects("document.body.style.backgroundColor = '#fff';");
        assert_eq!(effects.mutations.len(), 1);
    }

    #[test]
    fn extracts_query_selector_set_property() {
        let effects = extract_cssom_effects(
            "document.querySelector('.hero').style.setProperty('color', '#123');",
        );
        assert_eq!(effects.mutations.len(), 1);
    }
}
