//! Export / convert dialog (ComicRack "Export Books").

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

use super::widgets::{self, Form};
use super::{State, refresh_sidebar, run_job};
use crate::archive::OutputFormat;
use crate::convert::{DoublePages, ExportSettings, Naming, PageFormat, PageResize, Target, export};
use crate::i18n::tr;
use crate::model::{Book, PAGE_TYPES};

const TARGETS: [(Target, &str); 3] = [
    (Target::NewFolder, "Select a Folder"),
    (Target::SameAsSource, "Same Folder as the original Book"),
    (Target::ReplaceSource, "Same Folder and replace in Library"),
];
const NAMINGS: [(Naming, &str); 3] = [
    (Naming::Filename, "Original File Name"),
    (Naming::Caption, "Book Caption (series, number, title…)"),
    (Naming::Custom, "Custom: \"Name (n)\""),
];
const RESIZES: [(PageResize, &str); 4] = [
    (PageResize::Original, "Original Size"),
    (PageResize::WidthHeight, "Fit Width × Height"),
    (PageResize::Width, "Width"),
    (PageResize::Height, "Height"),
];
const DOUBLES: [(DoublePages, &str); 4] = [
    (DoublePages::Keep, "Keep"),
    (DoublePages::Split, "Split into two Pages"),
    (DoublePages::Rotate, "Rotate 90°"),
    (DoublePages::AdaptWidth, "Adapt Width (double)"),
];

fn pos<T: PartialEq>(items: &[(T, &str)], v: T) -> usize {
    items.iter().position(|(x, _)| *x == v).unwrap_or(0)
}

/// Translated labels of a constant table (English literals).
fn labels<T>(items: &[(T, &'static str)]) -> Vec<&'static str> {
    items.iter().map(|(_, l)| tr(l)).collect()
}

/// All widgets of the form, to read back an `ExportSettings`.
struct ExportForm {
    target: gtk::DropDown,
    folder: gtk::Entry,
    delete_original: gtk::CheckButton,
    add_to_library: gtk::CheckButton,
    overwrite: gtk::CheckButton,
    combine: gtk::CheckButton,
    naming: gtk::DropDown,
    custom_name: gtk::Entry,
    custom_start: gtk::SpinButton,
    format: gtk::DropDown,
    compression: gtk::DropDown,
    embed: gtk::CheckButton,
    remove_pages: gtk::CheckButton,
    remove_types: Vec<(String, gtk::CheckButton)>,
    include_pages: gtk::Entry,
    ignore_errors: gtk::CheckButton,
    tags: gtk::Entry,
    page_format: gtk::DropDown,
    quality: gtk::SpinButton,
    resize: gtk::DropDown,
    width: gtk::SpinButton,
    height: gtk::SpinButton,
    dont_enlarge: gtk::CheckButton,
    double: gtk::DropDown,
    keep_names: gtk::CheckButton,
    brightness: gtk::Scale,
    contrast: gtk::Scale,
    saturation: gtk::Scale,
    gamma: gtk::Scale,
    sharpen: gtk::Scale,
    auto_contrast: gtk::CheckButton,
}

fn scale(min: f64, max: f64) -> gtk::Scale {
    let s = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, 1.0);
    s.set_draw_value(true);
    s.set_hexpand(true);
    s.add_mark(0.0, gtk::PositionType::Bottom, None);
    s
}

impl ExportForm {
    fn set(&self, s: &ExportSettings) {
        self.target.set_selected(pos(&TARGETS, s.target) as u32);
        self.folder.set_text(&s.target_folder);
        self.delete_original.set_active(s.delete_original);
        self.add_to_library.set_active(s.add_to_library);
        self.overwrite.set_active(s.overwrite);
        self.combine.set_active(s.combine);
        self.naming.set_selected(pos(&NAMINGS, s.naming) as u32);
        self.custom_name.set_text(&s.custom_name);
        self.custom_start.set_value(s.custom_naming_start as f64);
        self.format.set_selected(match s.format {
            None => 0,
            Some(f) => OutputFormat::ALL.iter().position(|x| *x == f).map(|i| i + 1).unwrap_or(1),
        } as u32);
        self.compression.set_selected(s.compression.min(2) as u32);
        self.embed.set_active(s.embed_comic_info);
        self.remove_pages.set_active(s.remove_pages);
        for (t, c) in &self.remove_types {
            c.set_active(s.remove_page_types.contains(t));
        }
        self.include_pages.set_text(&s.include_pages);
        self.ignore_errors.set_active(s.ignore_error_pages);
        self.tags.set_text(&s.tags_to_append);
        self.page_format.set_selected(PageFormat::ALL.iter().position(|x| *x == s.page_format).unwrap_or(0) as u32);
        self.quality.set_value(s.page_quality as f64);
        self.resize.set_selected(pos(&RESIZES, s.page_resize) as u32);
        self.width.set_value(s.page_width as f64);
        self.height.set_value(s.page_height as f64);
        self.dont_enlarge.set_active(s.dont_enlarge);
        self.double.set_selected(pos(&DOUBLES, s.double_pages) as u32);
        self.keep_names.set_active(s.keep_original_names);
        self.brightness.set_value(s.brightness as f64);
        self.contrast.set_value(s.contrast as f64);
        self.saturation.set_value(s.saturation as f64);
        self.gamma.set_value(s.gamma as f64);
        self.sharpen.set_value(s.sharpen as f64);
        self.auto_contrast.set_active(s.auto_contrast);
    }

    fn get(&self, name: &str) -> ExportSettings {
        ExportSettings {
            name: name.to_string(),
            target: TARGETS[self.target.selected() as usize].0,
            target_folder: self.folder.text().trim().to_string(),
            delete_original: self.delete_original.is_active(),
            add_to_library: self.add_to_library.is_active(),
            overwrite: self.overwrite.is_active(),
            combine: self.combine.is_active(),
            naming: NAMINGS[self.naming.selected() as usize].0,
            custom_name: self.custom_name.text().trim().to_string(),
            custom_naming_start: self.custom_start.value() as i32,
            format: match self.format.selected() {
                0 => None,
                i => Some(OutputFormat::ALL[i as usize - 1]),
            },
            compression: self.compression.selected() as u8,
            embed_comic_info: self.embed.is_active(),
            remove_pages: self.remove_pages.is_active(),
            remove_page_types: self.remove_types.iter().filter(|(_, c)| c.is_active()).map(|(t, _)| t.clone()).collect(),
            include_pages: self.include_pages.text().trim().to_string(),
            ignore_error_pages: self.ignore_errors.is_active(),
            tags_to_append: self.tags.text().trim().to_string(),
            page_format: PageFormat::ALL[self.page_format.selected() as usize],
            page_quality: self.quality.value() as u8,
            page_resize: RESIZES[self.resize.selected() as usize].0,
            page_width: self.width.value() as u32,
            page_height: self.height.value() as u32,
            dont_enlarge: self.dont_enlarge.is_active(),
            double_pages: DOUBLES[self.double.selected() as usize].0,
            keep_original_names: self.keep_names.is_active(),
            brightness: self.brightness.value() as i32,
            contrast: self.contrast.value() as i32,
            saturation: self.saturation.value() as i32,
            gamma: self.gamma.value() as i32,
            sharpen: self.sharpen.value() as i32,
            auto_contrast: self.auto_contrast.is_active(),
        }
    }
}

fn tab(form: &Form) -> gtk::ScrolledWindow {
    form.grid.set_margin_start(12);
    form.grid.set_margin_end(12);
    form.grid.set_margin_top(12);
    form.grid.set_margin_bottom(12);
    widgets::scrolled(&form.grid)
}

pub fn open(state: &State) {
    let Some(books) = state.require_selection() else { return };
    let (win, content, buttons) = widgets::dialog(&state.window, &t!("Export {0} Book(s)", books.len()), 720, 640);

    // Presets
    let mut presets = ExportSettings::presets();
    presets.extend(state.settings.borrow().export_presets.clone());
    let presets = Rc::new(RefCell::new(presets));
    let mut preset_names: Vec<String> = vec![t!("(current settings)").into()];
    preset_names.extend(presets.borrow().iter().map(|p| p.name.clone()));
    let names_ref: Vec<&str> = preset_names.iter().map(String::as_str).collect();
    let preset_dd = widgets::dropdown(&names_ref, 0);
    let save_preset = gtk::Button::with_label(t!("Save as Preset…"));
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    top.append(&gtk::Label::new(Some(t!("Preset:"))));
    preset_dd.set_hexpand(true);
    top.append(&preset_dd);
    top.append(&save_preset);
    content.append(&top);

    let notebook = gtk::Notebook::new();
    notebook.set_vexpand(true);
    content.append(&notebook);

    // --- File tab
    let mut f = Form::new();
    f.add_title(t!("Export Location"));
    let target = widgets::dropdown(&labels(&TARGETS), 0);
    f.add(t!("Export:"), &target);
    let (folder_box, folder) = widgets::path_entry(win.upcast_ref(), "", false);
    f.add(t!("Folder:"), &folder_box);
    let delete_original = widgets::check(t!("Delete original Book after successful Export"), false);
    let add_to_library = widgets::check(t!("Add newly created Book to Library"), false);
    let overwrite = widgets::check(t!("Overwrite existing Files"), false);
    let combine = widgets::check(t!("Combine all selected Files"), false);
    for c in [&delete_original, &add_to_library, &overwrite, &combine] {
        f.add_full(c);
    }
    f.add_title(t!("File Names"));
    let naming = widgets::dropdown(&labels(&NAMINGS), 0);
    f.add(t!("Naming:"), &naming);
    let custom_name = widgets::entry("");
    f.add(t!("Custom Name:"), &custom_name);
    let custom_start = widgets::spin(0.0, 100000.0, 1.0, 1.0);
    f.add(t!("Start Numbering at:"), &custom_start);
    f.add_title(t!("Export Format"));
    let mut fmt_labels = vec![t!("Same as Original")];
    fmt_labels.extend(OutputFormat::ALL.iter().map(|f| f.label()));
    let format = widgets::dropdown(&fmt_labels, 1);
    f.add(t!("Format:"), &format);
    let compression = widgets::dropdown(&[t!("None"), t!("Medium"), t!("Strong")], 0);
    f.add(t!("Compression:"), &compression);
    let embed = widgets::check(t!("Embed Book Info (ComicInfo.xml)"), true);
    f.add_full(&embed);
    let remove_pages = widgets::check(t!("Remove Pages of the following Types:"), true);
    f.add_full(&remove_pages);
    let types_box = gtk::FlowBox::builder().selection_mode(gtk::SelectionMode::None).max_children_per_line(4).build();
    let mut remove_types = Vec::new();
    for t in PAGE_TYPES {
        let c = widgets::check(super::edit_dialog::page_type_label(t), *t == "Deleted");
        types_box.insert(&c, -1);
        remove_types.push((t.to_string(), c));
    }
    f.add_full(&types_box);
    let include_pages = widgets::entry("");
    include_pages.set_placeholder_text(Some(t!("e.g. 1-5,8 (empty = all)")));
    f.add(t!("Included Pages:"), &include_pages);
    let ignore_errors = widgets::check(t!("Ignore unreadable Pages (copied as is)"), false);
    f.add_full(&ignore_errors);
    let tags = widgets::entry("");
    tags.set_placeholder_text(Some(t!("tags to add, separated by commas")));
    f.add(t!("Add Tags:"), &tags);
    notebook.append_page(&tab(&f), Some(&gtk::Label::new(Some(t!("File")))));

    // --- Pages tab
    let mut p = Form::new();
    let pf_labels: Vec<&str> = PageFormat::ALL.iter().map(|f| f.label()).collect();
    let page_format = widgets::dropdown(&pf_labels, 0);
    p.add(t!("Image Format:"), &page_format);
    let quality = widgets::spin(1.0, 100.0, 1.0, 75.0);
    p.add(t!("JPEG Quality:"), &quality);
    let resize = widgets::dropdown(&labels(&RESIZES), 0);
    p.add(t!("Resize:"), &resize);
    let width = widgets::spin(16.0, 20000.0, 10.0, 1000.0);
    p.add(t!("Width:"), &width);
    let height = widgets::spin(16.0, 20000.0, 10.0, 1000.0);
    p.add(t!("Height:"), &height);
    let dont_enlarge = widgets::check(t!("Don't enlarge"), true);
    p.add_full(&dont_enlarge);
    let double = widgets::dropdown(&labels(&DOUBLES), 0);
    p.add(t!("Double Pages:"), &double);
    let keep_names = widgets::check(t!("Keep original image names"), true);
    p.add_full(&keep_names);
    notebook.append_page(&tab(&p), Some(&gtk::Label::new(Some(t!("Pages")))));

    // --- Image tab
    let mut im = Form::new();
    let brightness = scale(-100.0, 100.0);
    let contrast = scale(-100.0, 100.0);
    let saturation = scale(-100.0, 100.0);
    let gamma = scale(-100.0, 100.0);
    let sharpen = scale(0.0, 100.0);
    im.add(t!("Brightness:"), &brightness);
    im.add(t!("Contrast:"), &contrast);
    im.add(t!("Saturation:"), &saturation);
    im.add(t!("Gamma:"), &gamma);
    im.add(t!("Sharpen:"), &sharpen);
    let auto_contrast = widgets::check(t!("Auto Contrast Enhancement"), false);
    im.add_full(&auto_contrast);
    notebook.append_page(&tab(&im), Some(&gtk::Label::new(Some(t!("Image Processing")))));

    let form = Rc::new(ExportForm {
        target, folder, delete_original, add_to_library, overwrite, combine, naming, custom_name, custom_start,
        format, compression, embed, remove_pages, remove_types, include_pages, ignore_errors, tags, page_format,
        quality, resize, width, height, dont_enlarge, double, keep_names, brightness, contrast, saturation, gamma,
        sharpen, auto_contrast,
    });
    form.set(&state.settings.borrow().export);

    preset_dd.connect_selected_notify(glib::clone!(
        #[strong]
        form,
        #[strong]
        presets,
        #[strong]
        state,
        move |d| {
            let i = d.selected() as usize;
            if i == 0 {
                form.set(&state.settings.borrow().export);
            } else if let Some(p) = presets.borrow().get(i - 1) {
                form.set(p);
            }
        }
    ));
    save_preset.connect_clicked(glib::clone!(
        #[strong]
        form,
        #[strong]
        state,
        #[weak]
        win,
        move |_| {
            let (dlg, c, b) = widgets::dialog(&win, t!("New Preset"), 360, 120);
            let name = widgets::entry("");
            name.set_placeholder_text(Some(t!("Preset name")));
            c.append(&name);
            let ok = widgets::button(t!("Save"), true);
            let form = form.clone();
            let state = state.clone();
            ok.connect_clicked(glib::clone!(
                #[weak]
                dlg,
                #[weak]
                name,
                move |_| {
                    let n = name.text().trim().to_string();
                    if n.is_empty() {
                        return;
                    }
                    let preset = form.get(&n);
                    let mut s = state.settings.borrow_mut();
                    s.export_presets.retain(|p| p.name != n);
                    s.export_presets.push(preset);
                    let _ = s.save();
                    dlg.close();
                }
            ));
            b.append(&ok);
            dlg.present();
        }
    ));

    let cancel = widgets::button(t!("Cancel"), false);
    let ok = widgets::button(t!("Export"), true);
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
            let s = form.get(t!("Custom"));
            if s.target == Target::NewFolder && s.target_folder.is_empty() {
                widgets::alert(&win, t!("Missing folder"), t!("Choose the destination folder."));
                return;
            }
            {
                let mut set = st.settings.borrow_mut();
                set.export = s.clone();
                let _ = set.save();
            }
            win.close();
            run_export_books(&st, s, books.clone());
        }
    ));
    buttons.append(&cancel);
    buttons.append(&ok);
    win.present();
}

/// Runs a preset on the current selection.
pub fn run_export(state: &State, settings: ExportSettings) {
    let Some(books) = state.require_selection() else { return };
    run_export_books(state, settings, books);
}

fn run_export_books(state: &State, settings: ExportSettings, books: Vec<Book>) {
    if settings.format == Some(OutputFormat::Cbr) && crate::archive::rar_program().is_none() {
        widgets::alert(
            &state.window,
            t!("\"rar\" program not found"),
            t!("Creating CBR files requires the \"rar\" program (WinRAR / RAR for Linux and macOS). Install it or set its path in the settings."),
        );
        return;
    }
    let ask = settings.target == Target::ReplaceSource || settings.delete_original;
    let st = state.clone();
    let go = move || {
        let s = settings.clone();
        run_job(
            &st,
            t!("Converting…"),
            move |ctx| {
                let groups: Vec<Vec<Book>> = if s.combine { vec![books] } else { books.into_iter().map(|b| vec![b]).collect() };
                let total = groups.len();
                let mut results = Vec::new();
                let mut errors = Vec::new();
                for (i, g) in groups.into_iter().enumerate() {
                    if ctx.cancelled() {
                        break;
                    }
                    ctx.progress(i, total, format!("{}/{} {}", i + 1, total, g[0].file_name()));
                    ctx.log(t!("Converting {0}…", g[0].file_path.display()));
                    match export(&g, &s, i) {
                        Ok(r) => results.push(r),
                        Err(e) => errors.push(t!("{0}: {1}", g[0].file_path.display(), format!("{e:#}"))),
                    }
                }
                (results, errors)
            },
            |st, (results, errors)| {
                let n = results.len();
                let mut updated = Vec::new();
                let mut removed = Vec::new();
                let mut added = Vec::new();
                for r in results {
                    st.journal.info(&t!("Converted: {0}", r.new_path.display()));
                    if r.replace_in_library {
                        let first = r.source_ids[0];
                        if let Some(mut b) = st.library.borrow().get(first).cloned() {
                            b.file_path = r.new_path.clone();
                            b.info = r.info.clone();
                            b.info_dirty = false;
                            b.refresh_file_stats();
                            updated.push(b);
                        }
                        removed.extend(r.source_ids.iter().skip(1).copied());
                    } else {
                        if r.deleted_sources {
                            removed.extend(r.source_ids.iter().copied());
                        }
                        if r.add_to_library && !st.library.borrow().books.iter().any(|b| b.file_path == r.new_path) {
                            let mut b = Book::from_file(r.new_path.clone());
                            b.info = r.info.clone();
                            added.push(b);
                        }
                    }
                }
                st.remove_books(&removed);
                st.add_books(added);
                st.update_books(updated, false);
                st.save_library();
                refresh_sidebar(st);
                for e in &errors {
                    st.journal.error(e);
                }
                st.set_status(&super::with_errors(t!("{0} book(s) converted", n), errors.len()));
            },
        );
    };
    if ask {
        widgets::confirm(
            &state.window,
            t!("Replace the original files?"),
            t!("The original files will be deleted after a successful conversion."),
            t!("Convert"),
            go,
        );
    } else {
        go();
    }
}
