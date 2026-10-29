//! Toolbar icons drawn with cairo, in the style of the symbolic theme icons:
//! single colour (the widget's foreground colour, so they follow light/dark
//! themes), 16×16 design grid, crisp at any scale.
//!
//! They are inspired by the original projects: ComicRack's export (pages and
//! an arrow), the "BD" speech bubble of Bedetheque Scraper 2 and the book
//! stack of Library Organizer.

use gtk::cairo;
use gtk::prelude::*;

#[derive(Clone, Copy, Debug)]
pub enum Icon {
    /// Conversion / export: a page with an arrow towards another format.
    Convert,
    /// Bedetheque Scraper: speech bubble with "BD".
    Scraper,
    /// Library Organizer: stack of books.
    Organizer,
}

/// A 16×16 widget drawing the icon in the current foreground colour.
pub fn icon(kind: Icon) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(16)
        .content_height(16)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    area.set_draw_func(move |w, cr, width, height| {
        let c = w.color();
        cr.set_source_rgba(c.red() as f64, c.green() as f64, c.blue() as f64, c.alpha() as f64);
        let s = width.min(height) as f64 / 16.0;
        cr.translate((width as f64 - 16.0 * s) / 2.0, (height as f64 - 16.0 * s) / 2.0);
        cr.scale(s, s);
        cr.set_line_width(1.5);
        cr.set_line_join(cairo::LineJoin::Round);
        cr.set_line_cap(cairo::LineCap::Round);
        let _ = match kind {
            Icon::Convert => convert(cr),
            Icon::Scraper => scraper(cr),
            Icon::Organizer => organizer(cr),
        };
    });
    area
}

fn rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    use std::f64::consts::PI;
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 3.0 * PI / 2.0);
    cr.close_path();
}

/// A page with a folded corner and an arrow pointing to the right.
fn convert(cr: &cairo::Context) -> Result<(), cairo::Error> {
    // Page with a folded corner.
    cr.move_to(1.75, 1.75);
    cr.line_to(6.5, 1.75);
    cr.line_to(9.25, 4.5);
    cr.line_to(9.25, 14.25);
    cr.line_to(1.75, 14.25);
    cr.close_path();
    cr.stroke()?;
    cr.move_to(6.5, 1.75);
    cr.line_to(6.5, 4.5);
    cr.line_to(9.25, 4.5);
    cr.stroke()?;
    // Arrow (its shaft crosses the page edge).
    cr.move_to(5.0, 9.5);
    cr.line_to(14.0, 9.5);
    cr.stroke()?;
    cr.move_to(11.0, 6.5);
    cr.line_to(14.25, 9.5);
    cr.line_to(11.0, 12.5);
    cr.stroke()
}

/// A speech bubble with "BD" inside, like the Bedetheque Scraper logo.
fn scraper(cr: &cairo::Context) -> Result<(), cairo::Error> {
    // Bubble with its tail.
    cr.save()?;
    cr.translate(8.0, 7.0);
    cr.scale(7.0, 5.5);
    cr.arc(0.0, 0.0, 1.0, 0.0, 2.0 * std::f64::consts::PI);
    cr.restore()?;
    cr.stroke()?;
    cr.move_to(3.5, 11.0);
    cr.line_to(2.25, 15.0);
    cr.line_to(6.5, 12.25);
    cr.stroke()?;
    // "BD" letters.
    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    cr.set_font_size(7.0);
    let ext = cr.text_extents("BD")?;
    cr.move_to(8.0 - ext.width() / 2.0 - ext.x_bearing(), 7.0 - ext.height() / 2.0 - ext.y_bearing());
    cr.show_text("BD")
}

/// Three books stacked with an offset, like the Library Organizer icon.
fn organizer(cr: &cairo::Context) -> Result<(), cairo::Error> {
    for (x, y) in [(3.75, 1.75), (1.75, 6.25), (3.0, 10.75)] {
        rounded_rect(cr, x, y, 10.5, 3.5, 0.75);
        cr.stroke()?;
        // Edge of the pages, on the right of each book.
        cr.move_to(x + 8.0, y + 0.75);
        cr.line_to(x + 8.0, y + 2.75);
        cr.stroke()?;
    }
    Ok(())
}
