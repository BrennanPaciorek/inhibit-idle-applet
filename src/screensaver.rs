// Interface with the zbus API provided by https://github.com/pop-os/cosmic-idle/blob/master/src/freedesktop_screensaver.rs
// https://specifications.freedesktop.org/idle-inhibit-spec/latest
// https://invent.kde.org/plasma/kscreenlocker/-/blob/master/dbus/org.freedesktop.ScreenSaver.xml
use zbus::{Result, proxy};

#[proxy(
    interface = "org.freedesktop.ScreenSaver",
    default_service = "org.freedesktop.ScreenSaver",
    default_path = "/org/freedesktop/ScreenSaver"
)]
pub trait ScreenSaver {
    async fn inhibit(&self, application_name: &str, reason_for_inhibit: &str) -> Result<u32>;
    async fn uninhibit(&self, cookie: u32) -> Result<()>;
}
