// SPDX-License-Identifier: MPL-2.0
use crate::fl;
use crate::screensaver::ScreenSaverProxy;
use cosmic::iced::futures::executor::block_on;
use cosmic::iced::window::Id;
use cosmic::prelude::*;
use cosmic::widget;
use zbus::{Connection, Result};

/// The application model stores app-specific state used to describe its interface and
/// drive its logic.
#[derive(Default)]
pub struct AppModel {
    /// Application state which is managed by the COSMIC runtime.
    core: cosmic::Core,
    /// UI tracker for the inhibit idle toggle
    inhibit_idle: bool,
    /// Stores dbus connection for the duration of the application run
    dbus_connection: Option<Connection>,
    /// Tracks the actual state of whether we're inhibiting idle or not
    inhibit_idle_cookie: Option<u32>,
}

/// Messages emitted by the application and its widgets.
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

/// Create a COSMIC application from the app model
impl cosmic::Application for AppModel {
    /// The async executor that will be used to run your application's commands.
    type Executor = cosmic::executor::Default;

    /// Data that your application receives to its init method.
    type Flags = ();

    /// Messages which the application and its widgets will emit.
    type Message = Message;

    /// Unique identifier in RDNN (reverse domain name notation) format.
    const APP_ID: &'static str = AppModel::APP_NAME;

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    /// Initializes the application with any given flags and startup commands.
    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        let connection = Some(
            block_on(async { create_dbus_connection().await })
                .expect("Failed to establish dbus connection"),
        );
        // Construct the app model with the runtime's core.
        let app = AppModel {
            core,
            dbus_connection: connection,
            ..Default::default()
        };

        (app, Task::none())
    }

    // fn on_close_requested(&self, id: Id) -> Option<Message> {
    //     Some(Message::PopupClosed(id))
    // }

    /// Describes the interface based on the current state of the application model.
    ///
    /// The applet's button in the panel will be drawn using the main view method.
    /// This view should emit messages to toggle the applet's popup window, which will
    /// be drawn using the `view_window` method.
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

    /// The applet's popup window will be drawn using this view method. If there are
    /// multiple poups, you may match the id parameter to determine which popup to
    /// create a view for.
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

    /// Register subscriptions for this application.
    ///
    /// Subscriptions are long-lived async tasks running in the background which
    /// emit messages to the application through a channel. They may be conditionally
    /// activated by selectively appending to the subscription batch, and will
    /// continue to execute for the duration that they remain in the batch.
    /// fn subscription(&self) -> Subscription<Self::Message> {
    ///     struct InhibitIdleSubscription;

    ///     Subscription::batch(vec![
    ///         // Create a subscription which emits updates through a channel.
    ///         Subscription::run_with_id(
    ///             std::any::TypeId::of::<InhibitIdleSubscription>(),
    ///             cosmic::iced::stream::channel(4, move |mut channel| async move {
    ///                 _ = channel.send(Message::SubscriptionChannel).await;
    ///             }),
    ///         ),
    ///         // Watch for application configuration changes.
    ///         self.core()
    ///             .watch_config::<Config>(Self::APP_ID)
    ///             .map(|update| {
    ///                 // for why in update.errors {
    ///                 //     tracing::error!(?why, "app config error");
    ///                 // }

    ///                 Message::UpdateConfig(update.config)
    ///             }),
    ///     ])
    /// }

    /// Handles messages emitted by the application and its widgets.
    ///
    /// Tasks may be returned for asynchronous execution of code in the background
    /// on the application's async runtime. The application will not exit until all
    /// tasks are finished.
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
