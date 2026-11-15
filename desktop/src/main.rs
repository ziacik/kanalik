mod catalog;
mod checker;
mod tv_player;
mod ui;

fn main() -> gtk::glib::ExitCode {
    ui::run()
}
