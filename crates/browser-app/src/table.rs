//! Native GFM tables. Cell markup is escaped; links are always routed by the host.
use gtk::{glib, prelude::*};
use pulldown_cmark::{Alignment, Event, Tag, TagEnd};
use std::rc::Rc;

#[derive(Default)]
struct Cell {
    text: String,
    markup: String,
}

pub struct Table {
    alignments: Vec<Alignment>,
    rows: Vec<Vec<Cell>>,
    row: Vec<Cell>,
    cell: Cell,
    pub links: Vec<String>,
}

impl Table {
    pub fn new(alignments: Vec<Alignment>) -> Self {
        Self {
            alignments,
            rows: Vec::new(),
            row: Vec::new(),
            cell: Cell::default(),
            links: Vec::new(),
        }
    }
    pub fn push(&mut self, event: Event<'_>) {
        match event {
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                self.cell.text.push_str(&text);
                self.cell.markup.push_str(&glib::markup_escape_text(&text));
            }
            Event::Code(text) => {
                self.cell.text.push_str(&text);
                self.cell
                    .markup
                    .push_str(&format!("<tt>{}</tt>", glib::markup_escape_text(&text)));
            }
            Event::Start(Tag::Strong) => self.cell.markup.push_str("<b>"),
            Event::End(TagEnd::Strong) => self.cell.markup.push_str("</b>"),
            Event::Start(Tag::Emphasis) => self.cell.markup.push_str("<i>"),
            Event::End(TagEnd::Emphasis) => self.cell.markup.push_str("</i>"),
            Event::Start(Tag::Strikethrough) => self.cell.markup.push_str("<s>"),
            Event::End(TagEnd::Strikethrough) => self.cell.markup.push_str("</s>"),
            Event::Start(Tag::Link { dest_url, .. }) => {
                self.links.push(dest_url.to_string());
                self.cell.markup.push_str(&format!(
                    "<a href=\"{}\">",
                    glib::markup_escape_text(&dest_url)
                ));
            }
            Event::End(TagEnd::Link) => self.cell.markup.push_str("</a>"),
            Event::SoftBreak | Event::HardBreak => {
                self.cell.text.push(' ');
                self.cell.markup.push(' ');
            }
            Event::End(TagEnd::TableCell) => self.row.push(std::mem::take(&mut self.cell)),
            Event::End(TagEnd::TableHead | TagEnd::TableRow) => {
                self.rows.push(std::mem::take(&mut self.row))
            }
            _ => {}
        }
    }
    pub fn widget(&self, follow: Rc<dyn Fn(&str)>) -> Result<(gtk::Widget, String), String> {
        if self.alignments.len() > 12
            || self.rows.len() > 1000
            || self.rows.iter().map(Vec::len).sum::<usize>() > 4096
            || self
                .rows
                .iter()
                .flatten()
                .any(|cell| cell.text.chars().count() > 256)
        {
            return Err(
                "table limit: 12 columns, 1,000 rows, 4,096 cells, 256 characters per cell".into(),
            );
        }
        let grid = gtk::Grid::builder()
            .column_homogeneous(false)
            .row_homogeneous(false)
            .build();
        grid.add_css_class("smad-table");
        let css = gtk::CssProvider::new();
        css.load_from_data(".smad-table-cell { padding: 8px 12px; border-bottom: 1px solid alpha(currentColor, 0.18); } .smad-table-header { font-weight: bold; background-color: alpha(currentColor, 0.06); }");
        let mut plain = String::new();
        for (row, cells) in self.rows.iter().enumerate() {
            for (column, cell) in cells.iter().enumerate() {
                if column > 0 {
                    plain.push('\t');
                }
                plain.push_str(&cell.text);
                let label = gtk::Label::new(None);
                label.set_markup(&cell.markup);
                label.set_selectable(true);
                label.set_wrap(true);
                label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
                let width = self
                    .rows
                    .iter()
                    .filter_map(|row| row.get(column))
                    .map(|cell| cell.text.chars().count())
                    .max()
                    .unwrap_or(10)
                    .clamp(10, 32);
                // TextView child anchors allocate minimum width; an explicit readable
                // cell minimum prevents wrapping every word into single characters.
                label.set_width_chars(width as i32);
                label.set_max_width_chars(32);
                label.set_hexpand(true);
                label.set_halign(gtk::Align::Fill);
                label.set_valign(gtk::Align::Fill);
                label.add_css_class("smad-table-cell");
                #[allow(deprecated)]
                label
                    .style_context()
                    .add_provider(&css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
                if row == 0 {
                    label.add_css_class("smad-table-header");
                }
                let (align, justify) = match self.alignments.get(column) {
                    Some(Alignment::Right) => (1.0, gtk::Justification::Right),
                    Some(Alignment::Center) => (0.5, gtk::Justification::Center),
                    _ => (0.0, gtk::Justification::Left),
                };
                label.set_xalign(align);
                label.set_justify(justify);
                let follow = follow.clone();
                label.connect_activate_link(move |_, uri| {
                    follow(uri);
                    glib::Propagation::Stop
                });
                grid.attach(&label, column as i32, row as i32, 1, 1);
            }
            plain.push('\n');
        }
        Ok((grid.upcast(), plain))
    }
}
