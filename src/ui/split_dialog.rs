//! "Split Packs into Albums…": one book file per album of a pack.

use gtk::glib;
use gtk::prelude::*;

use super::widgets::{self, Form};
use super::{JobKind, State, refresh_sidebar, run_job};
use crate::archive::OutputFormat;
use crate::model::Book;
use crate::packs::{SplitFormat, SplitOptions, SplitTarget, split};

const FORMATS: [Option<OutputFormat>; 6] = [
    None,
    Some(OutputFormat::Cbz),
    Some(OutputFormat::Cb7),
    Some(OutputFormat::Cbr),
    Some(OutputFormat::Cbt),
    Some(OutputFormat::Pdf),
];

pub fn open(state: &State) {
    let Some(books) = state.require_selection() else { return };
    let current = state.settings.borrow().split.clone();
    let (win, content, buttons) = widgets::dialog(&state.window, t!("Split Packs into Albums"), 560, 300);
    content.append(
        &gtk::Label::builder()
            .label(t!(
                "{0} pack(s) selected. Each archive found inside a pack, and each folder of images, becomes a separate book.",
                books.len()
            ))
            .wrap(true)
            .xalign(0.0)
            .build(),
    );
    let mut form = Form::new();
    let format_labels: Vec<&str> = FORMATS
        .iter()
        .map(|f| match f {
            None => t!("Keep the albums as they are (image folders as CBZ)"),
            Some(f) => f.label(),
        })
        .collect();
    let format_index = FORMATS
        .iter()
        .position(|f| match (f, current.format) {
            (None, SplitFormat::Keep) => true,
            (Some(a), SplitFormat::Convert(b)) => *a == b,
            _ => false,
        })
        .unwrap_or(1);
    let format = widgets::dropdown(&format_labels, format_index);
    form.add(t!("Format:"), &format);
    let targets = [t!("In a folder named after the pack"), t!("Next to the pack"), t!("In the chosen folder")];
    let target_index = match current.target {
        SplitTarget::PackFolder => 0,
        SplitTarget::SameFolder => 1,
        SplitTarget::Folder(_) => 2,
    };
    let target = widgets::dropdown(&targets, target_index);
    form.add(t!("Destination:"), &target);
    let folder_text = match &current.target {
        SplitTarget::Folder(p) => p.display().to_string(),
        _ => String::new(),
    };
    let (folder_box, folder) = widgets::path_entry(win.upcast_ref(), &folder_text, false);
    folder_box.set_sensitive(target_index == 2);
    target.connect_selected_notify(glib::clone!(
        #[weak]
        folder_box,
        move |d| folder_box.set_sensitive(d.selected() == 2)
    ));
    form.add(t!("Folder:"), &folder_box);
    let delete = widgets::check(t!("Delete the pack once its albums are written"), current.delete_pack);
    form.add_full(&delete);
    content.append(&form.grid);

    let cancel = widgets::button(t!("Cancel"), false);
    let ok = widgets::button(t!("Split"), true);
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
            let options = SplitOptions {
                format: match FORMATS[format.selected() as usize] {
                    None => SplitFormat::Keep,
                    Some(f) => SplitFormat::Convert(f),
                },
                target: match target.selected() {
                    0 => SplitTarget::PackFolder,
                    1 => SplitTarget::SameFolder,
                    _ => {
                        let f = folder.text().trim().to_string();
                        if f.is_empty() {
                            widgets::alert(&win, t!("Missing folder"), t!("Choose the destination folder."));
                            return;
                        }
                        SplitTarget::Folder(f.into())
                    }
                },
                delete_pack: delete.is_active(),
            };
            if options.format == SplitFormat::Convert(OutputFormat::Cbr) && crate::archive::rar_program().is_none() {
                widgets::alert(&win, t!("“rar” program not found"), t!("Creating CBR files requires the \"rar\" program (WinRAR / RAR for Linux and macOS). Install it or set its path in the settings."));
                return;
            }
            {
                let mut s = st.settings.borrow_mut();
                s.split = options.clone();
                let _ = s.save();
            }
            win.close();
            run(&st, books.clone(), options);
        }
    ));
    buttons.append(&cancel);
    buttons.append(&ok);
    win.present();
}

fn run(state: &State, packs: Vec<Book>, options: SplitOptions) {
    let delete_pack = options.delete_pack;
    let ids = packs.iter().map(|b| b.id).collect();
    run_job(
        state,
        JobKind::Split,
        ids,
        t!("Splitting packs…"),
        move |ctx| {
            let mut new_books = Vec::new();
            let mut done_packs = Vec::new();
            let mut errors = Vec::new();
            let total = packs.len();
            for (i, pack) in packs.iter().enumerate() {
                if ctx.cancelled() {
                    break;
                }
                ctx.log(t!("Splitting {0}…", pack.file_path.display()));
                let c = ctx.clone();
                let name = pack.file_name();
                let result = split(&pack.file_path, &options, &mut |a, n, album| {
                    c.progress(i, total, t!("{0}: album {1}/{2} {3}", name, a + 1, n, album));
                });
                match result {
                    Ok(files) => {
                        for f in files {
                            ctx.log(t!("Created: {0}", f.display()));
                            let mut b = Book::from_file(f);
                            if let Err(e) = crate::library::refresh_from_file(&mut b, true) {
                                errors.push(t!("{0}: {1}", b.file_path.display(), format!("{e:#}")));
                            }
                            new_books.push(b);
                        }
                        done_packs.push(pack.id);
                    }
                    Err(e) => errors.push(t!("{0}: {1}", pack.file_path.display(), format!("{e:#}"))),
                }
            }
            (new_books, done_packs, errors)
        },
        move |st, (new_books, done_packs, errors)| {
            let n = new_books.len();
            let known: std::collections::HashSet<_> = st.library.borrow().books.iter().map(|b| b.file_path.clone()).collect();
            st.add_books(new_books.into_iter().filter(|b| !known.contains(&b.file_path)).collect());
            if delete_pack {
                st.remove_books(&done_packs);
            }
            st.save_library();
            refresh_sidebar(st);
            for e in &errors {
                st.journal.error(e);
            }
            let msg = super::with_errors(t!("{0} album(s) created from {1} pack(s)", n, done_packs.len()), errors.len());
            st.journal.info(&msg);
            st.set_status(&msg);
        },
    );
}
