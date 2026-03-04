#![deny(unsafe_code)]

pub mod dom;

mod parser;
mod serializer;
mod tokenizer;

#[doc(inline)]
pub use dom::Document;

#[doc(inline)]
pub use dom::Node;

#[doc(inline)]
pub use parser::parse;

#[doc(inline)]
pub use serializer::serialize_document;

pub fn dom_eq(a: &Document, b: &Document) -> bool {
    dom::dom_eq(a, b)
}

#[cfg(test)]
mod prop_tests {
    use super::*;
    use proptest::prelude::*;
    use proptest::strategy::{BoxedStrategy, Strategy};
    use proptest::string::string_regex;

    const TAGS: &[&str] = &["div", "span", "p", "a", "b", "i", "u", "br", "img"];

    fn html_strategy() -> impl Strategy<Value = String> {
        let key: BoxedStrategy<String> = string_regex("[a-z]{1,6}").unwrap().boxed();
        let text: BoxedStrategy<String> = string_regex("\\PC{0,24}").unwrap().boxed();

        let ent_text: BoxedStrategy<String> = prop_oneof![
            Just("&lt;".to_string()),
            Just("&gt;".to_string()),
            Just("&amp;".to_string()),
            Just("&quot;".to_string()),
            Just("&apos;".to_string()),
            any::<String>().prop_map(|s| s.replace('<', "").replace('>', "")),
        ]
        .boxed();

        let attr = (key.clone(), prop_oneof![text.clone(), ent_text.clone()]);

        let attrs: BoxedStrategy<Vec<(String, String)>> = prop::collection::vec(attr, 0..3).boxed();

        fn element(
            depth: u8,
            attrs: BoxedStrategy<Vec<(String, String)>>,
        ) -> BoxedStrategy<String> {
            let tag = prop::sample::select(TAGS.to_vec()).prop_map(|s| s.to_string());

            (tag, attrs.clone())
                .prop_flat_map(move |(t, a)| {
                    let opening = format!("<{}", t);
                    let attr_str = if a.is_empty() {
                        String::new()
                    } else {
                        a.iter()
                            .map(|(k, v)| format!(" {}=\"{}\"", k, v.replace('"', "&quot;")))
                            .collect::<Vec<_>>()
                            .join("")
                    };
                    let start = format!("{opening}{attr_str}>");

                    if ["br", "img"].contains(&t.as_str()) {
                        Just(start).boxed()
                    } else if depth == 0 {
                        let text0 = prop_oneof![
                            Just(String::new()),
                            string_regex("\\PC{0,20}").unwrap(),
                            Just("&lt;&gt;&amp;&quot;&apos;".to_string()),
                        ];
                        text0
                            .prop_map(move |txt| format!("{start}{txt}</{t}>"))
                            .boxed()
                    } else {
                        let child = prop_oneof![
                            Just(String::new()),
                            string_regex("\\PC{0,20}").unwrap(),
                            element(depth - 1, attrs.clone()),
                        ];
                        prop::collection::vec(child, 0..3)
                            .prop_map(move |kids| format!("{start}{}</{t}>", kids.join("")))
                            .boxed()
                    }
                })
                .boxed()
        }

        prop_oneof![
            string_regex("\\PC{0,40}").unwrap(),
            element(2, attrs.clone()),
            prop::collection::vec(element(1, attrs.clone()), 1..3).prop_map(|v| v.join("")),
        ]
    }

    proptest! {
        #[test]
        fn prop_round_trip(src in html_strategy()) {
            let d1 = parse(&src).expect("parse src");
            let html = serialize_document(&d1);
            let d2 = parse(&html).expect("parse normalized");
            prop_assert!(dom_eq(&d1, &d2), "DOM mismatch\nsrc: {}\nser: {}", src, html);
        }
    }
}
