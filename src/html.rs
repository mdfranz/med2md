use scraper::{Html, Selector};
use markup5ever::{QualName, Namespace, LocalName};
use crate::util::get_extension;

/// Medium articles render their H1 as either an ATX heading (`# Title`) or a
/// Setext heading (`Title` followed by a line of `===`), depending on the
/// source markup. Both must be recognized here, or articles using the Setext
/// form silently skip title-link injection and fall back to the raw file slug
/// as their title everywhere downstream (dashboard, article page).
pub fn inject_source_link(md: &str, url: &str) -> String {
    let lines: Vec<&str> = md.lines().collect();
    let mut result = String::new();
    let mut injected = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if !injected && line.starts_with("# ") {
            result.push_str(&format!("# [{}]({})\n", &line[2..], url));
            injected = true;
            i += 1;
            continue;
        }
        if !injected && !line.trim().is_empty() {
            if let Some(next) = lines.get(i + 1) {
                let underline = next.trim();
                if !underline.is_empty() && underline.chars().all(|c| c == '=') {
                    result.push_str(&format!("# [{}]({})\n", line.trim(), url));
                    injected = true;
                    i += 2;
                    continue;
                }
            }
        }
        result.push_str(line);
        result.push('\n');
        i += 1;
    }
    if !injected {
        result = format!("[Source]({})\n\n{}", url, result);
    }
    result.trim_end().to_string()
}

/// Retrofit for already-downloaded files written before Setext-heading
/// detection existed: they got the `[Source](url)` fallback prefix instead of
/// a proper `# [Title](url)`, leaving the raw `Title\n====\n` intact right
/// after it. Merge that pattern into the standard injected-title form.
pub fn fix_setext_title_link(md: &str) -> String {
    let lines: Vec<&str> = md.lines().collect();
    if lines.len() < 4 {
        return md.to_string();
    }
    let source_line = lines[0];
    if !source_line.starts_with("[Source](") || !source_line.ends_with(')') {
        return md.to_string();
    }
    if !lines[1].trim().is_empty() {
        return md.to_string();
    }
    let title_line = lines[2].trim();
    if title_line.is_empty() {
        return md.to_string();
    }
    let underline = lines[3].trim();
    if underline.is_empty() || !underline.chars().all(|c| c == '=') {
        return md.to_string();
    }
    let url = &source_line[9..source_line.len() - 1];
    let mut result = format!("# [{}]({})\n", title_line, url);
    result.push_str(&lines[4..].join("\n"));
    result
}

/// Medium wraps external link-preview embeds (e.g. GitHub/repo cards) in a single
/// `<a>` containing block-level children (title, description, site name). html2md
/// converts that into one `[...](url)` link whose bracketed text spans blank-line
/// separated paragraphs, which CommonMark can't parse as a link — renderers show
/// the bracket text as plain paragraphs followed by a stray `](url)`. Flatten any
/// link text that contains a blank line into a single line so it parses correctly.
pub fn fix_broken_card_links(md: &str) -> String {
    let re = regex::Regex::new(r"\[([^\[\]]*)\]\(([^()\s]+)\)").unwrap();
    re.replace_all(md, |caps: &regex::Captures| {
        let text = &caps[1];
        let url = &caps[2];
        if text.contains("\n\n") {
            let flattened = text.split_whitespace().collect::<Vec<_>>().join(" ");
            format!("[{}]({})", flattened, url)
        } else {
            caps[0].to_string()
        }
    }).to_string()
}

pub fn clean_markdown(md_text: &str) -> String {
    let lines = md_text.lines();
    let mut cleaned_lines = Vec::new();
    let mut prev_blank = false;
    for line in lines {
        let is_blank = line.trim().is_empty();
        if is_blank {
            if !prev_blank {
                cleaned_lines.push("");
                prev_blank = true;
            }
        } else {
            cleaned_lines.push(line.trim_end());
            prev_blank = false;
        }
    }
    cleaned_lines.join("\n")
}

pub fn has_key_descendants(node: ego_tree::NodeRef<'_, scraper::Node>) -> bool {
    for child in node.children() {
        if let Some(el) = child.value().as_element() {
            let name = el.name.local.as_ref();
            if ["p", "h1", "h2", "h3", "h4", "h5", "h6", "img", "pre", "ul", "ol"].contains(&name) {
                return true;
            }
        }
        if has_key_descendants(child) {
            return true;
        }
    }
    false
}

pub fn get_text(node: ego_tree::NodeRef<'_, scraper::Node>) -> String {
    let mut text = String::new();
    for child in node.children() {
        if let scraper::Node::Text(ref t) = *child.value() {
            text.push_str(&t.text);
        } else {
            text.push_str(&get_text(child));
        }
    }
    text
}

pub fn clean_article(document: &mut Html) {
    let decompose_selector = Selector::parse("button, svg, style, script").unwrap();
    let ids: Vec<_> = document.select(&decompose_selector).map(|el| el.id()).collect();
    for id in ids {
        if let Some(mut node) = document.tree.get_mut(id) {
            node.detach();
        }
    }

    let a_selector = Selector::parse("a").unwrap();
    let a_ids: Vec<_> = document.select(&a_selector).map(|el| el.id()).collect();
    for id in a_ids {
        let mut detach = false;
        let mut new_href = None;
        let mut should_remove_href = false;

        if let Some(node) = document.tree.get(id) {
            if let Some(element) = node.value().as_element() {
                if let Some(href) = element.attr("href") {
                    let href_lower = href.to_lowercase();
                    if href_lower.contains("signin")
                        || href_lower.contains("signup")
                        || href_lower.contains("plans?dimension")
                        || href_lower.contains("upgrade")
                    {
                        detach = true;
                    } else if let Ok(mut url) = url::Url::parse(href) {
                        let cleaned_query: Vec<(String, String)> = url
                            .query_pairs()
                            .filter(|(k, _)| {
                                !k.starts_with("source") && k != "referrer" && k != "gi"
                            })
                            .map(|(k, v)| (k.into_owned(), v.into_owned()))
                            .collect();
                        url.set_query(None);
                        if !cleaned_query.is_empty() {
                            let mut query_serializer = url.query_pairs_mut();
                            for (k, v) in cleaned_query {
                                query_serializer.append_pair(&k, &v);
                            }
                        }
                        new_href = Some(url.to_string());
                    } else if href.starts_with('/') {
                        if let Ok(mut url) = url::Url::parse(&format!("https://medium.com{}", href)) {
                            let cleaned_query: Vec<(String, String)> = url
                                .query_pairs()
                                .filter(|(k, _)| {
                                    !k.starts_with("source") && k != "referrer" && k != "gi"
                                })
                                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                                .collect();
                            url.set_query(None);
                            if !cleaned_query.is_empty() {
                                let mut query_serializer = url.query_pairs_mut();
                                for (k, v) in cleaned_query {
                                    query_serializer.append_pair(&k, &v);
                                }
                            }
                            new_href = Some(url.path().to_string() + url.query().map(|q| format!("?{}", q)).as_deref().unwrap_or(""));
                        }
                    }

                    if !detach {
                        let check_href = new_href.as_deref().unwrap_or(href);
                        if check_href.is_empty()
                            || check_href == "/"
                            || check_href == "javascript:void(0)"
                            || check_href.starts_with('?')
                        {
                            should_remove_href = true;
                        }
                    }
                }
            }
        }

        if detach {
            if let Some(mut node) = document.tree.get_mut(id) {
                node.detach();
            }
            continue;
        }

        if let Some(mut node) = document.tree.get_mut(id) {
            if let scraper::Node::Element(ref mut element) = *node.value() {
                if should_remove_href {
                    let mut keys = Vec::new();
                    for k in element.attrs.keys() {
                        if k.local.as_ref() == "href" {
                            keys.push(k.clone());
                        }
                    }
                    for k in keys {
                        element.attrs.remove(&k);
                    }
                } else if let Some(href_val) = new_href {
                    for (k, v) in &mut element.attrs {
                        if k.local.as_ref() == "href" {
                            *v = href_val.clone().into();
                        }
                    }
                }
            }
        }
    }

    let img_selector = Selector::parse("img").unwrap();
    let img_ids: Vec<_> = document.select(&img_selector).map(|el| el.id()).collect();
    for id in img_ids {
        let mut detach = false;
        if let Some(node) = document.tree.get(id) {
            if let Some(element) = node.value().as_element() {
                if let Some(src) = element.attr("src") {
                    if src.contains("resize:fill:64:64")
                        || src.contains("resize:fill:32:32")
                        || src.contains("resize:fill:40:40")
                        || src.contains("resize:fill:48:48")
                    {
                        detach = true;
                    }
                }
            }
        }
        if detach {
            if let Some(mut node) = document.tree.get_mut(id) {
                node.detach();
            }
        }
    }

    let text_container_selector = Selector::parse("p, span, div, a").unwrap();
    let tc_ids: Vec<_> = document.select(&text_container_selector).map(|el| el.id()).collect();

    let target_texts = [
        "member-only story",
        "listen",
        "share",
        "follow",
        "mute",
        "--",
        "·",
        "read",
        "press enter or click to view image in full size",
    ];

    for id in tc_ids {
        let mut detach = false;
        if let Some(node) = document.tree.get(id) {
            let has_key_elements = if let Some(el) = node.value().as_element() {
                if el.name.local.as_ref() == "div" {
                    has_key_descendants(node)
                } else {
                    false
                }
            } else {
                false
            };

            if !has_key_elements {
                let text = get_text(node).trim().to_lowercase();
                if target_texts.contains(&text.as_str())
                    || (text.len() == 1 && (text == "·" || text == "-" || text == "—"))
                {
                    detach = true;
                } else if text.ends_with("min read") || text.contains("min read") {
                    if let Some(el) = node.value().as_element() {
                        let name = el.name.local.as_ref();
                        if name == "span" || name == "p" || name == "div" {
                            detach = true;
                        }
                    }
                }
            }
        }

        if detach {
            if let Some(mut node) = document.tree.get_mut(id) {
                node.detach();
            }
        }
    }
}

pub fn clean_article_and_collect_images(
    document: &mut Html,
    images_dir_name: &str,
) -> Result<Vec<(String, String)>, String> {
    let picture_selector = Selector::parse("picture").unwrap();
    let source_selector = Selector::parse("source").unwrap();
    let img_selector = Selector::parse("img").unwrap();

    let pic_ids: Vec<_> = document.select(&picture_selector).map(|el| el.id()).collect();

    for pic_id in pic_ids {
        let mut image_url = None;
        if let Some(pic_node) = document.tree.get(pic_id) {
            let pic_ref = scraper::ElementRef::wrap(pic_node).unwrap();
            for src_ref in pic_ref.select(&source_selector) {
                if let Some(srcset) = src_ref.value().attr("srcset").or_else(|| src_ref.value().attr("srcSet")) {
                    if let Some(last_src) = srcset.split(',').last() {
                        let url_part = last_src.trim().split(' ').next().unwrap_or("");
                        if !url_part.is_empty() {
                            image_url = Some(url_part.to_string());
                            break;
                        }
                    }
                }
            }
        }

        if let Some(url) = image_url {
            let mut img_ids = Vec::new();
            if let Some(pic_node) = document.tree.get(pic_id) {
                let pic_ref = scraper::ElementRef::wrap(pic_node).unwrap();
                for img_ref in pic_ref.select(&img_selector) {
                    img_ids.push(img_ref.id());
                }
            }
            for img_id in img_ids {
                if let Some(mut img_node) = document.tree.get_mut(img_id) {
                    if let scraper::Node::Element(ref mut element) = *img_node.value() {
                        let src_key = QualName::new(None, Namespace::from(""), LocalName::from("src"));
                        element.attrs.insert(src_key, url.clone().into());
                    }
                }
            }
        }
    }

    let source_ids: Vec<_> = document.select(&source_selector).map(|el| el.id()).collect();
    for id in source_ids {
        if let Some(mut node) = document.tree.get_mut(id) {
            node.detach();
        }
    }

    clean_article(document);

    let mut image_downloads = Vec::new();
    let img_ids: Vec<_> = document.select(&img_selector).map(|el| el.id()).collect();
    let mut img_counter = 0;

    for id in img_ids {
        let mut original_src = None;
        if let Some(node) = document.tree.get(id) {
            if let Some(element) = node.value().as_element() {
                if let Some(src) = element.attr("src") {
                    if src.starts_with("http") {
                        original_src = Some(src.to_string());
                    }
                }
            }
        }

        if let Some(src) = original_src {
            img_counter += 1;
            let ext = get_extension(&src).unwrap_or("jpg");
            let local_filename = format!("img_{}.{}", img_counter, ext);
            let local_relative_path = format!("./{}/{}", images_dir_name, local_filename);

            image_downloads.push((src.clone(), local_relative_path.clone()));

            if let Some(mut node) = document.tree.get_mut(id) {
                if let scraper::Node::Element(ref mut element) = *node.value() {
                    for (k, v) in &mut element.attrs {
                        if k.local.as_ref() == "src" {
                            *v = local_relative_path.clone().into();
                        }
                    }
                }
            }
        }
    }

    Ok(image_downloads)
}
