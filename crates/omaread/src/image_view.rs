//! An image from the book, over the whole page: fit to the window, or
//! zoomed and panned from the keyboard.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};

/// Zoom steps go up and down by this factor.
const STEP: f64 = 1.25;
const MIN_ZOOM: f64 = 0.1;
const MAX_ZOOM: f64 = 8.0;

pub struct ImageView {
    pub root: gtk::Box,
    picture: gtk::Picture,
    scroller: gtk::ScrolledWindow,
    caption: gtk::Label,
    texture: RefCell<Option<gdk::Texture>>,
    /// `None` fits the image to the window; else its scale.
    zoom: Cell<Option<f64>>,
}

impl ImageView {
    pub fn new() -> Rc<ImageView> {
        let picture = gtk::Picture::builder()
            .can_shrink(true)
            .content_fit(gtk::ContentFit::Contain)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        let scroller = gtk::ScrolledWindow::builder().child(&picture).hexpand(true).vexpand(true).build();
        let caption = gtk::Label::builder().xalign(0.0).wrap(true).css_classes(["caption"]).build();
        let hint = gtk::Label::builder()
            .label("+/- zoom   0 fit   1 actual size   h/j/k/l move   y copy   esc close")
            .xalign(0.0)
            .wrap(true)
            .css_classes(["caption"])
            .build();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .visible(false)
            .css_classes(["card", "image-view"])
            .build();
        root.append(&scroller);
        root.append(&caption);
        root.append(&hint);
        Rc::new(ImageView { root, picture, scroller, caption, texture: RefCell::default(), zoom: Cell::new(None) })
    }

    pub fn is_open(&self) -> bool {
        self.root.is_visible()
    }

    /// Show a PNG sent from the page as base64.
    pub fn show(&self, base64: &str, alt: &str) -> Result<(), String> {
        let bytes = glib::base64_decode(base64);
        let texture = gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes)).map_err(|e| format!("can't show this image: {e}"))?;
        self.picture.set_paintable(Some(&texture));
        self.texture.replace(Some(texture));
        let alt = alt.trim();
        self.caption.set_label(alt);
        self.caption.set_visible(!alt.is_empty());
        self.zoom.set(None);
        self.apply_zoom();
        self.root.set_visible(true);
        Ok(())
    }

    pub fn texture(&self) -> Option<gdk::Texture> {
        self.texture.borrow().clone()
    }

    /// The scale the image is shown at now, fitted or not.
    fn current_scale(&self) -> f64 {
        if let Some(z) = self.zoom.get() {
            return z;
        }
        let Some(t) = self.texture() else { return 1.0 };
        let (w, h) = (self.scroller.width().max(1) as f64, self.scroller.height().max(1) as f64);
        (w / t.width().max(1) as f64).min(h / t.height().max(1) as f64).min(1.0)
    }

    /// `+1` in, `-1` out.
    pub fn zoom_by(&self, steps: i32) {
        let z = (self.current_scale() * STEP.powi(steps)).clamp(MIN_ZOOM, MAX_ZOOM);
        self.zoom.set(Some(z));
        self.apply_zoom();
    }

    pub fn fit(&self) {
        self.zoom.set(None);
        self.apply_zoom();
    }

    pub fn actual_size(&self) {
        self.zoom.set(Some(1.0));
        self.apply_zoom();
    }

    fn apply_zoom(&self) {
        let Some(t) = self.texture() else { return };
        match self.zoom.get() {
            None => {
                self.picture.set_can_shrink(true);
                self.picture.set_size_request(-1, -1);
                self.scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Never);
            }
            Some(z) => {
                self.picture.set_can_shrink(true);
                self.picture.set_size_request((t.width() as f64 * z) as i32, (t.height() as f64 * z) as i32);
                self.scroller.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
            }
        }
    }

    /// Move a zoomed image: `dx`/`dy` in steps.
    pub fn pan(&self, dx: i32, dy: i32) {
        for (adj, d) in [(self.scroller.hadjustment(), dx), (self.scroller.vadjustment(), dy)] {
            if d != 0 {
                adj.set_value(adj.value() + d as f64 * (adj.page_size() / 6.0).max(40.0));
            }
        }
    }

    pub fn close(&self) {
        self.root.set_visible(false);
        self.picture.set_paintable(None::<&gdk::Paintable>);
        self.texture.take();
    }
}
