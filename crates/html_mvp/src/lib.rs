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
    use proptest::test_runner::TestCaseError;

    const TAGS: &[&str] = &["div", "span", "p", "a", "b", "i", "u", "br", "img"];

    fn regex_strategy(pattern: &str) -> BoxedStrategy<String> {
        match string_regex(pattern) {
            Ok(strategy) => strategy.boxed(),
            Err(error) => panic!("invalid proptest regex `{pattern}`: {error}"),
        }
    }

    fn html_strategy() -> impl Strategy<Value = String> {
        let key: BoxedStrategy<String> = regex_strategy("[a-z]{1,6}");
        let text: BoxedStrategy<String> = regex_strategy("\\PC{0,24}");

        let entity_text: BoxedStrategy<String> = prop_oneof![
            Just("&lt;".to_owned()),
            Just("&gt;".to_owned()),
            Just("&amp;".to_owned()),
            Just("&quot;".to_owned()),
            Just("&apos;".to_owned()),
            any::<String>().prop_map(|value| value.replace(['<', '>'], "")),
        ]
        .boxed();

        let attr = (key.clone(), prop_oneof![text.clone(), entity_text.clone()]);
        let attrs: BoxedStrategy<Vec<(String, String)>> = prop::collection::vec(attr, 0..3).boxed();

        fn element(
            depth: u8,
            attrs: BoxedStrategy<Vec<(String, String)>>,
        ) -> BoxedStrategy<String> {
            let tag = prop::sample::select(TAGS.to_vec()).prop_map(ToOwned::to_owned);

            (tag, attrs.clone())
                .prop_flat_map(move |(tag_name, attr_values)| {
                    let attr_text = if attr_values.is_empty() {
                        String::new()
                    } else {
                        attr_values
                            .iter()
                            .map(|(key, value)| {
                                format!(" {key}=\"{}\"", value.replace('"', "&quot;"))
                            })
                            .collect::<Vec<_>>()
                            .join("")
                    };

                    let start = format!("<{tag_name}{attr_text}>");

                    if ["br", "img"].contains(&tag_name.as_str()) {
                        return Just(start).boxed();
                    }

                    if depth == 0 {
                        let leaf_text = prop_oneof![
                            Just(String::new()),
                            regex_strategy("\\PC{0,20}"),
                            Just("&lt;&gt;&amp;&quot;&apos;".to_owned()),
                        ];

                        return leaf_text
                            .prop_map(move |inner| format!("{start}{inner}</{tag_name}>"))
                            .boxed();
                    }

                    let child = prop_oneof![
                        Just(String::new()),
                        regex_strategy("\\PC{0,20}"),
                        element(depth - 1, attrs.clone()),
                    ];

                    prop::collection::vec(child, 0..3)
                        .prop_map(move |children| {
                            format!("{start}{}</{tag_name}>", children.join(""))
                        })
                        .boxed()
                })
                .boxed()
        }

        prop_oneof![
            regex_strategy("\\PC{0,40}"),
            element(2, attrs.clone()),
            prop::collection::vec(element(1, attrs), 1..3).prop_map(|items| items.join("")),
        ]
    }

    proptest! {
        #[test]
        fn prop_round_trip(src in html_strategy()) {
            let first = parse(&src)
                .map_err(|error| TestCaseError::fail(format!("parse src failed: {error}; src: {src:?}")))?;

            let serialized = serialize_document(&first);

            let second = parse(&serialized)
                .map_err(|error| TestCaseError::fail(format!("parse serialized failed: {error}; serialized: {serialized:?}")))?;

            prop_assert!(
                dom_eq(&first, &second),
                "DOM mismatch\nsrc: {src}\nserialized: {serialized}"
            );
        }
    }
}
