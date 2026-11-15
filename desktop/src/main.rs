mod catalog;
mod checker;
mod report;
mod tv_player;
mod ui;

fn main() -> gtk::glib::ExitCode {
    ui::run()
}
