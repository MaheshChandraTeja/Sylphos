#![allow(clippy::too_many_lines)]
#![doc = "CSSOM and dynamic style mutation runtime for SylJS."]

use crate::{
    dom::{DomHost, DomNodeRef, SharedDomHost},
    event_loop::JsEventLoop,
    JsFunction, JsHostObject, JsObject, JsObjectKind, JsValue, Vm,
};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

/// Shared CSSOM host pointer.
pub type SharedCssomHost = Rc<dyn CssomHost>;

/// Style invalidation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleInvalidationKind {
    /// Color/text-only repaint.
    Paint,

    /// Layout-affecting change.
    Layout,

    /// Selector/class/rule change.
    StyleRecalc,
}

/// CSSOM metrics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CssomMetrics {
    /// Inline style reads.
    pub inline_reads: u64,

    /// Inline style writes.
    pub inline_writes: u64,

    /// Inline style removals.
    pub inline_removals: u64,

    /// Computed style reads.
    pub computed_reads: u64,

    /// Stylesheet reads.
    pub stylesheet_reads: u64,

    /// Rules inserted.
    pub rules_inserted: u64,

    /// Rules deleted.
    pub rules_deleted: u64,

    /// Invalidation events.
    pub invalidations: u64,
}

/// Style mutation record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssStyleMutation {
    /// Target node.
    pub node: Option<DomNodeRef>,

    /// Property name.
    pub property: String,

    /// New value.
    pub value: Option<String>,

    /// Invalidation kind.
    pub invalidation: StyleInvalidationKind,
}

/// CSS rule record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssRuleRecord {
    /// Rule id within stylesheet.
    pub id: u64,

    /// Selector text.
    pub selector: String,

    /// Declarations.
    pub declarations: BTreeMap<String, String>,

    /// Raw css text.
    pub css_text: String,
}

/// Stylesheet record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssStyleSheetRecord {
    /// Sheet id.
    pub id: u64,

    /// Rules.
    pub rules: Vec<CssRuleRecord>,
}

/// CSSOM host abstraction.
pub trait CssomHost {
    /// Gets inline style property.
    fn inline_style_get(&self, node: DomNodeRef, property: &str) -> Option<String>;

    /// Sets inline style property.
    fn inline_style_set(&self, node: DomNodeRef, property: &str, value: String);

    /// Removes inline style property.
    fn inline_style_remove(&self, node: DomNodeRef, property: &str);

    /// Returns full inline css text.
    fn inline_style_css_text(&self, node: DomNodeRef) -> String;

    /// Replaces full inline css text.
    fn inline_style_set_css_text(&self, node: DomNodeRef, css_text: String);

    /// Returns computed style property.
    fn computed_style_get(
        &self,
        dom: &dyn DomHost,
        node: DomNodeRef,
        property: &str,
    ) -> Option<String>;

    /// Inserts a stylesheet rule.
    fn insert_rule(&self, sheet_index: usize, rule: &str, index: Option<usize>) -> usize;

    /// Deletes a stylesheet rule.
    fn delete_rule(&self, sheet_index: usize, index: usize) -> bool;

    /// Returns stylesheet snapshot.
    fn stylesheet(&self, sheet_index: usize) -> CssStyleSheetRecord;

    /// Returns stylesheet count.
    fn stylesheet_count(&self) -> usize;

    /// Returns metrics.
    fn metrics(&self) -> CssomMetrics;

    /// Returns mutation records.
    fn mutations(&self) -> Vec<CssStyleMutation>;
}

/// Installs CSSOM globals and connects existing `document` with `styleSheets`.
pub fn install_cssom_globals(
    vm: &mut Vm,
    event_loop: Rc<RefCell<JsEventLoop>>,
    dom: SharedDomHost,
    cssom: SharedCssomHost,
) {
    let document = vm.get_name("document");

    if !matches!(document, JsValue::Undefined | JsValue::Null) {
        document.set_property("styleSheets", create_style_sheet_list_object(cssom.clone()));
    }

    vm.define_global(
        "getComputedStyle",
        create_get_computed_style(dom.clone(), cssom.clone()),
    );
    vm.define_global(
        "CSSStyleSheet",
        create_css_style_sheet_constructor(cssom.clone()),
    );
    vm.define_global(
        "__sylphosCssomMetrics",
        create_cssom_metrics_function(cssom.clone()),
    );

    // Keep event_loop referenced for future async style flush hooks.
    let _ = event_loop;
}

/// Creates an inline style declaration for an element.
#[must_use]
pub fn create_inline_style_object(cssom: SharedCssomHost, node: DomNodeRef) -> JsValue {
    JsValue::host_object(
        Rc::new(CssStyleDeclarationHost {
            cssom,
            dom: None,
            node: Some(node),
            readonly: false,
        }),
        "[object CSSStyleDeclaration]",
    )
}

/// Creates a computed style declaration for an element.
#[must_use]
pub fn create_computed_style_object(
    dom: SharedDomHost,
    cssom: SharedCssomHost,
    node: DomNodeRef,
) -> JsValue {
    JsValue::host_object(
        Rc::new(CssStyleDeclarationHost {
            cssom,
            dom: Some(dom),
            node: Some(node),
            readonly: true,
        }),
        "[object CSSStyleDeclaration]",
    )
}

#[derive(Clone)]
struct CssStyleDeclarationHost {
    cssom: SharedCssomHost,
    dom: Option<SharedDomHost>,
    node: Option<DomNodeRef>,
    readonly: bool,
}

impl JsHostObject for CssStyleDeclarationHost {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        match key {
            "cssText" => {
                let node = self.node?;
                Some(JsValue::String(self.cssom.inline_style_css_text(node)))
            }
            "getPropertyValue" => Some(style_get_property_value(
                self.cssom.clone(),
                self.dom.clone(),
                self.node,
            )),
            "setProperty" if !self.readonly => {
                Some(style_set_property(self.cssom.clone(), self.node?))
            }
            "removeProperty" if !self.readonly => {
                Some(style_remove_property(self.cssom.clone(), self.node?))
            }
            "item" => Some(style_item(self.cssom.clone(), self.node)),
            "length" => {
                let node = self.node?;
                Some(JsValue::Number(
                    parse_style_declarations(&self.cssom.inline_style_css_text(node)).len() as f64,
                ))
            }
            property => {
                let node = self.node?;
                let normalized = normalize_css_property(property);

                if self.readonly {
                    let dom = self.dom.as_ref()?;
                    Some(JsValue::String(
                        self.cssom
                            .computed_style_get(dom.as_ref(), node, &normalized)
                            .unwrap_or_default(),
                    ))
                } else {
                    Some(JsValue::String(
                        self.cssom
                            .inline_style_get(node, &normalized)
                            .unwrap_or_default(),
                    ))
                }
            }
        }
    }

    fn set_property(&self, key: &str, value: JsValue) -> bool {
        if self.readonly {
            return false;
        }

        let Some(node) = self.node else {
            return false;
        };

        if key == "cssText" {
            self.cssom
                .inline_style_set_css_text(node, value.to_js_string());
            return true;
        }

        let property = normalize_css_property(key);
        self.cssom
            .inline_style_set(node, &property, value.to_js_string());
        true
    }
}

fn style_get_property_value(
    cssom: SharedCssomHost,
    dom: Option<SharedDomHost>,
    node: Option<DomNodeRef>,
) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CSSStyleDeclaration.getPropertyValue".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let property = args.first().map_or_else(String::new, JsValue::to_js_string);
            let property = normalize_css_property(&property);
            let Some(node) = node else {
                return Ok(JsValue::String(String::new()));
            };

            if let Some(dom) = &dom {
                Ok(JsValue::String(
                    cssom
                        .computed_style_get(dom.as_ref(), node, &property)
                        .unwrap_or_default(),
                ))
            } else {
                Ok(JsValue::String(
                    cssom.inline_style_get(node, &property).unwrap_or_default(),
                ))
            }
        }),
    })
}

fn style_set_property(cssom: SharedCssomHost, node: DomNodeRef) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CSSStyleDeclaration.setProperty".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let property = args.first().map_or_else(String::new, JsValue::to_js_string);
            let value = args.get(1).map_or_else(String::new, JsValue::to_js_string);
            cssom.inline_style_set(node, &normalize_css_property(&property), value);
            Ok(JsValue::Undefined)
        }),
    })
}

fn style_remove_property(cssom: SharedCssomHost, node: DomNodeRef) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CSSStyleDeclaration.removeProperty".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let property = args.first().map_or_else(String::new, JsValue::to_js_string);
            let property = normalize_css_property(&property);
            let old = cssom.inline_style_get(node, &property).unwrap_or_default();
            cssom.inline_style_remove(node, &property);
            Ok(JsValue::String(old))
        }),
    })
}

fn style_item(cssom: SharedCssomHost, node: Option<DomNodeRef>) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CSSStyleDeclaration.item".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let index = args
                .first()
                .map_or(0, |value| value.to_number().max(0.0) as usize);
            let Some(node) = node else {
                return Ok(JsValue::String(String::new()));
            };
            let declarations = parse_style_declarations(&cssom.inline_style_css_text(node));
            let key = declarations.keys().nth(index).cloned().unwrap_or_default();
            Ok(JsValue::String(key))
        }),
    })
}

fn create_get_computed_style(dom: SharedDomHost, cssom: SharedCssomHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "getComputedStyle".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let node = args.first().and_then(JsValue::dom_node_id).map(DomNodeRef);
            Ok(node.map_or(JsValue::Null, |node| {
                create_computed_style_object(dom.clone(), cssom.clone(), node)
            }))
        }),
    })
}

fn create_style_sheet_list_object(cssom: SharedCssomHost) -> JsValue {
    JsValue::host_object(
        Rc::new(StyleSheetListHost { cssom }),
        "[object StyleSheetList]",
    )
}

#[derive(Clone)]
struct StyleSheetListHost {
    cssom: SharedCssomHost,
}

impl JsHostObject for StyleSheetListHost {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        if key == "length" {
            return Some(JsValue::Number(self.cssom.stylesheet_count() as f64));
        }

        if let Ok(index) = key.parse::<usize>() {
            return Some(create_css_style_sheet_object(self.cssom.clone(), index));
        }

        if key == "item" {
            return Some(style_sheet_list_item(self.cssom.clone()));
        }

        None
    }

    fn set_property(&self, _key: &str, _value: JsValue) -> bool {
        false
    }
}

fn style_sheet_list_item(cssom: SharedCssomHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "StyleSheetList.item".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let index = args
                .first()
                .map_or(0, |value| value.to_number().max(0.0) as usize);
            if index < cssom.stylesheet_count() {
                Ok(create_css_style_sheet_object(cssom.clone(), index))
            } else {
                Ok(JsValue::Null)
            }
        }),
    })
}

fn create_css_style_sheet_constructor(cssom: SharedCssomHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CSSStyleSheet".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            let index = cssom.stylesheet_count();
            cssom.insert_rule(index, "", Some(0));
            Ok(create_css_style_sheet_object(cssom.clone(), index))
        }),
    })
}

fn create_css_style_sheet_object(cssom: SharedCssomHost, sheet_index: usize) -> JsValue {
    let object = JsValue::host_object(
        Rc::new(CssStyleSheetHost { cssom, sheet_index }),
        "[object CSSStyleSheet]",
    );
    object
}

#[derive(Clone)]
struct CssStyleSheetHost {
    cssom: SharedCssomHost,
    sheet_index: usize,
}

impl JsHostObject for CssStyleSheetHost {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        match key {
            "insertRule" => Some(style_sheet_insert_rule(
                self.cssom.clone(),
                self.sheet_index,
            )),
            "deleteRule" => Some(style_sheet_delete_rule(
                self.cssom.clone(),
                self.sheet_index,
            )),
            "cssRules" | "rules" => Some(create_css_rule_list_object(
                self.cssom.clone(),
                self.sheet_index,
            )),
            "disabled" => Some(JsValue::Boolean(false)),
            _ => None,
        }
    }

    fn set_property(&self, _key: &str, _value: JsValue) -> bool {
        false
    }
}

fn style_sheet_insert_rule(cssom: SharedCssomHost, sheet_index: usize) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CSSStyleSheet.insertRule".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let rule = args.first().map_or_else(String::new, JsValue::to_js_string);
            let index = args.get(1).map(|value| value.to_number().max(0.0) as usize);
            let inserted = cssom.insert_rule(sheet_index, &rule, index);
            Ok(JsValue::Number(inserted as f64))
        }),
    })
}

fn style_sheet_delete_rule(cssom: SharedCssomHost, sheet_index: usize) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "CSSStyleSheet.deleteRule".to_owned(),
        function: Rc::new(move |_vm, _this, args| {
            let index = args
                .first()
                .map_or(0, |value| value.to_number().max(0.0) as usize);
            cssom.delete_rule(sheet_index, index);
            Ok(JsValue::Undefined)
        }),
    })
}

fn create_css_rule_list_object(cssom: SharedCssomHost, sheet_index: usize) -> JsValue {
    JsValue::host_object(
        Rc::new(CssRuleListHost { cssom, sheet_index }),
        "[object CSSRuleList]",
    )
}

#[derive(Clone)]
struct CssRuleListHost {
    cssom: SharedCssomHost,
    sheet_index: usize,
}

impl JsHostObject for CssRuleListHost {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        let sheet = self.cssom.stylesheet(self.sheet_index);

        if key == "length" {
            return Some(JsValue::Number(sheet.rules.len() as f64));
        }

        if let Ok(index) = key.parse::<usize>() {
            return sheet.rules.get(index).map(create_css_rule_object);
        }

        None
    }

    fn set_property(&self, _key: &str, _value: JsValue) -> bool {
        false
    }
}

fn create_css_rule_object(rule: &CssRuleRecord) -> JsValue {
    let object = JsValue::Object(Rc::new(RefCell::new(JsObject::new(JsObjectKind::Host))));
    object.set_property("selectorText", JsValue::String(rule.selector.clone()));
    object.set_property("cssText", JsValue::String(rule.css_text.clone()));
    object.set_property("style", create_rule_style_object(rule.clone()));
    object
}

fn create_rule_style_object(rule: CssRuleRecord) -> JsValue {
    JsValue::host_object(
        Rc::new(RuleStyleHost {
            declarations: RefCell::new(rule.declarations),
        }),
        "[object CSSStyleDeclaration]",
    )
}

#[derive(Clone)]
struct RuleStyleHost {
    declarations: RefCell<BTreeMap<String, String>>,
}

impl JsHostObject for RuleStyleHost {
    fn get_property(&self, key: &str) -> Option<JsValue> {
        let key = normalize_css_property(key);

        if key == "css-text" {
            return Some(JsValue::String(serialize_style_declarations(
                &self.declarations.borrow(),
            )));
        }

        Some(JsValue::String(
            self.declarations
                .borrow()
                .get(&key)
                .cloned()
                .unwrap_or_default(),
        ))
    }

    fn set_property(&self, key: &str, value: JsValue) -> bool {
        self.declarations
            .borrow_mut()
            .insert(normalize_css_property(key), value.to_js_string());
        true
    }
}

fn create_cssom_metrics_function(cssom: SharedCssomHost) -> JsValue {
    JsValue::function(JsFunction::Native {
        name: "__sylphosCssomMetrics".to_owned(),
        function: Rc::new(move |_vm, _this, _args| {
            let metrics = cssom.metrics();
            let object = JsValue::object();
            object.set_property("inlineReads", JsValue::Number(metrics.inline_reads as f64));
            object.set_property(
                "inlineWrites",
                JsValue::Number(metrics.inline_writes as f64),
            );
            object.set_property(
                "computedReads",
                JsValue::Number(metrics.computed_reads as f64),
            );
            object.set_property(
                "rulesInserted",
                JsValue::Number(metrics.rules_inserted as f64),
            );
            object.set_property(
                "rulesDeleted",
                JsValue::Number(metrics.rules_deleted as f64),
            );
            object.set_property(
                "invalidations",
                JsValue::Number(metrics.invalidations as f64),
            );
            Ok(object)
        }),
    })
}

/// Deterministic research CSSOM host.
#[derive(Debug, Default)]
pub struct ResearchCssomHost {
    inline_styles: RefCell<BTreeMap<DomNodeRef, BTreeMap<String, String>>>,
    sheets: RefCell<Vec<CssStyleSheetRecord>>,
    next_rule_id: RefCell<u64>,
    mutations: RefCell<Vec<CssStyleMutation>>,
    metrics: RefCell<CssomMetrics>,
}

impl ResearchCssomHost {
    /// Creates host with one empty author stylesheet.
    #[must_use]
    pub fn new() -> Self {
        let host = Self::default();
        host.ensure_sheet(0);
        host
    }

    fn ensure_sheet(&self, index: usize) {
        let mut sheets = self.sheets.borrow_mut();
        while sheets.len() <= index {
            let id = sheets.len() as u64;
            sheets.push(CssStyleSheetRecord {
                id,
                rules: Vec::new(),
            });
        }
    }

    fn record_invalidation(
        &self,
        node: Option<DomNodeRef>,
        property: impl Into<String>,
        value: Option<String>,
    ) {
        let property = property.into();
        let invalidation = invalidation_for_property(&property);
        self.mutations.borrow_mut().push(CssStyleMutation {
            node,
            property,
            value,
            invalidation,
        });
        self.bump_metrics(|metrics| {
            metrics.invalidations = metrics.invalidations.saturating_add(1);
        });
    }

    fn bump_metrics(&self, update: impl FnOnce(&mut CssomMetrics)) {
        update(&mut self.metrics.borrow_mut());
    }
}

impl CssomHost for ResearchCssomHost {
    fn inline_style_get(&self, node: DomNodeRef, property: &str) -> Option<String> {
        self.bump_metrics(|metrics| {
            metrics.inline_reads = metrics.inline_reads.saturating_add(1);
        });
        self.inline_styles
            .borrow()
            .get(&node)
            .and_then(|style| style.get(&normalize_css_property(property)).cloned())
    }

    fn inline_style_set(&self, node: DomNodeRef, property: &str, value: String) {
        let property = normalize_css_property(property);
        self.inline_styles
            .borrow_mut()
            .entry(node)
            .or_default()
            .insert(property.clone(), value.clone());
        self.bump_metrics(|metrics| {
            metrics.inline_writes = metrics.inline_writes.saturating_add(1);
        });
        self.record_invalidation(Some(node), property, Some(value));
    }

    fn inline_style_remove(&self, node: DomNodeRef, property: &str) {
        let property = normalize_css_property(property);
        if let Some(style) = self.inline_styles.borrow_mut().get_mut(&node) {
            style.remove(&property);
        }
        self.bump_metrics(|metrics| {
            metrics.inline_removals = metrics.inline_removals.saturating_add(1);
        });
        self.record_invalidation(Some(node), property, None);
    }

    fn inline_style_css_text(&self, node: DomNodeRef) -> String {
        self.bump_metrics(|metrics| {
            metrics.inline_reads = metrics.inline_reads.saturating_add(1);
        });
        let styles = self.inline_styles.borrow();
        styles
            .get(&node)
            .map(serialize_style_declarations)
            .unwrap_or_default()
    }

    fn inline_style_set_css_text(&self, node: DomNodeRef, css_text: String) {
        let declarations = parse_style_declarations(&css_text);
        self.inline_styles.borrow_mut().insert(node, declarations);
        self.bump_metrics(|metrics| {
            metrics.inline_writes = metrics.inline_writes.saturating_add(1);
        });
        self.record_invalidation(Some(node), "cssText", Some(css_text));
    }

    fn computed_style_get(
        &self,
        dom: &dyn DomHost,
        node: DomNodeRef,
        property: &str,
    ) -> Option<String> {
        self.bump_metrics(|metrics| {
            metrics.computed_reads = metrics.computed_reads.saturating_add(1);
        });

        let property = normalize_css_property(property);

        if let Some(value) = self.inline_style_get(node, &property) {
            return Some(value);
        }

        let snapshot = dom.node_snapshot(node)?;

        // Very small cascade: later matching rule wins, inline already won above.
        let mut computed = BTreeMap::new();

        for sheet in self.sheets.borrow().iter() {
            for rule in &sheet.rules {
                if selector_matches_snapshot(&snapshot, &rule.selector) {
                    for (name, value) in &rule.declarations {
                        computed.insert(name.clone(), value.clone());
                    }
                }
            }
        }

        computed
            .get(&property)
            .cloned()
            .or_else(|| default_computed_value(&property))
    }

    fn insert_rule(&self, sheet_index: usize, rule: &str, index: Option<usize>) -> usize {
        self.ensure_sheet(sheet_index);

        let parsed = parse_rule(rule).unwrap_or_else(|| CssRuleRecord {
            id: 0,
            selector: String::new(),
            declarations: BTreeMap::new(),
            css_text: rule.to_owned(),
        });

        let mut sheets = self.sheets.borrow_mut();
        let sheet = &mut sheets[sheet_index];
        let insert_at = index.unwrap_or(sheet.rules.len()).min(sheet.rules.len());

        let mut parsed = parsed;
        {
            let mut next_rule_id = self.next_rule_id.borrow_mut();
            parsed.id = *next_rule_id;
            *next_rule_id = next_rule_id.saturating_add(1);
        }

        sheet.rules.insert(insert_at, parsed);

        self.bump_metrics(|metrics| {
            metrics.rules_inserted = metrics.rules_inserted.saturating_add(1);
        });
        self.record_invalidation(None, "insertRule", Some(rule.to_owned()));

        insert_at
    }

    fn delete_rule(&self, sheet_index: usize, index: usize) -> bool {
        self.ensure_sheet(sheet_index);
        let mut sheets = self.sheets.borrow_mut();
        let Some(sheet) = sheets.get_mut(sheet_index) else {
            return false;
        };

        if index < sheet.rules.len() {
            sheet.rules.remove(index);
            self.bump_metrics(|metrics| {
                metrics.rules_deleted = metrics.rules_deleted.saturating_add(1);
            });
            self.record_invalidation(None, "deleteRule", None);
            true
        } else {
            false
        }
    }

    fn stylesheet(&self, sheet_index: usize) -> CssStyleSheetRecord {
        self.ensure_sheet(sheet_index);
        self.bump_metrics(|metrics| {
            metrics.stylesheet_reads = metrics.stylesheet_reads.saturating_add(1);
        });
        self.sheets.borrow()[sheet_index].clone()
    }

    fn stylesheet_count(&self) -> usize {
        self.ensure_sheet(0);
        self.sheets.borrow().len()
    }

    fn metrics(&self) -> CssomMetrics {
        self.metrics.borrow().clone()
    }

    fn mutations(&self) -> Vec<CssStyleMutation> {
        self.mutations.borrow().clone()
    }
}

fn parse_rule(rule: &str) -> Option<CssRuleRecord> {
    let (selector, rest) = rule.split_once('{')?;
    let declarations = rest.rsplit_once('}')?.0;
    Some(CssRuleRecord {
        id: 0,
        selector: selector.trim().to_owned(),
        declarations: parse_style_declarations(declarations),
        css_text: rule.trim().to_owned(),
    })
}

fn parse_style_declarations(css_text: &str) -> BTreeMap<String, String> {
    css_text
        .split(';')
        .filter_map(|declaration| {
            let (name, value) = declaration.split_once(':')?;
            let name = normalize_css_property(name);
            let value = value.trim().to_owned();
            (!name.is_empty()).then_some((name, value))
        })
        .collect()
}

fn serialize_style_declarations(declarations: &BTreeMap<String, String>) -> String {
    declarations
        .iter()
        .map(|(name, value)| format!("{name}: {value};"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_css_property(name: &str) -> String {
    let mut out = String::new();

    for (index, ch) in name.trim().chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 {
                out.push('-');
            }
            out.push(ch.to_ascii_lowercase());
        } else if ch == '_' {
            out.push('-');
        } else {
            out.push(ch);
        }
    }

    out
}

fn invalidation_for_property(property: &str) -> StyleInvalidationKind {
    match normalize_css_property(property).as_str() {
        "width" | "height" | "min-width" | "max-width" | "min-height" | "max-height" | "margin"
        | "margin-top" | "margin-right" | "margin-bottom" | "margin-left" | "padding"
        | "padding-top" | "padding-right" | "padding-bottom" | "padding-left" | "border"
        | "border-width" | "font-size" | "display" | "position" | "top" | "right" | "bottom"
        | "left" => StyleInvalidationKind::Layout,
        "class" | "id" | "insertRule" | "deleteRule" => StyleInvalidationKind::StyleRecalc,
        _ => StyleInvalidationKind::Paint,
    }
}

fn default_computed_value(property: &str) -> Option<String> {
    Some(
        match property {
            "display" => "block",
            "position" => "static",
            "color" => "rgb(0, 0, 0)",
            "background-color" | "background" => "transparent",
            "font-size" => "16px",
            "margin" | "padding" | "border-width" => "0px",
            _ => return None,
        }
        .to_owned(),
    )
}

fn selector_matches_snapshot(snapshot: &crate::DomNodeSnapshot, selector: &str) -> bool {
    let selector = selector.trim();

    if selector.is_empty() {
        return false;
    }

    if let Some(id) = selector.strip_prefix('#') {
        return snapshot.attributes.get("id").map(String::as_str) == Some(id);
    }

    if let Some(class) = selector.strip_prefix('.') {
        return snapshot
            .attributes
            .get("class")
            .is_some_and(|value| value.split_whitespace().any(|entry| entry == class));
    }

    if let Some((tag, class)) = selector.split_once('.') {
        return snapshot.tag_name.eq_ignore_ascii_case(tag)
            && snapshot
                .attributes
                .get("class")
                .is_some_and(|value| value.split_whitespace().any(|entry| entry == class));
    }

    if let Some((tag, id)) = selector.split_once('#') {
        return snapshot.tag_name.eq_ignore_ascii_case(tag)
            && snapshot.attributes.get("id").map(String::as_str) == Some(id);
    }

    snapshot.tag_name.eq_ignore_ascii_case(selector)
}
