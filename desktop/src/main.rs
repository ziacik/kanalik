mod catalog;
mod checker;
mod ui;

fn main() -> gtk::glib::ExitCode {
    ui::run()
}
