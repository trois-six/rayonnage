//! Background operations. Each one runs in a thread and has its own line in
//! the status bar, with its progress and a Cancel button. As in ComicRack,
//! operations of different kinds run at the same time; two of the same kind,
//! or two that change the files of the same books, run one after the other.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use gtk::glib;
use gtk::prelude::*;

use super::{AppState, State, update_status};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    Scan,
    /// Rereading the book files.
    Refresh,
    /// Writing the information into the book files.
    WriteFiles,
    Convert,
    Organize,
    Split,
    Scrape,
}

impl JobKind {
    /// Whether the operation rewrites, moves or deletes book files.
    fn changes_files(self) -> bool {
        matches!(self, JobKind::WriteFiles | JobKind::Convert | JobKind::Organize | JobKind::Split)
    }
}

/// An operation, running or waiting for its turn.
pub struct Job {
    kind: JobKind,
    /// The books whose files it changes.
    books: HashSet<u64>,
    cancel: Arc<AtomicBool>,
    row: gtk::Box,
    label: gtk::Label,
    bar: gtk::ProgressBar,
    /// Starts the operation; None once started.
    start: RefCell<Option<Box<dyn FnOnce()>>>,
}

impl Job {
    fn started(&self) -> bool {
        self.start.borrow().is_none()
    }

    /// Whether it must wait until `earlier`, submitted before, is finished.
    fn waits_for(&self, earlier: &Job) -> bool {
        earlier.kind == self.kind
            || (earlier.kind.changes_files() && self.kind.changes_files() && !earlier.books.is_disjoint(&self.books))
    }

    fn set_text(&self, text: &str) {
        self.label.set_text(text);
        self.label.set_tooltip_text(Some(text));
    }
}

impl AppState {
    /// Whether an operation is running or waiting.
    pub fn busy(&self) -> bool {
        !self.jobs.borrow().is_empty()
    }

    pub fn has_job(&self, kind: JobKind) -> bool {
        self.jobs.borrow().iter().any(|j| j.kind == kind)
    }

    /// Cancels every operation (when quitting).
    pub fn cancel_jobs(&self) {
        let waiting: Vec<Rc<Job>> = self.jobs.borrow().iter().filter(|j| !j.started()).cloned().collect();
        for j in &waiting {
            self.forget_job(j);
        }
        for j in self.jobs.borrow().iter() {
            j.cancel.store(true, Ordering::Relaxed);
        }
    }

    fn forget_job(&self, job: &Rc<Job>) {
        self.jobs.borrow_mut().retain(|j| !Rc::ptr_eq(j, job));
        self.jobs_box.remove(&job.row);
        self.journal.set_active(self.jobs.borrow().iter().any(|j| j.started()));
    }
}

/// Starts the waiting operations whose turn has come, in the order they
/// were asked for.
fn start_ready(state: &State) {
    loop {
        let ready = {
            let jobs = state.jobs.borrow();
            jobs.iter()
                .enumerate()
                .find(|(i, j)| !j.started() && !jobs[..*i].iter().any(|e| j.waits_for(e)))
                .map(|(_, j)| j.clone())
        };
        let Some(job) = ready else { return };
        let start = job.start.borrow_mut().take();
        if let Some(start) = start {
            start();
        }
    }
}

type UiFn = Box<dyn FnOnce(&State) + Send>;

enum JobMsg {
    Progress(f64, String),
    /// Progress of unknown length.
    Pulse(String),
    Log(String),
    Error(String),
    Ui(UiFn),
    Finished,
}

/// Handle given to the worker thread of a job.
#[derive(Clone)]
pub struct JobCtx {
    tx: async_channel::Sender<JobMsg>,
    cancel: Arc<AtomicBool>,
}

impl JobCtx {
    pub fn progress(&self, done: usize, total: usize, text: impl Into<String>) {
        let frac = if total == 0 { 0.0 } else { done as f64 / total as f64 };
        let _ = self.tx.send_blocking(JobMsg::Progress(frac, text.into()));
    }

    /// Activity of unknown length (the progress bar bounces).
    pub fn pulse(&self, text: impl Into<String>) {
        let _ = self.tx.send_blocking(JobMsg::Pulse(text.into()));
    }

    /// Adds a line to the message journal.
    pub fn log(&self, msg: impl Into<String>) {
        let _ = self.tx.send_blocking(JobMsg::Log(msg.into()));
    }

    /// Adds an error to the message journal.
    pub fn error(&self, msg: impl Into<String>) {
        let _ = self.tx.send_blocking(JobMsg::Error(msg.into()));
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Runs `f` on the GTK thread.
    pub fn ui(&self, f: impl FnOnce(&State) + Send + 'static) {
        let _ = self.tx.send_blocking(JobMsg::Ui(Box::new(f)));
    }

    /// Asks something on the GTK thread and blocks until `reply` is called.
    pub fn ask<R: Send + 'static>(
        &self,
        f: impl FnOnce(&State, Box<dyn FnOnce(R) + Send>) + Send + 'static,
    ) -> Option<R> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.ui(move |st| {
            f(
                st,
                Box::new(move |r| {
                    let _ = tx.send(r);
                }),
            )
        });
        rx.recv().ok()
    }

    /// Sleeps in small steps, returning early when cancelled.
    pub fn sleep(&self, secs: u32) {
        for _ in 0..secs * 10 {
            if self.cancelled() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

/// Runs `work` in a thread, with a line in the status bar; `done` gets its
/// result on the GTK thread. `books` are the books whose files it changes.
/// The operation waits for its turn after the ones it would conflict with.
pub fn run_job<T: Send + 'static>(
    state: &State,
    kind: JobKind,
    books: Vec<u64>,
    title: &str,
    work: impl FnOnce(JobCtx) -> T + Send + 'static,
    done: impl FnOnce(&State, T) + 'static,
) {
    let label = gtk::Label::builder()
        .xalign(0.0)
        .width_chars(30)
        .max_width_chars(60)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .build();
    let bar = gtk::ProgressBar::builder().valign(gtk::Align::Center).width_request(160).visible(false).build();
    let cancel_button = gtk::Button::builder().label(t!("Cancel")).build();
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.append(&label);
    row.append(&bar);
    row.append(&cancel_button);
    let job = Rc::new(Job {
        kind,
        books: books.into_iter().collect(),
        cancel: Arc::new(AtomicBool::new(false)),
        row,
        label,
        bar,
        start: RefCell::new(None),
    });
    job.set_text(&t!("{0} (waiting)", title));

    let weak_job = Rc::downgrade(&job);
    let weak = Rc::downgrade(state);
    cancel_button.connect_clicked(move |b| {
        let (Some(job), Some(st)) = (weak_job.upgrade(), weak.upgrade()) else { return };
        if job.started() {
            job.cancel.store(true, Ordering::Relaxed);
            b.set_sensitive(false);
        } else {
            st.forget_job(&job);
            start_ready(&st);
        }
    });

    let weak_job = Rc::downgrade(&job);
    let weak = Rc::downgrade(state);
    let title = title.to_string();
    job.start.replace(Some(Box::new(move || {
        if let (Some(job), Some(st)) = (weak_job.upgrade(), weak.upgrade()) {
            start(&st, job, title, work, done);
        }
    })));
    state.jobs.borrow_mut().push(job.clone());
    state.jobs_box.append(&job.row);
    start_ready(state);
}

fn start<T: Send + 'static>(
    state: &State,
    job: Rc<Job>,
    title: String,
    work: impl FnOnce(JobCtx) -> T + Send + 'static,
    done: impl FnOnce(&State, T) + 'static,
) {
    let (tx, rx) = async_channel::unbounded::<JobMsg>();
    let ctx = JobCtx { tx: tx.clone(), cancel: job.cancel.clone() };
    let result: Arc<Mutex<Option<T>>> = Arc::new(Mutex::new(None));
    let slot = result.clone();

    job.set_text(&title);
    job.bar.set_fraction(0.0);
    job.bar.set_visible(true);
    state.journal.title(&format!("▶ {}", title.trim_end_matches('…')));
    state.journal.set_active(true);
    let started = std::time::Instant::now();

    std::thread::spawn(move || {
        let r = work(ctx);
        *slot.lock().unwrap() = Some(r);
        let _ = tx.send_blocking(JobMsg::Finished);
    });

    let weak = Rc::downgrade(state);
    glib::spawn_future_local(async move {
        let mut done = Some(done);
        while let Ok(msg) = rx.recv().await {
            let Some(st) = weak.upgrade() else { return };
            match msg {
                JobMsg::Progress(f, text) => {
                    job.bar.set_fraction(f);
                    job.set_text(&format!("{title} {text}"));
                }
                JobMsg::Pulse(text) => {
                    job.bar.pulse();
                    job.set_text(&format!("{title} {text}"));
                }
                JobMsg::Log(msg) => st.journal.info(&msg),
                JobMsg::Error(msg) => st.journal.error(&msg),
                JobMsg::Ui(f) => f(&st),
                JobMsg::Finished => {
                    let name = title.trim_end_matches('…');
                    let secs = started.elapsed().as_secs_f64();
                    if job.cancel.load(Ordering::Relaxed) {
                        st.journal.title(&t!("■ {0}: cancelled after {1} s", name, format!("{secs:.1}")));
                    } else {
                        st.journal.title(&t!("■ {0}: finished in {1} s", name, format!("{secs:.1}")));
                    }
                    st.forget_job(&job);
                    update_status(&st);
                    let r = result.lock().unwrap().take();
                    if let (Some(r), Some(done)) = (r, done.take()) {
                        done(&st, r);
                    }
                    start_ready(&st);
                    return;
                }
            }
        }
    });
}
