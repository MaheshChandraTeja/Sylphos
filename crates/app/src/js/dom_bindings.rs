//! DOM binding extraction and render-document mutation helpers.
//!
//! Module 24 keeps the JavaScript host deliberately bounded: it recognizes a
//! practical subset of browser DOM calls and converts them into deterministic
//! document mutations. A future V8-backed engine can replace the recognizer while
//! keeping this mutation boundary intact.

use present::{
    edit_form_control, AncestorSignature, ElementSignature, FormControl, FormTextEdit,
    InlineFragment, RenderBlock, RenderDocument,
};
use tracing::debug;

/// One script-visible DOM side effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DomBindingEffect {
    /// `node.textContent = ...` or `node.innerText = ...`.
    SetText { selector: String, value: String },

    /// `node.innerHTML = ...`; currently applied as text-only content.
    SetHtml { selector: String, value: String },

    /// `node.setAttribute(name, value)`.
    SetAttribute {
        selector: String,
        name: String,
        value: String,
    },

    /// `input.value = ...`.
    SetValue { selector: String, value: String },

    /// `node.classList.add(...)`.
    AddClass {
        selector: String,
        class_name: String,
    },

    /// `node.classList.remove(...)`.
    RemoveClass {
        selector: String,
        class_name: String,
    },

    /// `addEventListener(...)` registration captured for the browser event loop.
    RegisterEventListener { target: String, event_type: String },

    /// `dispatchEvent(new Event(...))` captured as a script-originated event.
    DispatchEvent { target: String, event_type: String },
}

/// Result of scanning script source for DOM binding operations.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DomBindingCapture {
    /// Captured effects.
    pub effects: Vec<DomBindingEffect>,

    /// Event-listener registrations found in source.
    pub registered_listeners: usize,

    /// Explicit dispatch calls found in source.
    pub queued_events: usize,

    /// Non-fatal binding warnings.
    pub warnings: Vec<String>,
}

/// Result of applying DOM binding effects to a render document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DomBindingApplyReport {
    /// Effects attempted.
    pub attempted: usize,

    /// Effects that changed the render document or runtime binding state.
    pub applied: usize,

    /// Effects ignored because no target matched or because the operation is unsupported.
    pub ignored: usize,

    /// Human-readable diagnostics.
    pub diagnostics: Vec<String>,
}

/// Captures a conservative subset of DOM API operations from script source.
#[must_use]
pub(crate) fn capture_dom_binding_effects(source: &str) -> DomBindingCapture {
    let mut capture = DomBindingCapture::default();

    capture
        .effects
        .extend(capture_text_assignments(source, "textContent", false));
    capture
        .effects
        .extend(capture_text_assignments(source, "innerText", false));
    capture
        .effects
        .extend(capture_text_assignments(source, "innerHTML", true));
    capture.effects.extend(capture_value_assignments(source));
    capture.effects.extend(capture_set_attribute_calls(source));
    capture
        .effects
        .extend(capture_class_list_calls(source, "add"));
    capture
        .effects
        .extend(capture_class_list_calls(source, "remove"));

    let listeners = capture_add_event_listener_calls(source);
    capture.registered_listeners = listeners.len();
    capture.effects.extend(listeners);

    let dispatches = capture_dispatch_event_calls(source);
    capture.queued_events = dispatches.len();
    capture.effects.extend(dispatches);

    if source.contains("document.createElement")
        || source.contains("appendChild")
        || source.contains("insertBefore")
    {
        capture.warnings.push(
            "script uses structural DOM creation APIs; Module 24 records hooks but does not fully materialize arbitrary nodes yet"
                .to_owned(),
        );
    }

    capture
}

/// Applies one captured effect to the current render document.
pub(crate) fn apply_dom_binding_effect(
    document: &mut RenderDocument,
    effect: &DomBindingEffect,
) -> DomBindingApplyReport {
    let mut report = DomBindingApplyReport {
        attempted: 1,
        ..DomBindingApplyReport::default()
    };

    let changed = match effect {
        DomBindingEffect::SetText { selector, value }
        | DomBindingEffect::SetHtml { selector, value } => {
            set_text_for_selector(document, selector, value)
        }
        DomBindingEffect::SetAttribute {
            selector,
            name,
            value,
        } => set_attribute_for_selector(document, selector, name, value),
        DomBindingEffect::SetValue { selector, value } => {
            set_form_value_for_selector(document, selector, value)
        }
        DomBindingEffect::AddClass {
            selector,
            class_name,
        } => add_class_for_selector(document, selector, class_name),
        DomBindingEffect::RemoveClass {
            selector,
            class_name,
        } => remove_class_for_selector(document, selector, class_name),
        DomBindingEffect::RegisterEventListener { target, event_type } => {
            debug!(target = %target, event_type = %event_type, "registered script event listener placeholder");
            true
        }
        DomBindingEffect::DispatchEvent { target, event_type } => {
            debug!(target = %target, event_type = %event_type, "queued script event dispatch placeholder");
            true
        }
    };

    if changed {
        report.applied = 1;
        document.recompute_style_tree();
    } else {
        report.ignored = 1;
        report.diagnostics.push(format!(
            "DOM binding effect did not match any document target: {effect:?}"
        ));
    }

    report
}

/// Applies all effects and returns aggregate diagnostics.
#[allow(dead_code)]
pub(crate) fn apply_dom_binding_effects(
    document: &mut RenderDocument,
    effects: &[DomBindingEffect],
) -> DomBindingApplyReport {
    let mut aggregate = DomBindingApplyReport::default();

    for effect in effects {
        let report = apply_dom_binding_effect(document, effect);
        aggregate.attempted = aggregate.attempted.saturating_add(report.attempted);
        aggregate.applied = aggregate.applied.saturating_add(report.applied);
        aggregate.ignored = aggregate.ignored.saturating_add(report.ignored);
        aggregate.diagnostics.extend(report.diagnostics);
    }

    aggregate
}

fn capture_text_assignments(source: &str, property: &str, html: bool) -> Vec<DomBindingEffect> {
    let mut effects = Vec::new();

    for target in capture_dom_targets(source) {
        let needle = format!("{}.{}", target.expression, property);
        let mut offset = 0usize;
        while let Some(relative) = source[offset..].find(&needle) {
            let assignment_start = offset + relative + needle.len();
            if let Some((value, end)) = extract_assignment_string(source, assignment_start) {
                let effect = if html {
                    DomBindingEffect::SetHtml {
                        selector: target.selector.clone(),
                        value,
                    }
                } else {
                    DomBindingEffect::SetText {
                        selector: target.selector.clone(),
                        value,
                    }
                };
                effects.push(effect);
                offset = end;
            } else {
                offset = assignment_start;
            }
        }
    }

    effects
}

fn capture_value_assignments(source: &str) -> Vec<DomBindingEffect> {
    let mut effects = Vec::new();

    for target in capture_dom_targets(source) {
        let needle = format!("{}.value", target.expression);
        let mut offset = 0usize;
        while let Some(relative) = source[offset..].find(&needle) {
            let assignment_start = offset + relative + needle.len();
            if let Some((value, end)) = extract_assignment_string(source, assignment_start) {
                effects.push(DomBindingEffect::SetValue {
                    selector: target.selector.clone(),
                    value,
                });
                offset = end;
            } else {
                offset = assignment_start;
            }
        }
    }

    effects
}

fn capture_set_attribute_calls(source: &str) -> Vec<DomBindingEffect> {
    let mut effects = Vec::new();

    for target in capture_dom_targets(source) {
        let needle = format!("{}.setAttribute", target.expression);
        let mut offset = 0usize;
        while let Some(relative) = source[offset..].find(&needle) {
            let call_start = offset + relative + needle.len();
            if let Some((args, end)) = extract_parenthesized(source, call_start) {
                let strings = js_string_arguments(args);
                if strings.len() >= 2 {
                    effects.push(DomBindingEffect::SetAttribute {
                        selector: target.selector.clone(),
                        name: strings[0].clone(),
                        value: strings[1].clone(),
                    });
                }
                offset = end;
            } else {
                offset = call_start;
            }
        }
    }

    effects
}

fn capture_class_list_calls(source: &str, operation: &str) -> Vec<DomBindingEffect> {
    let mut effects = Vec::new();

    for target in capture_dom_targets(source) {
        let needle = format!("{}.classList.{operation}", target.expression);
        let mut offset = 0usize;
        while let Some(relative) = source[offset..].find(&needle) {
            let call_start = offset + relative + needle.len();
            if let Some((args, end)) = extract_parenthesized(source, call_start) {
                if let Some(class_name) = js_string_arguments(args).first().cloned() {
                    let effect = if operation == "add" {
                        DomBindingEffect::AddClass {
                            selector: target.selector.clone(),
                            class_name,
                        }
                    } else {
                        DomBindingEffect::RemoveClass {
                            selector: target.selector.clone(),
                            class_name,
                        }
                    };
                    effects.push(effect);
                }
                offset = end;
            } else {
                offset = call_start;
            }
        }
    }

    effects
}

fn capture_add_event_listener_calls(source: &str) -> Vec<DomBindingEffect> {
    let mut effects = Vec::new();

    for target in capture_event_targets(source) {
        let needle = format!("{}.addEventListener", target.expression);
        let mut offset = 0usize;
        while let Some(relative) = source[offset..].find(&needle) {
            let call_start = offset + relative + needle.len();
            if let Some((args, end)) = extract_parenthesized(source, call_start) {
                if let Some(event_type) = js_string_arguments(args).first().cloned() {
                    effects.push(DomBindingEffect::RegisterEventListener {
                        target: target.selector.clone(),
                        event_type,
                    });
                }
                offset = end;
            } else {
                offset = call_start;
            }
        }
    }

    effects
}

fn capture_dispatch_event_calls(source: &str) -> Vec<DomBindingEffect> {
    let mut effects = Vec::new();

    for target in capture_event_targets(source) {
        let needle = format!("{}.dispatchEvent", target.expression);
        let mut offset = 0usize;
        while let Some(relative) = source[offset..].find(&needle) {
            let call_start = offset + relative + needle.len();
            if let Some((args, end)) = extract_parenthesized(source, call_start) {
                let event_type =
                    first_event_constructor_name(args).unwrap_or_else(|| "custom".to_owned());
                effects.push(DomBindingEffect::DispatchEvent {
                    target: target.selector.clone(),
                    event_type,
                });
                offset = end;
            } else {
                offset = call_start;
            }
        }
    }

    effects
}

#[derive(Debug, Clone)]
struct DomTarget {
    expression: String,
    selector: String,
}

fn capture_dom_targets(source: &str) -> Vec<DomTarget> {
    let mut targets = vec![
        DomTarget {
            expression: "document.body".to_owned(),
            selector: "body".to_owned(),
        },
        DomTarget {
            expression: "window.document.body".to_owned(),
            selector: "body".to_owned(),
        },
    ];

    targets.extend(capture_selector_calls(source, "document.querySelector"));
    targets.extend(capture_selector_calls(
        source,
        "window.document.querySelector",
    ));
    targets.extend(capture_get_element_by_id_calls(
        source,
        "document.getElementById",
    ));
    targets.extend(capture_get_element_by_id_calls(
        source,
        "window.document.getElementById",
    ));

    dedupe_targets(targets)
}

fn capture_event_targets(source: &str) -> Vec<DomTarget> {
    let mut targets = capture_dom_targets(source);
    targets.push(DomTarget {
        expression: "document".to_owned(),
        selector: "document".to_owned(),
    });
    targets.push(DomTarget {
        expression: "window".to_owned(),
        selector: "window".to_owned(),
    });
    dedupe_targets(targets)
}

fn capture_selector_calls(source: &str, function_name: &str) -> Vec<DomTarget> {
    let mut targets = Vec::new();
    let mut offset = 0usize;

    while let Some(relative) = source[offset..].find(function_name) {
        let start = offset + relative + function_name.len();
        if let Some((args, end)) = extract_parenthesized(source, start) {
            if let Some(selector) = js_string_arguments(args).first().cloned() {
                targets.push(DomTarget {
                    expression: source[offset + relative..end].to_owned(),
                    selector,
                });
            }
            offset = end;
        } else {
            offset = start;
        }
    }

    targets
}

fn capture_get_element_by_id_calls(source: &str, function_name: &str) -> Vec<DomTarget> {
    capture_selector_calls(source, function_name)
        .into_iter()
        .map(|mut target| {
            target.selector = format!("#{}", target.selector.trim_start_matches('#'));
            target
        })
        .collect()
}

fn dedupe_targets(targets: Vec<DomTarget>) -> Vec<DomTarget> {
    let mut out = Vec::new();
    for target in targets {
        if !out
            .iter()
            .any(|existing: &DomTarget| existing.expression == target.expression)
        {
            out.push(target);
        }
    }
    out
}

fn set_text_for_selector(document: &mut RenderDocument, selector: &str, value: &str) -> bool {
    if selector.eq_ignore_ascii_case("body") || selector.eq_ignore_ascii_case("document") {
        document.blocks.clear();
        document.block_elements.clear();
        document.push_block(
            RenderBlock::Paragraph {
                text: text_from_htmlish(value),
            },
            ElementSignature::synthetic("p", &[] as &[AncestorSignature]),
        );
        return true;
    }

    let Some(index) = first_matching_block_index(document, selector) else {
        return false;
    };

    set_block_text(&mut document.blocks[index], value)
}

fn set_block_text(block: &mut RenderBlock, value: &str) -> bool {
    let text = text_from_htmlish(value);
    match block {
        RenderBlock::Heading { text: target, .. }
        | RenderBlock::Paragraph { text: target }
        | RenderBlock::Link { text: target, .. }
        | RenderBlock::Generic { text: target, .. } => {
            *target = text;
            true
        }
        RenderBlock::InlineFlow { fragments } => {
            fragments.clear();
            fragments.push(InlineFragment::Text { text });
            true
        }
        RenderBlock::Image { alt, .. } => {
            *alt = Some(text);
            true
        }
        RenderBlock::Form(form) => {
            if let Some(control) = form.controls.iter_mut().find(|control| control.can_focus()) {
                control.value = text;
                return true;
            }
            false
        }
    }
}

fn set_attribute_for_selector(
    document: &mut RenderDocument,
    selector: &str,
    name: &str,
    value: &str,
) -> bool {
    let Some(index) = first_matching_block_index(document, selector) else {
        return false;
    };

    let name = name.trim().to_ascii_lowercase();
    let value = value.to_owned();
    mutate_block_attribute(&mut document.blocks[index], &name, &value);

    if let Some(element) = document.block_elements.get_mut(index) {
        set_signature_attr(element, &name, &value);
    }

    true
}

fn mutate_block_attribute(block: &mut RenderBlock, name: &str, value: &str) {
    match block {
        RenderBlock::Link { href, .. } if name == "href" => *href = Some(value.to_owned()),
        RenderBlock::Image { src, .. } if name == "src" => *src = Some(value.to_owned()),
        RenderBlock::Image { alt, .. } if name == "alt" => *alt = Some(value.to_owned()),
        RenderBlock::Form(form) if name == "action" => form.action = Some(value.to_owned()),
        _ => {}
    }
}

fn set_form_value_for_selector(document: &mut RenderDocument, selector: &str, value: &str) -> bool {
    let Some(control_id) = find_form_control_id(document, selector) else {
        return false;
    };
    edit_form_control(document, control_id, FormTextEdit::Set(value.to_owned()))
}

fn add_class_for_selector(document: &mut RenderDocument, selector: &str, class_name: &str) -> bool {
    let Some(index) = first_matching_block_index(document, selector) else {
        return false;
    };
    let class_name = class_name.trim().to_ascii_lowercase();
    if class_name.is_empty() {
        return false;
    }
    let Some(element) = document.block_elements.get_mut(index) else {
        return false;
    };
    if !element
        .classes
        .iter()
        .any(|existing| existing == &class_name)
    {
        element.classes.push(class_name);
    }
    sync_class_attr(element);
    true
}

fn remove_class_for_selector(
    document: &mut RenderDocument,
    selector: &str,
    class_name: &str,
) -> bool {
    let Some(index) = first_matching_block_index(document, selector) else {
        return false;
    };
    let class_name = class_name.trim().to_ascii_lowercase();
    let Some(element) = document.block_elements.get_mut(index) else {
        return false;
    };
    let before = element.classes.len();
    element.classes.retain(|existing| existing != &class_name);
    sync_class_attr(element);
    before != element.classes.len()
}

fn first_matching_block_index(document: &RenderDocument, selector: &str) -> Option<usize> {
    document
        .block_elements
        .iter()
        .enumerate()
        .find_map(|(index, element)| selector_matches_element(selector, element).then_some(index))
        .or_else(|| fallback_tag_match(document, selector))
}

fn fallback_tag_match(document: &RenderDocument, selector: &str) -> Option<usize> {
    let normalized = normalize_selector(selector);
    document
        .blocks
        .iter()
        .enumerate()
        .find_map(|(index, block)| {
            let tag = match block {
                RenderBlock::Heading { level, .. } => format!("h{level}"),
                RenderBlock::Paragraph { .. } | RenderBlock::InlineFlow { .. } => "p".to_owned(),
                RenderBlock::Link { .. } => "a".to_owned(),
                RenderBlock::Image { .. } => "img".to_owned(),
                RenderBlock::Form(_) => "form".to_owned(),
                RenderBlock::Generic { tag, .. } => tag.clone(),
            };
            (tag == normalized).then_some(index)
        })
}

fn selector_matches_element(selector: &str, element: &ElementSignature) -> bool {
    let selector = normalize_selector(selector);
    if selector.is_empty() || selector == "*" {
        return true;
    }

    if let Some(id) = selector.strip_prefix('#') {
        return element.id.as_deref() == Some(id);
    }

    if let Some(class_name) = selector.strip_prefix('.') {
        return element.classes.iter().any(|class| class == class_name);
    }

    if let Some((tag, class_name)) = selector.split_once('.') {
        return element.tag == tag && element.classes.iter().any(|class| class == class_name);
    }

    if let Some((tag, id)) = selector.split_once('#') {
        return element.tag == tag && element.id.as_deref() == Some(id);
    }

    element.tag == selector
}

fn find_form_control_id(document: &RenderDocument, selector: &str) -> Option<u64> {
    let selector = normalize_selector(selector);
    let name_filter = extract_name_filter(&selector);
    let tag = selector
        .split('[')
        .next()
        .unwrap_or(selector.as_str())
        .trim()
        .to_ascii_lowercase();

    for block in &document.blocks {
        let RenderBlock::Form(form) = block else {
            continue;
        };
        for control in &form.controls {
            if control_matches_selector(control, &tag, name_filter.as_deref()) {
                return Some(control.id);
            }
        }
    }
    None
}

fn control_matches_selector(control: &FormControl, tag: &str, name_filter: Option<&str>) -> bool {
    if let Some(name) = name_filter {
        if control.name.as_deref() != Some(name) {
            return false;
        }
    }

    matches!(tag, "input" | "textarea" | "button" | "select") || tag.is_empty()
}

fn extract_name_filter(selector: &str) -> Option<String> {
    let start = selector.find("[name=")? + "[name=".len();
    let rest = &selector[start..];
    let end = rest.find(']')?;
    Some(
        rest[..end]
            .trim_matches(|ch| matches!(ch, '\'' | '"'))
            .to_owned(),
    )
}

fn set_signature_attr(element: &mut ElementSignature, name: &str, value: &str) {
    if name == "id" {
        element.id = Some(value.trim().to_ascii_lowercase());
    } else if name == "class" {
        element.classes = value
            .split_whitespace()
            .map(str::to_ascii_lowercase)
            .collect();
    }

    if let Some((_, existing)) = element
        .attrs
        .iter_mut()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
    {
        *existing = value.to_owned();
    } else {
        element.attrs.push((name.to_owned(), value.to_owned()));
    }
}

fn sync_class_attr(element: &mut ElementSignature) {
    let value = element.classes.join(" ");
    set_signature_attr(element, "class", &value);
}

fn normalize_selector(selector: &str) -> String {
    selector
        .trim()
        .trim_matches(|ch| matches!(ch, '\'' | '"'))
        .split_whitespace()
        .last()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn text_from_htmlish(value: &str) -> String {
    value
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .split('<')
        .map(|part| part.split('>').nth(1).unwrap_or(part))
        .collect::<Vec<_>>()
        .join("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn extract_assignment_string(source: &str, start: usize) -> Option<(String, usize)> {
    let mut index = skip_ascii_ws(source, start);
    if source[index..].chars().next()? != '=' {
        return None;
    }
    index += 1;
    index = skip_ascii_ws(source, index);
    parse_js_string(source, index)
}

fn extract_parenthesized(source: &str, open_start: usize) -> Option<(&str, usize)> {
    let mut index = skip_ascii_ws(source, open_start);
    if source[index..].chars().next()? != '(' {
        return None;
    }

    index += 1;
    let content_start = index;
    let mut depth = 1usize;
    let mut in_string: Option<char> = None;
    let mut escape = false;

    for (relative, ch) in source[index..].char_indices() {
        let absolute = index + relative;

        if let Some(quote) = in_string {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == quote {
                in_string = None;
            }
            continue;
        }

        match ch {
            '\'' | '"' | '`' => in_string = Some(ch),
            '(' => depth = depth.saturating_add(1),
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some((&source[content_start..absolute], absolute + ch.len_utf8()));
                }
            }
            _ => {}
        }
    }

    None
}

fn js_string_arguments(args: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut rest = args.trim();
    while !rest.is_empty() {
        if let Some((value, end)) = first_js_string_with_end(rest) {
            values.push(value);
            rest = rest[end..].trim_start_matches(|ch| ch == ',' || char::is_whitespace(ch));
        } else {
            break;
        }
    }
    values
}

fn first_event_constructor_name(args: &str) -> Option<String> {
    let event_name = first_js_string_with_end(args).map(|(value, _)| value)?;
    Some(event_name)
}

fn first_js_string_with_end(input: &str) -> Option<(String, usize)> {
    let index = input.find(['\'', '"', '`'])?;
    let (value, end) = parse_js_string(input, index)?;
    Some((value, end))
}

fn parse_js_string(source: &str, start: usize) -> Option<(String, usize)> {
    let quote = source[start..].chars().next()?;
    if !matches!(quote, '\'' | '"' | '`') {
        return None;
    }

    let mut value = String::new();
    let mut escape = false;
    let mut index = start + quote.len_utf8();

    for ch in source[index..].chars() {
        index += ch.len_utf8();
        if escape {
            match ch {
                'n' => value.push('\n'),
                'r' => value.push('\r'),
                't' => value.push('\t'),
                '\\' => value.push('\\'),
                '\'' => value.push('\''),
                '"' => value.push('"'),
                '`' => value.push('`'),
                other => value.push(other),
            }
            escape = false;
            continue;
        }

        if ch == '\\' {
            escape = true;
        } else if ch == quote {
            return Some((value, index));
        } else {
            value.push(ch);
        }
    }

    None
}

fn skip_ascii_ws(source: &str, mut index: usize) -> usize {
    while index < source.len() {
        let Some(ch) = source[index..].chars().next() else {
            break;
        };
        if !ch.is_ascii_whitespace() {
            break;
        }
        index += ch.len_utf8();
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_query_selector_text_content_assignment() {
        let capture = capture_dom_binding_effects(
            r#"document.querySelector('#title').textContent = 'Updated';"#,
        );
        assert_eq!(capture.effects.len(), 1);
        assert!(matches!(
            &capture.effects[0],
            DomBindingEffect::SetText { selector, value } if selector == "#title" && value == "Updated"
        ));
    }
}
