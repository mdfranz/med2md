use scraper::{Html, Selector, Node};
use markup5ever::{QualName, Namespace, LocalName};

fn main() {
    let html = r#"
        <html>
            <body>
                <img class="bd lk nl nm" width="700" height="394" loading="eager" role="presentation">
            </body>
        </html>
    "#;

    let mut document = Html::parse_document(html);
    let img_selector = Selector::parse("img").unwrap();
    let ids: Vec<_> = document.select(&img_selector).map(|el| el.id()).collect();

    for id in ids {
        if let Some(mut node) = document.tree.get_mut(id) {
            let node_val: &mut Node = node.value();
            if let Node::Element(ref mut element) = *node_val {
                let key = QualName::new(
                    None,
                    Namespace::from(""),
                    LocalName::from("src")
                );
                element.attrs.insert(key, "https://my-image-url.jpg".into());
            }
        }
    }

    println!("Updated HTML: {}", document.html());
}
