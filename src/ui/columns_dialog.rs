//! Column chooser for the book list.

use gtk::glib;
use gtk::prelude::*;

use super::widgets;
use super::{State, save_columns};

pub fn open(state: &State) {
    let (win, content, buttons) = widgets::dialog(&state.window, t!("Visible Columns"), 420, 620);
    content.append(
        &gtk::Label::builder()
            .label(t!("Check the columns to show. Drag the column headers to change their order."))
            .wrap(true)
            .xalign(0.0)
            .build(),
    );
    let flow = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let mut cols: Vec<_> = state.columns.borrow().clone();
    cols.sort_by_key(|(f, _)| f.label().to_lowercase());
    for (field, col) in cols {
        let c = widgets::check(field.label(), col.is_visible());
        c.connect_toggled(move |c| col.set_visible(c.is_active()));
        flow.append(&c);
    }
    content.append(&widgets::scrolled(&flow));
    let reset = widgets::button(t!("Restore Default Columns"), false);
    let st = state.clone();
    reset.connect_clicked(glib::clone!(
        #[weak]
        win,
        move |_| {
            let defaults = crate::settings::default_columns();
            for (i, d) in defaults.iter().enumerate() {
                if let Some((_, col)) = st.columns.borrow().iter().find(|(f, _)| *f == d.field) {
                    col.set_visible(d.visible);
                    col.set_fixed_width(d.width);
                    st.column_view.insert_column(i as u32, col);
                }
            }
            win.close();
        }
    ));
    let close = widgets::button(t!("Close"), true);
    let st = state.clone();
    close.connect_clicked(glib::clone!(
        #[weak]
        win,
        move |_| {
            save_columns(&st);
            let _ = st.settings.borrow().save();
            win.close();
        }
    ));
    buttons.append(&reset);
    buttons.append(&close);
    win.present();
}
