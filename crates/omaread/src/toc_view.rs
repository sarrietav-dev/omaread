//! The table of contents: a panel over the book, a tree you can fold.

use std::rc::Rc;

use gtk::glib::BoxedAnyObject;
use gtk::prelude::*;
use gtk::{gio, glib};
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TocItem {
    pub label: String,
    pub href: Option<String>,
    #[serde(default)]
    pub children: Vec<TocItem>,
}

pub struct TocView {
    pub root: gtk::Box,
    model: gtk::TreeListModel,
    selection: gtk::SingleSelection,
    list: gtk::ListView,
}

fn item_of(obj: &glib::Object) -> Option<TocItem> {
    let row = obj.downcast_ref::<gtk::TreeListRow>()?;
    let item = row.item()?;
    Some(item.downcast_ref::<BoxedAnyObject>()?.borrow::<TocItem>().clone())
}

fn store_of(items: &[TocItem]) -> gio::ListStore {
    let store = gio::ListStore::new::<BoxedAnyObject>();
    let objects: Vec<BoxedAnyObject> = items.iter().cloned().map(BoxedAnyObject::new).collect();
    store.extend_from_slice(&objects);
    store
}

impl TocView {
    pub fn new(on_go: impl Fn(String) + 'static) -> Rc<TocView> {
        let model = gtk::TreeListModel::new(store_of(&[]), false, false, |obj| {
            let item = obj.downcast_ref::<BoxedAnyObject>()?.borrow::<TocItem>().clone();
            (!item.children.is_empty()).then(|| store_of(&item.children).upcast())
        });
        let selection = gtk::SingleSelection::builder().model(&model).autoselect(true).build();

        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
            let label = gtk::Label::builder().xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::End).build();
            let expander = gtk::TreeExpander::builder().child(&label).indent_for_icon(true).build();
            item.set_child(Some(&expander));
        });
        factory.connect_bind(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
            let Some(row) = item.item().and_downcast::<gtk::TreeListRow>() else { return };
            let Some(expander) = item.child().and_downcast::<gtk::TreeExpander>() else { return };
            expander.set_list_row(Some(&row));
            if let (Some(label), Some(toc)) = (expander.child().and_downcast::<gtk::Label>(), item_of(row.upcast_ref())) {
                label.set_label(&toc.label);
                label.set_tooltip_text(Some(&toc.label));
            }
        });

        let list = gtk::ListView::builder().model(&selection).factory(&factory).css_classes(["toc-list"]).build();
        let on_go = Rc::new(on_go);
        list.connect_activate({
            let selection = selection.clone();
            let on_go = on_go.clone();
            move |_, position| {
                if let Some(href) = selection.item(position).as_ref().and_then(item_of).and_then(|t| t.href) {
                    on_go(href);
                }
            }
        });

        let scroller = gtk::ScrolledWindow::builder()
            .child(&list)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let title = gtk::Label::builder().label("Contents").xalign(0.0).css_classes(["title"]).build();
        let hint = gtk::Label::builder()
            .label("j/k move   h/l fold   enter go   t close")
            .xalign(0.0)
            .css_classes(["caption"])
            .build();

        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Fill)
            .visible(false)
            .css_classes(["card", "side-panel"])
            .build();
        root.append(&title);
        root.append(&scroller);
        root.append(&hint);

        Rc::new(TocView { root, model, selection, list })
    }

    pub fn set_items(&self, items: &[TocItem]) {
        if let Ok(store) = self.model.model().downcast::<gio::ListStore>() {
            let objects: Vec<BoxedAnyObject> = items.iter().cloned().map(BoxedAnyObject::new).collect();
            store.splice(0, store.n_items(), &objects);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.model.n_items() == 0
    }

    pub fn is_open(&self) -> bool {
        self.root.is_visible()
    }

    /// Show the panel with the current chapter selected and its parents unfolded.
    pub fn open(&self, current_href: Option<&str>) {
        self.root.set_visible(true);
        if let Some(href) = current_href {
            self.reveal(href);
        }
        self.list.grab_focus();
        let position = self.selection.selected();
        if position != gtk::INVALID_LIST_POSITION {
            self.list.scroll_to(position, gtk::ListScrollFlags::FOCUS | gtk::ListScrollFlags::SELECT, None);
        }
    }

    pub fn close(&self) {
        self.root.set_visible(false);
    }

    fn reveal(&self, href: &str) {
        let mut i = 0;
        while i < self.model.n_items() {
            let Some(row) = self.model.row(i) else { break };
            if let Some(item) = row.item().and_downcast::<BoxedAnyObject>() {
                let item = item.borrow::<TocItem>();
                if item.href.as_deref() == Some(href) {
                    self.selection.set_selected(i);
                    return;
                }
                // Unfold the way down to it.
                if contains(&item.children, href) {
                    row.set_expanded(true);
                }
            }
            i += 1;
        }
    }

    pub fn move_cursor(&self, delta: i32) {
        let n = self.selection.n_items() as i32;
        if n == 0 {
            return;
        }
        let current = match self.selection.selected() {
            gtk::INVALID_LIST_POSITION => 0,
            p => p as i32,
        };
        let next = (current + delta).clamp(0, n - 1) as u32;
        self.list.scroll_to(next, gtk::ListScrollFlags::FOCUS | gtk::ListScrollFlags::SELECT, None);
    }

    /// `l` unfolds, `h` folds (or steps to the parent).
    pub fn fold(&self, open: bool) {
        let Some(row) = self.selection.selected_item().and_downcast::<gtk::TreeListRow>() else { return };
        if open {
            row.set_expanded(true);
        } else if row.is_expanded() {
            row.set_expanded(false);
        } else if let Some(parent) = row.parent() {
            parent.set_expanded(false);
            let position = parent.position();
            self.list.scroll_to(position, gtk::ListScrollFlags::FOCUS | gtk::ListScrollFlags::SELECT, None);
        }
    }

    pub fn activate(&self) {
        let position = self.selection.selected();
        if position != gtk::INVALID_LIST_POSITION {
            self.list.emit_by_name::<()>("activate", &[&position]);
        }
    }
}

fn contains(items: &[TocItem], href: &str) -> bool {
    items.iter().any(|i| i.href.as_deref() == Some(href) || contains(&i.children, href))
}

/// The href of chapter `n`: the top-level entry numbered `n` ("3. …",
/// "Chapter 3", "Capítulo 3"), or else the n-th entry.
pub fn chapter_href(items: &[TocItem], n: usize) -> Option<String> {
    let numbered = items.iter().find(|item| label_number(&item.label) == Some(n));
    numbered.or_else(|| items.get(n.checked_sub(1)?)).and_then(|i| i.href.clone())
}

fn label_number(label: &str) -> Option<usize> {
    let lower = label.trim().to_lowercase();
    let rest = ["chapter ", "capítulo ", "capitulo ", "part ", "parte "]
        .iter()
        .find_map(|p| lower.strip_prefix(p))
        .unwrap_or(&lower);
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let after = rest[digits.len()..].chars().next();
    // "3. Title", "3 Title", "Chapter 3" — but not "1984" or "3D Negotiation".
    let ends_cleanly = matches!(after, None | Some('.' | ':' | ' ' | ')' | '-' | '—'));
    if digits.is_empty() || !ends_cleanly { None } else { digits.parse().ok() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &str, href: &str) -> TocItem {
        TocItem { label: label.into(), href: Some(href.into()), children: vec![] }
    }

    #[test]
    fn chapters_are_found_by_their_number() {
        let toc = vec![item("Preface", "p"), item("1. Intro", "c1"), item("2. Modeling", "c2"), item("3. Blocks", "c3")];
        assert_eq!(chapter_href(&toc, 3).as_deref(), Some("c3"));
        let toc = vec![item("Cover", "x"), item("Chapter 1", "c1"), item("Chapter 2: Rome", "c2")];
        assert_eq!(chapter_href(&toc, 2).as_deref(), Some("c2"));
    }

    #[test]
    fn unnumbered_tocs_fall_back_to_position() {
        let toc = vec![item("Begin", "a"), item("Middle", "b"), item("3D Printing", "c")];
        assert_eq!(chapter_href(&toc, 2).as_deref(), Some("b"));
        assert_eq!(chapter_href(&toc, 3).as_deref(), Some("c"));
        assert_eq!(chapter_href(&toc, 9), None);
    }
}
