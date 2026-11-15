use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use mpv_engine::PlaybackEvent;

use crate::tv_player::TvPlayer;

use crate::catalog::{self, CatalogSource, Channel, LoadedCatalog};
use crate::checker::{self, CheckResult, CheckStatus};
use crate::report::{self, ReportEntry};

const WORKERS: usize = 3;

#[derive(Default)]
struct AppState {
    channels: Vec<Channel>,
    rows: HashMap<String, CheckerRow>,
    results: HashMap<String, ReportEntry>,
    active_id: Option<String>,
    source: CatalogSource,
    pending_source: Option<CatalogSource>,
    running: bool,
    completed: usize,
    online: usize,
    failed: usize,
    timed_out: usize,
}

struct CheckerRow {
    icon: gtk::Image,
    status: gtk::Label,
}

#[derive(Clone)]
struct Ui {
    window: adw::ApplicationWindow,
    header: adw::HeaderBar,
    window_title: adw::WindowTitle,
    main_toasts: adw::ToastOverlay,
    load_json_button: gtk::Button,
    default_catalog_button: gtk::Button,
    sidebar: gtk::Box,
    footer: gtk::Box,
    search: gtk::SearchEntry,
    channel_list: gtk::ListBox,
    video: Option<TvPlayer>,
    empty_page: adw::StatusPage,
    channel_title: gtk::Label,
    playback_status: gtk::Label,
    pause_button: gtk::Button,
    refresh_button: gtk::Button,
    checker_window: gtk::Window,
    checker_list: gtk::ListBox,
    checker_summary: gtk::Label,
    check_button: gtk::Button,
    copy_report_button: gtk::Button,
    export_report_button: gtk::Button,
    report_toasts: adw::ToastOverlay,
}

enum Message {
    Catalog(CatalogSource, Result<LoadedCatalog, String>),
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
        .default_width(1200)
        .default_height(780)
        .width_request(720)
        .height_request(450)
        .build();

    let header = adw::HeaderBar::new();
    let window_title = adw::WindowTitle::new("Kanálik", "Live television");
    header.set_title_widget(Some(&window_title));

    let refresh_button = gtk::Button::from_icon_name("view-refresh-symbolic");
    refresh_button.set_tooltip_text(Some("Reload channels"));
    header.pack_start(&refresh_button);

    let menu = gtk::MenuButton::new();
    menu.set_icon_name("open-menu-symbolic");
    menu.set_tooltip_text(Some("More options"));
    let popover = gtk::Popover::new();
    let menu_content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    menu_content.set_margin_top(8);
    menu_content.set_margin_bottom(8);
    menu_content.set_margin_start(8);
    menu_content.set_margin_end(8);
    let load_json_button = gtk::Button::with_label("Load channels JSON…");
    menu_content.append(&load_json_button);
    let default_catalog_button = gtk::Button::with_label("Use default catalog");
    default_catalog_button.set_sensitive(false);
    menu_content.append(&default_catalog_button);
    menu_content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let checker_menu_button = gtk::Button::with_label("Check stream availability…");
    menu_content.append(&checker_menu_button);
    popover.set_child(Some(&menu_content));
    menu.set_popover(Some(&popover));
    header.pack_end(&menu);

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Find channel"));
    search.set_margin_start(12);
    search.set_margin_end(12);
    search.set_margin_top(10);
    search.set_margin_bottom(10);

    let channel_list = gtk::ListBox::new();
    channel_list.add_css_class("navigation-sidebar");
    channel_list.set_selection_mode(gtk::SelectionMode::Single);
    channel_list.set_activate_on_single_click(true);

    let search_query = Rc::new(RefCell::new(String::new()));
    let query = search_query.clone();
    channel_list.set_filter_func(move |row| {
        let needle = query.borrow();
        row.downcast_ref::<adw::ActionRow>()
            .map(|action| action.title().to_lowercase().contains(needle.as_str()))
            .unwrap_or(true)
    });
    let search_list = channel_list.clone();
    search.connect_search_changed(move |entry| {
        *search_query.borrow_mut() = entry.text().to_lowercase();
        search_list.invalidate_filter();
    });

    let channel_scroll = gtk::ScrolledWindow::new();
    channel_scroll.set_vexpand(true);
    channel_scroll.set_child(Some(&channel_list));

    let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sidebar.set_size_request(240, -1);
    sidebar.append(&search);
    sidebar.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    sidebar.append(&channel_scroll);

    // GTK Video/GStreamer hangs on some otherwise playable IPTV/HLS streams.
    // libmpv draws directly into a GTK GLArea, including on Wayland.
    let embedded_player = TvPlayer::new();
    let embedded_error = embedded_player.as_ref().err().cloned();
    let video = embedded_player.ok();

    let empty_page = adw::StatusPage::builder()
        .icon_name("video-display-symbolic")
        .title("Kanálik")
        .description("Select a channel to start watching")
        .build();

    let video_overlay = gtk::Overlay::new();
    if let Some(ref embedded) = video {
        video_overlay.set_child(Some(&embedded.area));
    } else {
        let unavailable = gtk::Box::new(gtk::Orientation::Vertical, 0);
        unavailable.set_hexpand(true);
        unavailable.set_vexpand(true);
        video_overlay.set_child(Some(&unavailable));
    }
    video_overlay.add_overlay(&empty_page);
    video_overlay.set_hexpand(true);
    video_overlay.set_vexpand(true);
    video_overlay.add_css_class("video-stage");

    let channel_title = gtk::Label::new(Some("No channel selected"));
    channel_title.set_xalign(0.0);
    channel_title.set_hexpand(true);
    channel_title.add_css_class("title-3");

    let playback_status = gtk::Label::new(Some("Choose a channel from the list"));
    playback_status.set_xalign(0.0);
    playback_status.add_css_class("dim-label");

    let channel_info = gtk::Box::new(gtk::Orientation::Vertical, 3);
    channel_info.set_hexpand(true);
    channel_info.append(&channel_title);
    channel_info.append(&playback_status);

    let previous = gtk::Button::from_icon_name("media-skip-backward-symbolic");
    previous.set_tooltip_text(Some("Previous channel (Up)"));
    let pause_button = gtk::Button::from_icon_name("media-playback-pause-symbolic");
    pause_button.set_tooltip_text(Some("Pause / Resume (Space)"));
    let next = gtk::Button::from_icon_name("media-skip-forward-symbolic");
    next.set_tooltip_text(Some("Next channel (Down)"));
    let external = gtk::Button::from_icon_name("video-display-symbolic");
    external.set_tooltip_text(Some("Open current channel in mpv"));
    let fullscreen = gtk::Button::from_icon_name("view-fullscreen-symbolic");
    fullscreen.set_tooltip_text(Some("Fullscreen (F11, Esc to leave)"));

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 9);
    footer.set_margin_start(16);
    footer.set_margin_end(16);
    footer.set_margin_top(12);
    footer.set_margin_bottom(12);
    footer.append(&channel_info);
    footer.append(&previous);
    footer.append(&pause_button);
    footer.append(&next);
    footer.append(&external);
    footer.append(&fullscreen);

    let player = gtk::Box::new(gtk::Orientation::Vertical, 0);
    player.set_hexpand(true);
    player.set_vexpand(true);
    player.append(&video_overlay);
    player.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    player.append(&footer);

    let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    paned.set_start_child(Some(&sidebar));
    paned.set_end_child(Some(&player));
    paned.set_position(285);
    paned.set_resize_start_child(false);
    paned.set_shrink_start_child(false);
    paned.set_vexpand(true);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&header);
    content.append(&paned);
    let main_toasts = adw::ToastOverlay::new();
    main_toasts.set_child(Some(&content));
    window.set_content(Some(&main_toasts));

    let checker_window = gtk::Window::builder()
        .title("Stream Checker · Kanálik")
        .transient_for(&window)
        .default_width(780)
        .default_height(650)
        .build();
    let checker_header = adw::HeaderBar::new();
    checker_header.set_title_widget(Some(&adw::WindowTitle::new(
        "Stream Checker",
        "Diagnostic tool",
    )));
    let check_button = gtk::Button::with_label("Check All");
    check_button.add_css_class("suggested-action");
    check_button.set_sensitive(false);
    checker_header.pack_end(&check_button);

    let copy_report_button = gtk::Button::from_icon_name("edit-copy-symbolic");
    copy_report_button.set_tooltip_text(Some("Copy full CSV report to clipboard"));
    copy_report_button.set_sensitive(false);
    checker_header.pack_start(&copy_report_button);

    let export_report_button = gtk::Button::with_label("Export CSV…");
    export_report_button.set_sensitive(false);
    checker_header.pack_start(&export_report_button);

    let checker_list = gtk::ListBox::new();
    checker_list.add_css_class("boxed-list");
    checker_list.set_selection_mode(gtk::SelectionMode::None);
    let checker_scroll = gtk::ScrolledWindow::new();
    checker_scroll.set_vexpand(true);
    checker_scroll.set_child(Some(&checker_list));
    let checker_summary = gtk::Label::new(Some("Load a catalog to start checking"));
    checker_summary.set_xalign(0.0);
    checker_summary.add_css_class("dim-label");
    checker_summary.set_margin_start(16);
    checker_summary.set_margin_end(16);
    checker_summary.set_margin_top(10);
    checker_summary.set_margin_bottom(10);

    let checker_content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    checker_content.set_margin_start(12);
    checker_content.set_margin_end(12);
    checker_content.append(&checker_header);
    checker_content.append(&checker_scroll);
    checker_content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    checker_content.append(&checker_summary);
    let report_toasts = adw::ToastOverlay::new();
    report_toasts.set_child(Some(&checker_content));
    checker_window.set_child(Some(&report_toasts));
    checker_window.connect_close_request(|window| {
        window.set_visible(false);
        glib::Propagation::Stop
    });

    let css = gtk::CssProvider::new();
    css.load_from_string(".video-stage { background-color: #000; }");
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let ui = Ui {
        window,
        header,
        window_title,
        main_toasts,
        load_json_button,
        default_catalog_button,
        sidebar,
        footer,
        search,
        channel_list,
        video,
        empty_page,
        channel_title,
        playback_status,
        pause_button,
        refresh_button,
        checker_window,
        checker_list,
        checker_summary,
        check_button,
        copy_report_button,
        export_report_button,
        report_toasts,
    };
    if let Some(error) = embedded_error {
        ui.playback_status.set_text(&format!(
            "{error}. External mpv playback is still available."
        ));
    }
    let state = Rc::new(RefCell::new(AppState {
        source: catalog::load_saved_source(),
        ..AppState::default()
    }));
    let (sender, receiver) = mpsc::channel();

    {
        let ui = ui.clone();
        let state = state.clone();
        let sender = sender.clone();
        let menu_popover = popover.clone();
        ui.load_json_button.clone().connect_clicked(move |_| {
            menu_popover.popdown();
            let dialog = gtk::FileDialog::new();
            dialog.set_title("Choose channels JSON");
            let ui = ui.clone();
            let state = state.clone();
            let sender = sender.clone();
            let parent = ui.window.clone();
            dialog.open(
                Some(&parent),
                None::<&gio::Cancellable>,
                move |result| {
                    if let Ok(file) = result {
                        if let Some(path) = file.path() {
                            request_catalog_load(
                                &ui,
                                &state,
                                &sender,
                                CatalogSource::Local(path),
                            );
                        } else {
                            ui.main_toasts.add_toast(adw::Toast::new(
                                "Please select a local JSON file",
                            ));
                        }
                    }
                },
            );
        });
    }

    {
        let ui = ui.clone();
        let state = state.clone();
        let sender = sender.clone();
        let menu_popover = popover.clone();
        ui.default_catalog_button.clone().connect_clicked(move |_| {
            menu_popover.popdown();
            request_catalog_load(&ui, &state, &sender, CatalogSource::Remote);
        });
    }

    {
        let ui = ui.clone();
        let state = state.clone();
        ui.copy_report_button.clone().connect_clicked(move |_| {
            let csv = {
                let state = state.borrow();
                report::to_csv(&state.channels, &state.results, state.running)
            };
            if let Some(display) = gdk::Display::default() {
                display.clipboard().set_text(&csv);
                ui.report_toasts
                    .add_toast(adw::Toast::new("Stream report copied"));
            }
        });
    }

    {
        let ui = ui.clone();
        let state = state.clone();
        ui.export_report_button.clone().connect_clicked(move |_| {
            let csv = {
                let state = state.borrow();
                report::to_csv(&state.channels, &state.results, state.running)
            };
            let dialog = gtk::FileDialog::new();
            dialog.set_title("Export stream report");
            dialog.set_initial_name(Some("kanalik-stream-report.csv"));
            let toasts = ui.report_toasts.clone();
            dialog.save(
                Some(&ui.checker_window),
                None::<&gio::Cancellable>,
                move |result| {
                    if let Ok(file) = result {
                        let message = match file.path() {
                            Some(path) => match fs::write(path, &csv) {
                                Ok(()) => "Stream report exported".to_owned(),
                                Err(error) => format!("Could not export report: {error}"),
                            },
                            None => "Cannot export to this non-local location".to_owned(),
                        };
                        toasts.add_toast(adw::Toast::new(&message));
                    }
                },
            );
        });
    }

    {
        let ui = ui.clone();
        checker_menu_button.connect_clicked(move |_| {
            popover.popdown();
            ui.checker_window.present();
        });
    }

    {
        let ui = ui.clone();
        let state = state.clone();
        ui.channel_list
            .clone()
            .connect_row_activated(move |_, row| {
                let channel = {
                    let state = state.borrow();
                    state
                        .channels
                        .iter()
                        .find(|channel| channel.id.as_str() == row.widget_name().as_str())
                        .cloned()
                };
                if let Some(channel) = channel {
                    play_channel(&ui, &state, &channel);
                }
            });
    }

    {
        let ui = ui.clone();
        let state = state.clone();
        previous.connect_clicked(move |_| change_channel(&ui, &state, -1));
    }
    {
        let ui = ui.clone();
        let state = state.clone();
        next.connect_clicked(move |_| change_channel(&ui, &state, 1));
    }
    {
        let ui = ui.clone();
        ui.pause_button
            .clone()
            .connect_clicked(move |_| toggle_pause(&ui));
    }
    {
        let ui = ui.clone();
        let state = state.clone();
        external.connect_clicked(move |_| {
            let channel = {
                let state = state.borrow();
                state.active_id.as_ref().and_then(|id| {
                    state
                        .channels
                        .iter()
                        .find(|channel| &channel.id == id)
                        .cloned()
                })
            };
            if let Some(channel) = channel
                && let Err(error) = launch_mpv(&channel)
            {
                ui.playback_status.set_text(&error);
            }
        });
    }
    {
        let ui = ui.clone();
        fullscreen.connect_clicked(move |_| toggle_fullscreen(&ui));
    }
    {
        let ui = ui.clone();
        let state = state.clone();
        let controller = gtk::EventControllerKey::new();
        let keyboard_window = ui.window.clone();
        controller.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::F11 {
                toggle_fullscreen(&ui);
                return glib::Propagation::Stop;
            }
            if key == gdk::Key::Escape && ui.window.is_fullscreen() {
                toggle_fullscreen(&ui);
                return glib::Propagation::Stop;
            }
            if ui.search.has_focus() {
                return glib::Propagation::Proceed;
            }
            match key {
                gdk::Key::Up => change_channel(&ui, &state, -1),
                gdk::Key::Down => change_channel(&ui, &state, 1),
                gdk::Key::space => toggle_pause(&ui),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        keyboard_window.add_controller(controller);
    }

    {
        let sender = sender.clone();
        let ui = ui.clone();
        let state = state.clone();
        ui.refresh_button.clone().connect_clicked(move |_| {
            let source = state.borrow().source.clone();
            request_catalog_load(&ui, &state, &sender, source);
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
            state.timed_out = 0;
            state.results.clear();
            for row in state.rows.values() {
                set_status(row, "Queued", "content-loading-symbolic", None, None);
            }
            ui.check_button.set_sensitive(false);
            update_catalog_actions(&ui, &state);
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

    {
        let ui = ui.clone();
        glib::timeout_add_local(Duration::from_millis(500), move || {
            update_playback_status(&ui);
            glib::ControlFlow::Continue
        });
    }

    let source = state.borrow().source.clone();
    request_catalog_load(&ui, &state, &sender, source);
    ui.window.present();
}

fn handle_message(message: Message, ui: &Ui, state: &Rc<RefCell<AppState>>) {
    match message {
        Message::Catalog(source, Ok(loaded)) => {
            let mut state_mut = state.borrow_mut();
            if state_mut.pending_source.as_ref() != Some(&source) {
                return;
            }
            state_mut.pending_source = None;
            state_mut.source = source.clone();
            while let Some(child) = ui.channel_list.first_child() {
                ui.channel_list.remove(&child);
            }
            while let Some(child) = ui.checker_list.first_child() {
                ui.checker_list.remove(&child);
            }

            state_mut.rows.clear();
            state_mut.results.clear();
            state_mut.channels = loaded.channels;
            state_mut.completed = 0;
            state_mut.online = 0;
            state_mut.failed = 0;
            state_mut.timed_out = 0;
            for channel in state_mut.channels.clone() {
                let row = adw::ActionRow::builder()
                    .title(&channel.name)
                    .activatable(true)
                    .build();
                row.set_widget_name(&channel.id);
                ui.channel_list.append(&row);
                let checker = make_checker_row(&channel);
                ui.checker_list.append(&checker.0);
                state_mut.rows.insert(channel.id.clone(), checker.1);
            }
            ui.channel_list.invalidate_filter();
            ui.check_button.set_sensitive(true);
            ui.copy_report_button.set_sensitive(true);
            ui.export_report_button.set_sensitive(true);
            update_catalog_actions(ui, &state_mut);
            ui.window_title.set_subtitle(&loaded.source);
            ui.checker_summary.set_text(&format!(
                "{} channels · {} · Not checked",
                state_mut.channels.len(),
                loaded.source,
            ));
            let selected = state_mut.active_id.clone().or_else(last_channel_id);
            let next_channel = selected
                .as_ref()
                .and_then(|id| state_mut.channels.iter().find(|channel| &channel.id == id))
                .or_else(|| state_mut.channels.first())
                .cloned();
            drop(state_mut);
            if let Err(error) = catalog::save_source(&source) {
                ui.main_toasts.add_toast(adw::Toast::new(&error));
            }
            if let Some(channel) = next_channel {
                play_channel(ui, state, &channel);
            }
        }
        Message::Catalog(source, Err(error)) => {
            let mut state = state.borrow_mut();
            if state.pending_source.as_ref() != Some(&source) {
                return;
            }
            state.pending_source = None;
            update_catalog_actions(ui, &state);
            ui.check_button.set_sensitive(!state.channels.is_empty());
            ui.copy_report_button.set_sensitive(!state.channels.is_empty());
            ui.export_report_button.set_sensitive(!state.channels.is_empty());
            ui.main_toasts.add_toast(adw::Toast::new(&error));
            if state.channels.is_empty() {
                ui.playback_status.set_text(&error);
            }
        }
        Message::Checking(id) => {
            if let Some(row) = state.borrow().rows.get(&id) {
                set_status(row, "Checking…", "content-loading-symbolic", None, None);
            }
        }
        Message::Checked(id, result) => {
            let mut state = state.borrow_mut();
            state.completed += 1;
            if let Some(row) = state.rows.get(&id) {
                let duration = format!("{:.1} s", result.elapsed.as_secs_f64());
                match result.status {
                    CheckStatus::Online => set_status(
                        row,
                        &format!("Online · {duration}"),
                        "emblem-ok-symbolic",
                        Some("success"),
                        Some(&result.details),
                    ),
                    CheckStatus::TimedOut => set_status(
                        row,
                        "Unknown · Timeout",
                        "dialog-warning-symbolic",
                        Some("warning"),
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
            match result.status {
                CheckStatus::Online => state.online += 1,
                CheckStatus::Failed => state.failed += 1,
                CheckStatus::TimedOut => state.timed_out += 1,
            }
            let checked_at = glib::DateTime::now_local()
                .ok()
                .and_then(|date| date.format("%Y-%m-%d %H:%M:%S %z").ok())
                .map(|date| date.to_string())
                .unwrap_or_default();
            state.results.insert(
                id,
                ReportEntry {
                    status: result.status,
                    elapsed: result.elapsed,
                    checked_at,
                    details: result.details,
                },
            );
            update_summary(ui, &state);
        }
        Message::Finished => {
            let mut state = state.borrow_mut();
            state.running = false;
            ui.check_button.set_sensitive(true);
            update_catalog_actions(ui, &state);
            update_summary(ui, &state);
        }
    }
}

fn make_checker_row(channel: &Channel) -> (adw::ActionRow, CheckerRow) {
    let row = adw::ActionRow::builder()
        .title(&channel.name)
        .subtitle(&channel.url)
        .build();
    let icon = gtk::Image::from_icon_name("dialog-question-symbolic");
    row.add_prefix(&icon);
    let status = gtk::Label::new(Some("Not checked"));
    status.set_valign(gtk::Align::Center);
    status.add_css_class("dim-label");
    row.add_suffix(&status);
    let copy = gtk::Button::from_icon_name("edit-copy-symbolic");
    copy.add_css_class("flat");
    copy.set_valign(gtk::Align::Center);
    copy.set_tooltip_text(Some("Copy URL"));
    let url = channel.url.clone();
    copy.connect_clicked(move |_| {
        if let Some(display) = gdk::Display::default() {
            display.clipboard().set_text(&url);
        }
    });
    row.add_suffix(&copy);
    (row, CheckerRow { icon, status })
}

fn set_status(
    row: &CheckerRow,
    text: &str,
    icon: &str,
    style: Option<&str>,
    details: Option<&str>,
) {
    row.icon.set_icon_name(Some(icon));
    row.status.set_text(text);
    row.status.set_tooltip_text(details);
    for class in ["dim-label", "success", "error", "warning"] {
        row.status.remove_css_class(class);
    }
    row.status.add_css_class(style.unwrap_or("dim-label"));
}

fn update_summary(ui: &Ui, state: &AppState) {
    let progress = if state.running { " · Checking…" } else { "" };
    ui.checker_summary.set_text(&format!(
        "{}/{} checked · {} online · {} failed · {} unknown{}",
        state.completed,
        state.channels.len(),
        state.online,
        state.failed,
        state.timed_out,
        progress,
    ));
}

fn play_channel(ui: &Ui, state: &Rc<RefCell<AppState>>, channel: &Channel) {
    state.borrow_mut().active_id = Some(channel.id.clone());
    save_last_channel(&channel.id);
    ui.channel_title.set_text(&channel.name);
    ui.empty_page.set_visible(false);

    if let Some(player) = &ui.video {
        ui.playback_status.set_text("Connecting…");
        if let Err(error) = player.load(channel) {
            ui.playback_status.set_text(&error);
        }
    } else {
        match launch_mpv(channel) {
            Ok(()) => ui.playback_status.set_text("Playing in external mpv"),
            Err(error) => ui.playback_status.set_text(&error),
        }
    }
    ui.pause_button
        .set_icon_name("media-playback-pause-symbolic");
    let mut index = 0;
    while let Some(row) = ui.channel_list.row_at_index(index) {
        if row.widget_name().as_str() == channel.id {
            ui.channel_list.select_row(Some(&row));
            break;
        }
        index += 1;
    }
}

fn change_channel(ui: &Ui, state: &Rc<RefCell<AppState>>, offset: isize) {
    let next = {
        let state = state.borrow();
        if state.channels.is_empty() {
            return;
        }
        let current = state
            .active_id
            .as_ref()
            .and_then(|id| state.channels.iter().position(|channel| &channel.id == id))
            .unwrap_or(0);
        let index = (current as isize + offset).rem_euclid(state.channels.len() as isize) as usize;
        state.channels[index].clone()
    };
    play_channel(ui, state, &next);
}

fn toggle_pause(ui: &Ui) {
    let Some(player) = &ui.video else { return };
    match player.toggle_pause() {
        Ok(paused) => {
            ui.pause_button.set_icon_name(if paused {
                "media-playback-start-symbolic"
            } else {
                "media-playback-pause-symbolic"
            });
            ui.playback_status
                .set_text(if paused { "Paused" } else { "Live" });
        }
        Err(error) => ui.playback_status.set_text(&error),
    }
}

fn toggle_fullscreen(ui: &Ui) {
    let fullscreen = !ui.window.is_fullscreen();
    ui.sidebar.set_visible(!fullscreen);
    ui.header.set_visible(!fullscreen);
    ui.footer.set_visible(!fullscreen);
    if fullscreen {
        ui.window.fullscreen();
    } else {
        ui.window.unfullscreen();
    }
}

fn update_playback_status(ui: &Ui) {
    let Some(player) = &ui.video else { return };
    if let Some(error) = player.error() {
        ui.playback_status.set_text(&error);
        return;
    }
    for event in player.events() {
        match event {
            PlaybackEvent::Loaded => ui.playback_status.set_text("Buffering…"),
            PlaybackEvent::PlaybackRestart => {
                ui.playback_status
                    .set_text(if player.is_paused() { "Paused" } else { "Live" });
            }
            PlaybackEvent::Failed { message, .. } => {
                ui.playback_status
                    .set_text(&format!("Playback failed: {message}"));
            }
            PlaybackEvent::Ended { .. } => ui.playback_status.set_text("Playback ended"),
            _ => {}
        }
    }
}

fn update_catalog_actions(ui: &Ui, state: &AppState) {
    let ready = !state.running && state.pending_source.is_none();
    ui.refresh_button.set_sensitive(ready);
    ui.load_json_button.set_sensitive(ready);
    ui.default_catalog_button
        .set_sensitive(ready && state.source != CatalogSource::Remote);
}

fn request_catalog_load(
    ui: &Ui,
    state: &Rc<RefCell<AppState>>,
    sender: &mpsc::Sender<Message>,
    source: CatalogSource,
) {
    let mut current = state.borrow_mut();
    if current.running || current.pending_source.is_some() {
        return;
    }
    current.pending_source = Some(source.clone());
    update_catalog_actions(ui, &current);
    ui.check_button.set_sensitive(false);
    ui.playback_status.set_text("Loading channel catalog…");
    drop(current);
    spawn_catalog_load(sender.clone(), source);
}

fn spawn_catalog_load(sender: mpsc::Sender<Message>, source: CatalogSource) {
    thread::spawn(move || {
        let loaded = catalog::load_catalog(&source);
        let _ = sender.send(Message::Catalog(source, loaded));
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
    checker::configure_mpv(&mut command, channel);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|err| format!("Cannot launch mpv: {err}"))?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn last_channel_path() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("kanalik-gnome").join("last-channel")
}

fn save_last_channel(id: &str) {
    let path = last_channel_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, id);
}

fn last_channel_id() -> Option<String> {
    fs::read_to_string(last_channel_path())
        .ok()
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty())
}
