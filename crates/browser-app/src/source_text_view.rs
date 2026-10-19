//! Paint attention behind entire source rows, including GTK's unpainted empty paragraphs.
use gtk::{glib, prelude::*, subclass::prelude::*};

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct SourceTextView;

    #[glib::object_subclass]
    impl ObjectSubclass for SourceTextView {
        const NAME: &'static str = "SmadSourceTextView";
        type Type = super::SourceTextView;
        type ParentType = gtk::TextView;
    }
    impl ObjectImpl for SourceTextView {}
    impl WidgetImpl for SourceTextView {}
    impl TextViewImpl for SourceTextView {
        fn snapshot_layer(&self, layer: gtk::TextViewLayer, snapshot: gtk::Snapshot) {
            if layer == gtk::TextViewLayer::BelowText {
                let view = self.obj();
                let buffer = view.buffer();
                if let Some(tag) = buffer.tag_table().lookup("agent-highlight")
                    && let Some(color) =
                        tag.property::<Option<gtk::gdk::RGBA>>("paragraph-background-rgba")
                {
                    // Snapshot layers use buffer coordinates. Visit only visible rows;
                    // GTK still draws selection, syntax text and the caret above us.
                    let visible = view.visible_rect();
                    let (mut iter, _) = view.line_at_y(visible.y());
                    loop {
                        let (y, height) = view.line_yrange(&iter);
                        if y >= visible.y() + visible.height() {
                            break;
                        }
                        if iter.has_tag(&tag) {
                            snapshot.append_color(
                                &color,
                                &gtk::graphene::Rect::new(
                                    visible.x() as f32,
                                    y as f32,
                                    visible.width() as f32,
                                    height as f32,
                                ),
                            );
                        }
                        if !iter.forward_line() {
                            break;
                        }
                    }
                }
            }
            self.parent_snapshot_layer(layer, snapshot);
        }
    }
}

glib::wrapper! {
    pub struct SourceTextView(ObjectSubclass<imp::SourceTextView>)
        @extends gtk::TextView, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Scrollable;
}
