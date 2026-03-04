async fn fetch_parse_present(url: String, clear: Arc<Mutex<[f32; 4]>>) -> Result<()> {
    fetch::init(Default::default());
    let resp = fetch::get(&url).await?;
    info!(status = %resp.status(), "GET ok");

    let text = resp.get_text().await?;
    info!(bytes = text.len(), "downloaded");

    let doc = html_mvp::parse(&text)?;
    if let Some(content) = find_meta_theme_color(&doc) {
        if let Some(rgb) = parse_css_color_hex(&content) {
            *clear.lock().expect("clear lock") = [rgb.0, rgb.1, rgb.2, 1.0];
            info!(theme_color = %content, "updated clear color from meta");
        }
    }

    let normalized = html_mvp::serialize_document(&doc);
    let preview: String = normalized.chars().take(512).collect();
    info!(preview = %preview, "normalized html (first 512 chars)");
    Ok(())
}


fn find_meta_theme_color(doc: &html_mvp::Document) -> Option<String> {
    use html_mvp::Node;

    fn walk(nodes: &[Node]) -> Option<String> {
        for n in nodes {
            if let Node::Element(el) = n {
                if el.tag.eq_ignore_ascii_case("meta") {
                    let mut name: Option<&str> = None;
                    let mut content: Option<&str> = None;

                    for (k, v) in &el.attrs {
                        if k.eq_ignore_ascii_case("name") {
                            name = Some(v.as_str());
                        } else if k.eq_ignore_ascii_case("content") {
                            content = Some(v.as_str());
                        }
                    }

                    if matches!(name, Some(nm) if nm.eq_ignore_ascii_case("theme-color")) {
                        if let Some(c) = content {
                            return Some(c.trim().to_string());
                        }
                    }
                }

                if let Some(found) = walk(&el.children) {
                    return Some(found);
                }
            }
        }
        None
    }

    walk(&doc.children)
}


fn parse_css_color_hex(s: &str) -> Option<(f32, f32, f32)> {
    let t = s.trim();
    if !t.starts_with('#') {
        return None;
    }
    let hex = &t[1..];
    let (r, g, b) = match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
            (r, g, b)
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            (r, g, b)
        }
        _ => return None,
    };
    Some((r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0))
}
