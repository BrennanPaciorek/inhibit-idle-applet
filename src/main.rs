mod app;
mod i18n;
mod screensaver;

fn main() -> cosmic::iced::Result {
    env_logger::init();

    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();

    i18n::init(&requested_languages);

    cosmic::applet::run::<app::AppModel>(())
}
