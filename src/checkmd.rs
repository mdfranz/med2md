use crate::html::{fix_broken_card_links, fix_setext_title_link};

/// A single, independent markdown repair. Add new entries here as future
/// issues are discovered; each fixer receives the current file contents and
/// returns the repaired version (a no-op fixer just returns its input).
struct Fixer {
    name: &'static str,
    apply: fn(&str) -> String,
}

const FIXERS: &[Fixer] = &[
    Fixer { name: "setext-title-link", apply: fix_setext_title_link },
    Fixer { name: "broken-card-links", apply: fix_broken_card_links },
];

const SKIP_FILES: &[&str] = &["README.md", "ARCHITECTURE.md", "PROJECT.md", "PKG.md", "AGENTS.md", "CLAUDE.md"];

pub async fn run_checkmd(output_dir: &str, dry_run: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut entries = tokio::fs::read_dir(output_dir).await?;
    let mut scanned = 0;
    let mut fixed = 0;

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if SKIP_FILES.contains(&filename) {
            continue;
        }

        scanned += 1;
        let original = match tokio::fs::read_to_string(&path).await {
            Ok(content) => content,
            Err(e) => {
                eprintln!("Warning: failed to read {}: {}", filename, e);
                continue;
            }
        };

        let mut content = original.clone();
        let mut applied = Vec::new();
        for fixer in FIXERS {
            let next = (fixer.apply)(&content);
            if next != content {
                applied.push(fixer.name);
                content = next;
            }
        }

        if applied.is_empty() {
            continue;
        }

        fixed += 1;
        if dry_run {
            println!("Would fix {}: {}", filename, applied.join(", "));
        } else {
            tokio::fs::write(&path, &content).await?;
            println!("Fixed {}: {}", filename, applied.join(", "));
        }
    }

    if dry_run {
        println!("\nScanned {} articles, {} would be fixed (dry run, no files written).", scanned, fixed);
    } else {
        println!("\nScanned {} articles, {} fixed.", scanned, fixed);
    }

    Ok(())
}
