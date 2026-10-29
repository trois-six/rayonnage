//! "Autonumber Wizard": numbers the selected books in list order.

use gtk::glib;
use gtk::prelude::*;

use super::State;
use super::widgets::{self, Form};
use crate::autonumber::{AutoNumberOptions, apply};

pub fn open(state: &State) {
    let Some(books) = state.require_selection() else { return };
    let defaults = AutoNumberOptions::for_books(&books, false);
    let (win, content, buttons) = widgets::dialog(&state.window, t!("Automatic Numbering"), 460, 260);
    content.append(
        &gtk::Label::builder()
            .label(t!(
                "{0} book(s) will be numbered in the displayed list order.",
                books.len()
            ))
            .xalign(0.0)
            .wrap(true)
            .build(),
    );
    let mut form = Form::new();
    let mode = widgets::dropdown(&[t!("Number the series"), t!("Number the alternate series")], 0);
    let start = widgets::spin(-100000.0, 100000.0, 1.0, defaults.start as f64);
    let set_count = widgets::check(t!("Set the total count:"), false);
    let count = widgets::spin(-1.0, 100000.0, 1.0, defaults.count as f64);
    count.set_sensitive(false);
    set_count.connect_toggled(glib::clone!(
        #[weak]
        count,
        move |c| count.set_sensitive(c.is_active())
    ));
    let first = books.clone();
    mode.connect_selected_notify(glib::clone!(
        #[weak]
        start,
        #[weak]
        count,
        move |d| {
            let o = AutoNumberOptions::for_books(&first, d.selected() == 1);
            start.set_value(o.start as f64);
            count.set_value(o.count as f64);
        }
    ));
    start.connect_value_changed(glib::clone!(
        #[weak]
        count,
        #[strong]
        books,
        move |s| count.set_value(s.value() + books.len() as f64 - 1.0)
    ));
    form.add(t!("Mode:"), &mode);
    form.add(t!("Start at number:"), &start);
    form.grid.attach(&set_count, 0, 2, 1, 1);
    form.grid.attach(&count, 1, 2, 1, 1);
    content.append(&form.grid);

    let cancel = widgets::button(t!("Cancel"), false);
    let ok = widgets::button(t!("Renumber"), true);
    cancel.connect_clicked(glib::clone!(
        #[weak]
        win,
        move |_| win.close()
    ));
    let st = state.clone();
    ok.connect_clicked(glib::clone!(
        #[weak]
        win,
        move |_| {
            let opts = AutoNumberOptions {
                alternate: mode.selected() == 1,
                start: start.value() as i32,
                set_count: set_count.is_active(),
                count: count.value() as i32,
            };
            let mut books = books.clone();
            apply(&mut books, &opts);
            let n = books.len();
            st.update_books(books, true);
            st.set_status(&t!("{0} book(s) numbered", n));
            win.close();
        }
    ));
    buttons.append(&cancel);
    buttons.append(&ok);
    win.present();
}
