use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;

use crate::catalog::{self, Channel, LoadedCatalog};
use crate::checker::{self, CheckResult, CheckStatus};

const WORKERS: usize = 6;

#[derive(Default)]
struct AppState {
    channels: Vec<Channel>,
    rows: HashMap<String, ChannelRow>,
    running: bool,
    completed: usize,
    online: usize,
    failed: usize,
}

struct ChannelRow {
    icon: gtk::Image,
    status: gtk::Label,
}

#[derive(Clone)]
struct Ui {
    window: adw::ApplicationWindow,
    list: gtk::ListBox,
    stack: gtk::Stack,
    summary: gtk::Label,
    check_button: gtk::Button,
    refresh_button: gtk::Button,
}

enum Message {
    Catalog(Result<LoadedCatalog, String>),
    Checking(String),
    Checked(String, CheckResult),
    Finished,
}

pub fn run() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id("com.github.ziacik.Kanalik")
        .build();
    app.connect_activate(build_ui);
    app.run()
}

fn build_ui(app: &adw::Application) {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Kanálik")
        .default_width(980)
        .default_height(760)
        .width_request(620)
        .height_request(420)
        .build();

    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::WindowTitle::new(
        "Kanálik",
        "GNOME stream checker",
    )));

    let refresh_button = gtk::Button::from_icon_name("view-refresh-symbolic");
    refresh_button.set_tooltip_text(Some("Reload channel catalog"));
    header.pack_start(&refresh_button);

    let check_button = gtk::Button::with_label("Check All");
    check_button.add_css_class("suggested-action");
    check_button.set_sensitive(false);
    header.pack_end(&check_button);

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search channels"));
    search.set_hexpand(true);

    let search_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    search_box.set_margin_start(18);
    search_box.set_margin_end(18);
    search_box.set_margin_top(12);
    search_box.set_margin_bottom(12);
    search_box.append(&search);

    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::None);

    let query = Rc::new(RefCell::new(String::new()));
    let filter_query = query.clone();
    list.set_filter_func(move |row| {
        let needle = filter_query.borrow();
        row.downcast_ref::<adw::ActionRow>()
            .map(|action| action.title().to_lowercase().contains(needle.as_str()))
            .unwrap_or(true)
    });
    let search_list = list.clone();
    search.connect_search_changed(move |entry| {
        *query.borrow_mut() = entry.text().to_lowercase();
        search_list.invalidate_filter();
    });

    let channel_container = gtk::Box::new(gtk::Orientation::Vertical, 0);
    channel_container.set_margin_start(18);
    channel_container.set_margin_end(18);
    channel_container.set_margin_top(6);
    channel_container.set_margin_bottom(18);
    channel_container.append(&list);

    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_child(Some(&channel_container));

    let empty_page = adw::StatusPage::builder()
        .icon_name("video-display-symbolic")
        .title("Loading channels…")
        .description("Downloading the current Kanálik catalog")
        .build();
    let stack = gtk::Stack::new();
    stack.set_vexpand(true);
    stack.add_named(&empty_page, Some("empty"));
    stack.add_named(&scrolled, Some("channels"));
    stack.set_visible_child_name("empty");

    let summary = gtk::Label::new(Some("Loading channel catalog…"));
    summary.set_xalign(0.0);
    summary.add_css_class("dim-label");
    summary.set_margin_start(18);
    summary.set_margin_end(18);
    summary.set_margin_top(8);
    summary.set_margin_bottom(12);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&header);
    content.append(&search_box);
    content.append(&stack);
    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    content.append(&summary);
    window.set_content(Some(&content));

    let ui = Ui {
        window,
        list,
        stack,
        summary,
        check_button,
        refresh_button,
    };
    let state = Rc::new(RefCell::new(AppState::default()));
    let (sender, receiver) = mpsc::channel();

    {
        let sender = sender.clone();
        let ui = ui.clone();
        let state = state.clone();
        ui.refresh_button.clone().connect_clicked(move |_| {
            if state.borrow().running {
                return;
            }
            ui.refresh_button.set_sensitive(false);
            ui.check_button.set_sensitive(false);
            ui.summary.set_text("Refreshing channel catalog…");
            spawn_catalog_load(sender.clone());
        });
    }

    {
        let sender = sender.clone();
        let ui = ui.clone();
        let state = state.clone();
        ui.check_button.clone().connect_clicked(move |_| {
            let mut state = state.borrow_mut();
            if state.running || state.channels.is_empty() {
                return;
            }
            state.running = true;
            state.completed = 0;
            state.online = 0;
            state.failed = 0;
            for row in state.rows.values() {
                set_status(row, "Queued", "content-loading-symbolic", None, None);
            }
            ui.check_button.set_sensitive(false);
            ui.refresh_button.set_sensitive(false);
            update_summary(&ui, &state);
            spawn_checks(state.channels.clone(), sender.clone());
        });
    }

    {
        let ui = ui.clone();
        let state = state.clone();
        glib::timeout_add_local(Duration::from_millis(80), move || {
            while let Ok(message) = receiver.try_recv() {
                handle_message(message, &ui, &state);
            }
            glib::ControlFlow::Continue
        });
    }

    spawn_catalog_load(sender);
    ui.window.present();
}

fn handle_message(message: Message, ui: &Ui, state: &Rc<RefCell<AppState>>) {
    match message {
        Message::Catalog(Ok(loaded)) => {
            let mut state = state.borrow_mut();
            while let Some(child) = ui.list.first_child() {
                ui.list.remove(&child);
            }

            state.rows.clear();
            state.channels = loaded.channels;
            state.completed = 0;
            state.online = 0;
            state.failed = 0;
            for channel in state.channels.clone() {
                let row = make_channel_row(&channel, &ui.summary);
                ui.list.append(&row.0);
                state.rows.insert(channel.id.clone(), row.1);
            }

            ui.list.invalidate_filter();
            ui.stack.set_visible_child_name("channels");
            ui.check_button.set_sensitive(true);
            ui.refresh_button.set_sensitive(true);
            ui.summary.set_text(&format!(
                "{} channels · {} · Not checked",
                state.channels.len(),
                loaded.source,
            ));
        }
        Message::Catalog(Err(error)) => {
            ui.summary.set_text(&error);
            ui.refresh_button.set_sensitive(true);
            ui.check_button
                .set_sensitive(!state.borrow().channels.is_empty());
        }
        Message::Checking(id) => {
            if let Some(row) = state.borrow().rows.get(&id) {
                set_status(row, "Checking…", "content-loading-symbolic", None, None);
            }
        }
        Message::Checked(id, result) => {
            let mut state = state.borrow_mut();
            state.completed += 1;
            let success = result.status == CheckStatus::Online;
            if success {
                state.online += 1;
            } else {
                state.failed += 1;
            }
            if let Some(row) = state.rows.get(&id) {
                let duration = format!("{:.1} s", result.elapsed.as_secs_f64());
                match result.status {
                    CheckStatus::Online => set_status(
                        row,
                        &format!("OK · {duration}"),
                        "emblem-ok-symbolic",
                        Some("success"),
                        Some(&result.details),
                    ),
                    CheckStatus::TimedOut => set_status(
                        row,
                        "Timeout",
                        "dialog-error-symbolic",
                        Some("error"),
                        Some(&result.details),
                    ),
                    CheckStatus::Failed => set_status(
                        row,
                        &format!("Failed · {duration}"),
                        "dialog-error-symbolic",
                        Some("error"),
                        Some(&result.details),
                    ),
                }
            }
            update_summary(ui, &state);
        }
        Message::Finished => {
            let mut state = state.borrow_mut();
            state.running = false;
            ui.check_button.set_sensitive(true);
            ui.refresh_button.set_sensitive(true);
            update_summary(ui, &state);
        }
    }
}

fn make_channel_row(channel: &Channel, summary: &gtk::Label) -> (adw::ActionRow, ChannelRow) {
    let row = adw::ActionRow::builder()
        .title(&channel.name)
        .subtitle(&channel.url)
        .build();
    row.set_activatable(false);

    let icon = gtk::Image::from_icon_name("dialog-question-symbolic");
    row.add_prefix(&icon);

    let status = gtk::Label::new(Some("Not checked"));
    status.add_css_class("dim-label");
    status.set_valign(gtk::Align::Center);
    row.add_suffix(&status);

    let copy = gtk::Button::from_icon_name("edit-copy-symbolic");
    copy.add_css_class("flat");
    copy.set_valign(gtk::Align::Center);
    copy.set_tooltip_text(Some("Copy stream URL"));
    let url = channel.url.clone();
    copy.connect_clicked(move |_| {
        if let Some(display) = gtk::gdk::Display::default() {
            display.clipboard().set_text(&url);
        }
    });
    row.add_suffix(&copy);

    let play = gtk::Button::from_icon_name("media-playback-start-symbolic");
    play.add_css_class("flat");
    play.set_valign(gtk::Align::Center);
    play.set_tooltip_text(Some("Play in mpv"));
    let channel = channel.clone();
    let summary = summary.clone();
    play.connect_clicked(move |_| {
        if let Err(error) = launch_mpv(&channel) {
            summary.set_text(&error);
        }
    });
    row.add_suffix(&play);

    (row, ChannelRow { icon, status })
}

fn set_status(
    row: &ChannelRow,
    text: &str,
    icon: &str,
    style: Option<&str>,
    details: Option<&str>,
) {
    row.icon.set_icon_name(Some(icon));
    row.status.set_text(text);
    row.status.set_tooltip_text(details);
    for class in ["dim-label", "success", "error"] {
        row.status.remove_css_class(class);
    }
    row.status.add_css_class(style.unwrap_or("dim-label"));
}

fn update_summary(ui: &Ui, state: &AppState) {
    let progress = if state.running { " · Checking…" } else { "" };
    ui.summary.set_text(&format!(
        "{}/{} checked · {} online · {} failed{}",
        state.completed,
        state.channels.len(),
        state.online,
        state.failed,
        progress,
    ));
}

fn spawn_catalog_load(sender: mpsc::Sender<Message>) {
    thread::spawn(move || {
        let _ = sender.send(Message::Catalog(catalog::load_catalog()));
    });
}

fn spawn_checks(channels: Vec<Channel>, sender: mpsc::Sender<Message>) {
    thread::spawn(move || {
        let worker_count = WORKERS.min(channels.len());
        let pending = Arc::new(Mutex::new(VecDeque::from(channels)));
        let mut workers = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let pending = pending.clone();
            let sender = sender.clone();
            workers.push(thread::spawn(move || {
                loop {
                    let next = pending.lock().ok().and_then(|mut queue| queue.pop_front());
                    let Some(channel) = next else { break };
                    let _ = sender.send(Message::Checking(channel.id.clone()));
                    let result = checker::check(&channel);
                    let _ = sender.send(Message::Checked(channel.id, result));
                }
            }));
        }
        for worker in workers {
            let _ = worker.join();
        }
        let _ = sender.send(Message::Finished);
    });
}

fn launch_mpv(channel: &Channel) -> Result<(), String> {
    let mut command = Command::new("mpv");
    command.arg(format!("--title=Kanálik · {}", channel.name));
    let mut other_headers = Vec::new();
    for (name, value) in &channel.headers {
        if name.eq_ignore_ascii_case("referer") {
            command.arg(format!("--referrer={value}"));
        } else if name.eq_ignore_ascii_case("user-agent") {
            command.arg(format!("--user-agent={value}"));
        } else {
            other_headers.push(format!("{name}: {value}"));
        }
    }
    if !other_headers.is_empty() {
        command.arg(format!("--http-header-fields={}", other_headers.join(",")));
    }
    command
        .arg(&channel.url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|err| format!("Cannot launch mpv: {err}. Install mpv first."))?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
