mod catalog;
mod checker;
mod ui;
mod tv_player;

fn main() -> gtk::glib::ExitCode {
    ui::run()
}
