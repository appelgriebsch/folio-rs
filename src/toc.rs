use std::collections::HashSet;

use dom_query::Document;

use crate::slug::slug_from_title;

pub struct Section {
    pub label: String,
    pub id: String,
    pub children: Vec<Section>,
}

/// Give each h1–h3 a stable id and return a table of contents at most two levels deep.
///
/// A single h1 is the chapter title, so the two levels under it are h2 and h3.
/// Several h1 headings use h1 and h2, and h3 is left out so the list stays two deep.
pub fn anchor_outline(html: &str) -> (String, Vec<Section>) {
    let doc = Document::fragment(html);
    let headings: Vec<_> = doc.select("h1, h2, h3").nodes().to_vec();
    let heading_ids: HashSet<_> = headings.iter().map(|heading| heading.id).collect();
    let mut seen = HashSet::new();
    for node in doc.select("[id]").nodes() {
        if heading_ids.contains(&node.id) {
            continue;
        }
        if let Some(id) = node.attr("id") {
            let id = id.trim();
            if is_xml_id(id) {
                seen.insert(id.to_string());
            }
        }
    }
    let mut marks = Vec::new();
    for heading in &headings {
        let label = flat_label(&heading.text());
        if label.is_empty() {
            continue;
        }
        let level = match heading.node_name().as_deref() {
            Some("h1") => 1,
            Some("h2") => 2,
            Some("h3") => 3,
            _ => continue,
        };
        let current = heading.attr("id").map(|id| id.trim().to_string());
        let id = match current {
            Some(id) if is_xml_id(&id) && seen.insert(id.clone()) => id,
            _ => {
                let fresh = unique_id(&label, &mut seen);
                heading.set_attr("id", &fresh);
                fresh
            }
        };
        marks.push(Mark { level, id, label });
    }
    let outline = outline_from(&marks);
    (doc.tree.root().inner_html().to_string(), outline)
}

struct Mark {
    level: u8,
    id: String,
    label: String,
}

fn outline_from(marks: &[Mark]) -> Vec<Section> {
    let (top, child) = ranks(marks);
    if top == 0 {
        return Vec::new();
    }
    let mut sections = Vec::new();
    for mark in marks {
        if mark.level == top {
            sections.push(section_from(mark));
        } else if Some(mark.level) == child {
            if let Some(parent) = sections.last_mut() {
                parent.children.push(section_from(mark));
            } else {
                sections.push(section_from(mark));
            }
        }
    }
    sections
}

fn section_from(mark: &Mark) -> Section {
    Section {
        label: mark.label.clone(),
        id: mark.id.clone(),
        children: Vec::new(),
    }
}

/// First and second heading ranks to show. `0` means there is nothing to list.
fn ranks(marks: &[Mark]) -> (u8, Option<u8>) {
    let mut present = [false; 4];
    let mut h1s = 0usize;
    for mark in marks {
        if (1..=3).contains(&mark.level) {
            present[mark.level as usize] = true;
        }
        if mark.level == 1 {
            h1s += 1;
        }
    }
    let mut order: Vec<u8> = (1..=3).filter(|level| present[*level as usize]).collect();
    if h1s == 1 && order.len() > 1 && order[0] == 1 {
        order.remove(0);
    }
    let top = order.first().copied().unwrap_or(0);
    let child = order.get(1).copied();
    (top, child)
}

fn flat_label(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn unique_id(label: &str, seen: &mut HashSet<String>) -> String {
    let mut base = slug_from_title(label).unwrap_or_else(|| "section".to_string());
    if base.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        base = format!("s-{base}");
    }
    if !is_xml_id(&base) {
        base = "section".to_string();
    }
    if seen.insert(base.clone()) {
        return base;
    }
    for number in 2.. {
        let candidate = format!("{base}-{number}");
        if seen.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!("a free heading id exists")
}

fn is_xml_id(id: &str) -> bool {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|ch| ch.is_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_title_uses_h2_and_h3() {
        let html = r#"
            <h1>Tahoe</h1>
            <h2>Installer</h2>
            <h3>Free space</h3>
            <h3>FileVault</h3>
            <h2>Spotlight</h2>
            <h4>Ignored</h4>
        "#;
        let (anchored, outline) = anchor_outline(html);
        assert!(anchored.contains("id=\"installer\""), "{anchored}");
        assert!(anchored.contains("id=\"free-space\""), "{anchored}");
        assert_eq!(outline.len(), 2);
        assert_eq!(outline[0].label, "Installer");
        assert_eq!(outline[0].children.len(), 2);
        assert_eq!(outline[0].children[0].label, "Free space");
        assert_eq!(outline[1].label, "Spotlight");
        assert!(outline[1].children.is_empty());
        assert!(!outline.iter().any(|item| item.label == "Tahoe"));
        assert!(!outline.iter().any(|item| item.label == "Ignored"));
    }

    #[test]
    fn several_h1_headings_stop_at_h2() {
        let html = r#"
            <h1 id="one">One</h1>
            <h2>Part</h2>
            <h3>Dropped</h3>
            <h1>Two</h1>
        "#;
        let (anchored, outline) = anchor_outline(html);
        assert!(anchored.contains("id=\"one\""), "{anchored}");
        assert_eq!(outline.len(), 2);
        assert_eq!(outline[0].label, "One");
        assert_eq!(outline[0].id, "one");
        assert_eq!(outline[0].children.len(), 1);
        assert_eq!(outline[0].children[0].label, "Part");
        assert!(outline[0].children[0].children.is_empty());
        assert_eq!(outline[1].label, "Two");
    }

    #[test]
    fn keeps_the_first_id_and_replaces_a_duplicate() {
        let html = r#"<h2 id="same">Alpha</h2><h2 id="same">Beta</h2>"#;
        let (anchored, outline) = anchor_outline(html);
        assert_eq!(outline[0].id, "same");
        assert_ne!(outline[1].id, "same");
        assert!(
            anchored.contains(&format!("id=\"{}\"", outline[1].id)),
            "{anchored}"
        );
    }
}
