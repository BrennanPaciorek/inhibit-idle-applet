use crate::fl;
use crate::screensaver::ScreenSaverProxy;
use cosmic::iced::futures::executor::block_on;
use cosmic::iced::window::Id;
use cosmic::prelude::*;
use cosmic::widget;
use zbus::{Connection, Result};

#[derive(Default)]
pub struct AppModel {
    core: cosmic::Core,
    /// UI tracker for the inhibit idle toggle
    inhibit_idle: bool,
    /// Stores dbus connection for the duration of the application run
    dbus_connection: Option<Connection>,
    /// Tracks the actual state of whether we're inhibiting idle locks or not
    inhibit_idle_cookie: Option<u32>,
}

#[derive(Debug, Clone)]
pub enum Message {
    SetInhibitIdle(bool, Option<u32>),
    ToggleInhibitIdle(bool),
}

async fn create_dbus_connection() -> Result<Connection> {
    Ok(Connection::session().await?)
}

impl AppModel {
    const APP_NAME: &'static str = "com.github.brennanpaciorek.inhibit-idle-applet";
    const INHIBITED_ICON_NAME: &'static str =
        "com.github.brennanpaciorek.inhibit-idle-applet.Inhibited";
    const UNINHIBITED_ICON_NAME: &'static str =
        "com.github.brennanpaciorek.inhibit-idle-applet.Uninhibited";

    async fn toggle_idle(
        toggled: bool,
        connection: &Connection,
        cookie: &Option<u32>,
    ) -> Result<(bool, Option<u32>)> {
        if toggled {
            Ok(Self::inhibit(&connection, cookie).await?)
        } else {
            Ok(Self::uninhibit(&connection, cookie).await?)
        }
    }

    async fn inhibit(connection: &Connection, cookie: &Option<u32>) -> Result<(bool, Option<u32>)> {
        // Check config for a cookie, uninhibit
        if cookie.is_some() {
            Self::uninhibit(connection, cookie).await?;
        }

        let proxy = ScreenSaverProxy::new(&connection).await?;

        let cookie = proxy
            .inhibit(AppModel::APP_NAME, "User has enabled the idle inhibitor")
            .await?;
        Ok((true, Some(cookie)))
    }

    async fn uninhibit(
        connection: &Connection,
        idle_cookie: &Option<u32>,
    ) -> Result<(bool, Option<u32>)> {
        let proxy = ScreenSaverProxy::new(&connection).await?;

        if let Some(cookie) = idle_cookie {
            proxy.un_inhibit(*cookie).await?;
        }

        Ok((false, None))
    }
}

impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;

    type Flags = ();

    type Message = Message;

    const APP_ID: &'static str = AppModel::APP_NAME;

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        let connection = Some(
            block_on(async { create_dbus_connection().await })
                .expect("Failed to establish dbus connection"),
        );
        let app = AppModel {
            core,
            dbus_connection: connection,
            ..Default::default()
        };

        (app, Task::none())
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let icon_name = if self.inhibit_idle {
            Self::INHIBITED_ICON_NAME
        } else {
            Self::UNINHIBITED_ICON_NAME
        };
        self.core
            .applet
            .icon_button(icon_name)
            .on_press(Message::ToggleInhibitIdle(!self.inhibit_idle))
            .into()
    }

    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        let content_list = widget::list_column()
            .padding(5)
            .spacing(0)
            .add(widget::settings::item(
                fl!("inhibit-idle"),
                widget::toggler(self.inhibit_idle).on_toggle(Message::ToggleInhibitIdle),
            ));

        self.core.applet.popup_container(content_list).into()
    }

    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::ToggleInhibitIdle(toggled) => {
                log::debug!("Handling ToggleInhibitIdle({})", toggled);
                // Update the UI value, roll the change back on failure
                self.inhibit_idle = toggled;
                // TODO undo the unwrap at some point
                let connection = self.dbus_connection.clone().unwrap();
                let idle_cookie = self.inhibit_idle_cookie.clone();
                return cosmic::task::future(async move {
                    log::debug!("Starting ToggleInhibitIdle({}) task", toggled);
                    let message = match AppModel::toggle_idle(toggled, &connection, &idle_cookie)
                        .await
                    {
                        // We technically do not need to do anything in this case, but we should
                        // can re-set the value to the right value just in case.
                        Ok((idle, new_idle_cookie)) => {
                            Message::SetInhibitIdle(idle, new_idle_cookie)
                        }
                        Err(e) => {
                            // Get our logging info
                            let action = if toggled { "inhibit" } else { "uninhibit" }.to_string();
                            let description = match e.description() {
                                Some(d) => d,
                                None => "error description unavailable",
                            }
                            .to_string();
                            // Log the error
                            log::error!(
                                "Failed to {} idle due to dbus error: {}",
                                action,
                                description
                            );
                            // Correct the UI-facing value
                            // Concern: if an action fails due to something like the bus session
                            //   disconnecting, this may put the app in a bad state where our
                            //   model of the inhibit idle state does not reflect the state in
                            //   the idle manager.
                            //   The best option may be to log, crash, then send a notification,
                            //   then expect COSMIC to restart.
                            Message::SetInhibitIdle(!toggled, idle_cookie)
                        }
                    };
                    return message;
                });
            }
            Message::SetInhibitIdle(idle, idle_cookie) => {
                self.inhibit_idle = idle;
                self.inhibit_idle_cookie = idle_cookie;
            }
        }
        Task::none()
    }

    fn style(&self) -> Option<cosmic::iced_runtime::Appearance> {
        Some(cosmic::applet::style())
    }
}
