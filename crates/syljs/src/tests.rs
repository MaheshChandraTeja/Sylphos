use std::rc::Rc;

use crate::ast::{ExprKind, Literal, StmtKind};
use crate::{
    compile_program, eval_script_with_research_cssom, eval_script_with_research_dom,
    install_cssom_globals, install_dom_globals, install_web_api_globals, AstStats, CompileOptions,
    CssomHost, DomHost, EventLoopConfig, Keyword, Lexer, ResearchCssomHost, ResearchDom,
    ResearchWebApiHost, ScheduledVm, TokenKind, VmConfig, WebApiHost, WebApiResponse,
};

#[test]
fn lexer_tokenizes_dom_mutation_script() {
    let source = r#"
        const title = "Changed";
        document.querySelector("h1").textContent = title;
    "#;

    let tokens = Lexer::new(source).tokenize().expect("tokenize");

    assert!(tokens
        .iter()
        .any(|token| matches!(token.kind, TokenKind::Keyword(Keyword::Const))));
    assert!(tokens.iter().any(
        |token| matches!(token.kind, TokenKind::Identifier(ref value) if value == "document")
    ));
    assert!(tokens
        .iter()
        .any(|token| matches!(token.kind, TokenKind::String(ref value) if value == "h1")));
}

#[test]
fn parser_preserves_binary_precedence() {
    let program = crate::parse_script("let x = 1 + 2 * 3;").expect("parse");

    let StmtKind::VarDecl(decl) = &program.body[0].kind else {
        panic!("expected var decl");
    };
    let Some(init) = &decl.declarations[0].init else {
        panic!("expected initializer");
    };

    match &init.kind {
        ExprKind::Binary { op, left, right } => {
            assert_eq!(*op, crate::ast::BinaryOp::Add);
            assert!(matches!(left.kind, ExprKind::Literal(Literal::Number(1.0))));
            assert!(matches!(
                right.kind,
                ExprKind::Binary {
                    op: crate::ast::BinaryOp::Mul,
                    ..
                }
            ));
        }
        other => panic!("expected binary expression, got {other:?}"),
    }
}

#[test]
fn parser_parses_function_and_control_flow() {
    let program = crate::parse_script(
        r##"
        function update(title) {
            if (title) {
                document.title = title;
            } else {
                document.title = "Fallback";
            }
            return document.title;
        }
        "##,
    )
    .expect("parse");

    assert_eq!(program.body.len(), 1);
    let stats = AstStats::collect(&program);

    assert_eq!(stats.functions, 1);
    assert!(stats.assignments >= 2);
    assert!(stats.member_accesses >= 3);
}

#[test]
fn vm_executes_for_loop() {
    let program = crate::parse_script(
        r##"
        let total = 0;
        for (let i = 0; i < 4; i = i + 1) {
            total = total + i;
        }
        console.log(total);
        "##,
    )
    .expect("parse");

    let bytecode = compile_program(&program, CompileOptions::default()).expect("compile");
    let mut vm = crate::Vm::default();
    let outcome = vm.execute(&bytecode).expect("execute");

    assert_eq!(outcome.console, vec!["6"]);
}

#[test]
fn event_loop_supports_promise_resolve_then() {
    let summary = crate::eval_script_with_event_loop(
        r##"
        Promise.resolve("done").then(function (value) {
            console.log(value);
        });
        console.log("sync");
        "##,
    )
    .expect("execute");

    assert_eq!(summary.console, vec!["sync", "done"]);
    assert_eq!(summary.event_loop.promise_reactions_executed, 1);
}

#[test]
fn dom_bindings_create_append_query_and_mutate_elements() {
    let (summary, dom) = eval_script_with_research_dom(
        r##"
        const h1 = document.createElement("h1");
        h1.id = "hero";
        h1.className = "title";
        h1.textContent = "Old";
        document.body.appendChild(h1);

        document.querySelector("#hero").textContent = "New";
        document.querySelector(".title").classList.add("active");

        console.log(document.getElementById("hero").textContent);
        console.log(document.querySelector("h1").className);
        "##,
    )
    .expect("execute");

    assert_eq!(summary.console, vec!["New", "active title"]);
    assert!(dom.get_element_by_id("hero").is_some());
    assert!(dom.metrics().nodes_created >= 1);
    assert!(dom.metrics().text_mutations >= 2);
    assert!(dom.metrics().attribute_mutations >= 3);
}

#[test]
fn webapi_fetch_returns_response_text_promise() {
    let host = Rc::new(ResearchWebApiHost::new("https://sylphos.local/page"));
    host.register_route(
        "/api/message",
        WebApiResponse::text("https://sylphos.local/api/message", "hello"),
    );

    let mut scheduled = ScheduledVm::default();
    install_web_api_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        host.clone(),
    );

    scheduled
        .execute_script(
            r#"
            fetch("/api/message")
                .then(function (response) {
                    response.text().then(function (text) {
                        console.log(text);
                    });
                });
            "#,
        )
        .expect("execute");

    let summary = scheduled.run_until_idle().expect("drain");

    assert_eq!(summary.console, vec!["hello"]);
    assert_eq!(host.metrics().fetch_calls, 1);
    assert_eq!(host.fetch_records().len(), 1);
}

#[test]
fn cssom_supports_element_style_property_assignment() {
    let (summary, dom, cssom) = eval_script_with_research_cssom(
        r##"
        const box = document.createElement("div");
        box.id = "box";
        document.body.appendChild(box);

        box.style.backgroundColor = "#112233";
        box.style.width = "320px";
        box.style.setProperty("margin-top", "12px");

        console.log(box.style.backgroundColor);
        console.log(box.style.getPropertyValue("width"));
        console.log(box.style.cssText);
        "##,
    )
    .expect("execute");

    assert_eq!(summary.console[0], "#112233");
    assert_eq!(summary.console[1], "320px");
    assert!(summary.console[2].contains("background-color: #112233"));
    assert!(summary.console[2].contains("width: 320px"));
    assert!(dom.get_element_by_id("box").is_some());
    assert!(cssom.metrics().inline_writes >= 3);
    assert!(cssom.metrics().invalidations >= 3);
}

#[test]
fn cssom_supports_get_computed_style_and_stylesheet_rules() {
    let (summary, _dom, cssom) = eval_script_with_research_cssom(
        r#"
        const title = document.createElement("h1");
        title.id = "hero";
        title.className = "headline";
        document.body.appendChild(title);

        document.styleSheets[0].insertRule(".headline { color: red; font-size: 24px; }", 0);

        console.log(getComputedStyle(title).color);
        console.log(getComputedStyle(title).fontSize);
        console.log(document.styleSheets[0].cssRules.length);
        "#,
    )
    .expect("execute");

    assert_eq!(summary.console, vec!["red", "24px", "1"]);
    assert_eq!(cssom.metrics().rules_inserted, 1);
    assert!(cssom.metrics().computed_reads >= 2);
}

#[test]
fn cssom_supports_remove_property_and_delete_rule() {
    let (summary, _dom, cssom) = eval_script_with_research_cssom(
        r#"
        const p = document.createElement("p");
        document.body.appendChild(p);
        p.style.color = "blue";
        console.log(p.style.removeProperty("color"));
        console.log(p.style.color);

        document.styleSheets[0].insertRule("p { color: green; }", 0);
        console.log(document.styleSheets[0].cssRules.length);
        document.styleSheets[0].deleteRule(0);
        console.log(document.styleSheets[0].cssRules.length);
        "#,
    )
    .expect("execute");

    assert_eq!(summary.console, vec!["blue", "", "1", "0"]);
    assert_eq!(cssom.metrics().rules_inserted, 1);
    assert_eq!(cssom.metrics().rules_deleted, 1);
    assert!(cssom.metrics().inline_removals >= 1);
}

#[test]
fn cssom_can_be_installed_with_dom_and_webapi() {
    let cssom = Rc::new(ResearchCssomHost::new());
    let dom = Rc::new(ResearchDom::with_cssom("Combined", cssom.clone()));
    let web = Rc::new(ResearchWebApiHost::default());
    let mut scheduled = ScheduledVm::with_config(VmConfig::default(), EventLoopConfig::default());

    install_dom_globals(&mut scheduled.vm, scheduled.event_loop.clone(), dom.clone());
    install_cssom_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        dom.clone(),
        cssom.clone(),
    );
    install_web_api_globals(&mut scheduled.vm, scheduled.event_loop.clone(), web.clone());

    scheduled
        .execute_script(
            r#"
            const card = document.createElement("div");
            card.id = "card";
            document.body.appendChild(card);
            card.style.color = "purple";
            localStorage.setItem("cardColor", getComputedStyle(card).color);
            console.log(localStorage.getItem("cardColor"));
            "#,
        )
        .expect("execute");

    let summary = scheduled.run_until_idle().expect("drain");

    assert_eq!(summary.console, vec!["purple"]);
    assert_eq!(web.metrics().storage_writes, 1);
    assert!(cssom.metrics().inline_writes >= 1);
}
