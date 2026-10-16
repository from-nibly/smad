use super::{markdown, webview};
use gtk::prelude::*;
use std::rc::Rc;

pub enum DocumentView {
    Review(Rc<webview::DocumentView>),
    Markdown(Rc<markdown::MarkdownView>),
}

impl DocumentView {
    pub fn widget(&self) -> gtk::Widget {
        match self {
            Self::Review(view) => view.widget(),
            Self::Markdown(view) => view.widget().upcast(),
        }
    }
    pub fn loaded_url(&self) -> String {
        match self {
            Self::Review(view) => view.loaded_url(),
            Self::Markdown(_) => String::new(),
        }
    }
    pub fn navigate(&self, url: &str) {
        if let Self::Review(view) = self {
            view.navigate(url);
        }
    }
    pub fn reload(&self) {
        match self {
            Self::Review(view) => view.reload(),
            Self::Markdown(view) => view.reload(),
        }
    }
    pub fn heading(&self, fragment: &str) {
        if let Self::Markdown(view) = self {
            view.heading(fragment);
        }
    }
    pub fn automation_webview(&self) -> Option<webkit6::WebView> {
        match self {
            Self::Review(view) => Some(view.automation_webview()),
            Self::Markdown(_) => None,
        }
    }
}
