//! **What happens to the app, for its own threads** (plan
//! `20261023_01_FOREGROUND_SERVICE_PLAN.md`): any thread can follow the
//! app's visibility, the foreground service, the network and Doze without
//! the main loop and without an async runtime. [`subscribe`] returns a
//! queue that receives every event after the call, in order, never dropped.
//!
//! ```no_run
//! use libmorph::system_events::{self, SystemEvent};
//!
//! let events = system_events::subscribe();
//! std::thread::spawn(move || loop {
//!     match events.next_blocking() {
//!         SystemEvent::DeviceIdle(false) | SystemEvent::Network(Some(_)) => { /* reconnect */ }
//!         _ => {}
//!     }
//! });
//! ```
//!
//! * **Android** with `mobile-system-events`: libmobile's events, reported
//!   by the system on whatever thread it uses. The first subscription also
//!   reports the state at that moment.
//! * **The desktop and the web**: libmorph's own, from the winit runner:
//!   [`SystemEvent::Visible`] when the app resumes or suspends, and nothing
//!   else.
//! * **Android without `mobile-system-events`** and scripted runs: nothing.
//!
//! [`current`] gives the latest of each kind, for a subscriber that starts
//! late. With the feature `tokio`, [`subscribe_tokio`] gives a tokio
//! receiver instead.

pub use crate::service::ServiceEvent;

/// Something that happened to the app.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SystemEvent {
    /// The app can be seen (`true`), or no longer.
    Visible(bool),
    /// The app has the focus (`true`), or no longer (Android).
    Focused(bool),
    /// The foreground service ([`service`](crate::service)).
    Service(ServiceEvent),
    /// The default network: `Some` when it is available or changes, `None`
    /// when there is none (Android).
    Network(Option<NetworkInfo>),
    /// Doze entered (`true`) or left (Android).
    DeviceIdle(bool),
    /// Whether the app is exempt from battery optimization, when that
    /// changed ([`battery`](crate::battery); Android with `mobile-battery`).
    BatteryExemption(bool),
}

/// The default network.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct NetworkInfo {
    /// The system checked that it reaches the internet.
    pub validated: bool,
    /// It may cost the user money.
    pub metered: bool,
    pub transport: Transport,
}

/// How the default network is carried. A VPN is `Vpn`, whatever it runs
/// over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Transport {
    Wifi,
    Cellular,
    Ethernet,
    Vpn,
    Other,
}

/// The latest event of each kind; `None` for a kind not reported yet.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SystemState {
    pub visible: Option<bool>,
    pub focused: Option<bool>,
    /// The latest service event.
    pub service: Option<ServiceEvent>,
    /// `Some(None)`: no network.
    pub network: Option<Option<NetworkInfo>>,
    pub device_idle: Option<bool>,
    pub battery_exemption: Option<bool>,
}

impl SystemState {
    #[cfg_attr(
        all(target_os = "android", feature = "mobile-system-events"),
        allow(dead_code)
    )]
    const EMPTY: Self = Self {
        visible: None,
        focused: None,
        service: None,
        network: None,
        device_idle: None,
        battery_exemption: None,
    };

    #[cfg_attr(
        all(target_os = "android", feature = "mobile-system-events"),
        allow(dead_code)
    )]
    fn apply(&mut self, event: &SystemEvent) {
        match event {
            SystemEvent::Visible(v) => self.visible = Some(*v),
            SystemEvent::Focused(v) => self.focused = Some(*v),
            SystemEvent::Service(e) => self.service = Some(e.clone()),
            SystemEvent::Network(n) => self.network = Some(n.clone()),
            SystemEvent::DeviceIdle(v) => self.device_idle = Some(*v),
            SystemEvent::BatteryExemption(v) => self.battery_exemption = Some(*v),
        }
    }
}

/// One subscriber's queue of events. Dropping it unsubscribes.
#[derive(Debug)]
pub struct Events {
    inner: imp::Events,
}

impl Events {
    /// The next event, or `None` if there is none yet.
    pub fn try_next(&self) -> Option<SystemEvent> {
        self.inner.try_next()
    }

    /// The next event, waiting for one.
    pub fn next_blocking(&self) -> SystemEvent {
        self.inner.next_blocking()
    }
}

/// Every event after the call, in order, never dropped (the queue is
/// unbounded; events are rare).
#[must_use]
pub fn subscribe() -> Events {
    Events {
        inner: imp::subscribe(),
    }
}

/// [`subscribe`] as a tokio receiver (feature `tokio`). Sending needs no
/// tokio runtime; receive it however the app likes, for example in a
/// `select!`.
#[cfg(feature = "tokio")]
#[must_use]
pub fn subscribe_tokio() -> tokio::sync::mpsc::UnboundedReceiver<SystemEvent> {
    imp::subscribe_tokio()
}

/// The latest of each kind, for a subscriber that starts late.
#[must_use]
pub fn current() -> SystemState {
    imp::current()
}

/// Hand `event` to every subscriber (the winit runner's lifecycle).
#[cfg(not(all(target_os = "android", feature = "mobile-system-events")))]
#[cfg_attr(target_os = "android", allow(dead_code))]
pub(crate) fn publish(event: SystemEvent) {
    imp::publish(event);
}

/// libmorph's own hub, where libmobile's events are not in use.
#[cfg(not(all(target_os = "android", feature = "mobile-system-events")))]
mod imp {
    use super::{SystemEvent, SystemState};
    use std::collections::VecDeque;
    use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};

    #[derive(Debug, Default)]
    struct Queue {
        items: Mutex<VecDeque<SystemEvent>>,
        ready: Condvar,
    }

    #[derive(Debug)]
    pub(super) struct Events {
        queue: Arc<Queue>,
    }

    impl Events {
        pub(super) fn try_next(&self) -> Option<SystemEvent> {
            lock(&self.queue.items).pop_front()
        }

        pub(super) fn next_blocking(&self) -> SystemEvent {
            let mut items = lock(&self.queue.items);
            loop {
                if let Some(event) = items.pop_front() {
                    return event;
                }
                items = self
                    .queue
                    .ready
                    .wait(items)
                    .unwrap_or_else(|e| e.into_inner());
            }
        }
    }

    enum Sink {
        Queue(Weak<Queue>),
        #[cfg(feature = "tokio")]
        Tokio(tokio::sync::mpsc::UnboundedSender<SystemEvent>),
    }

    impl Sink {
        /// Deliver; `false` once the subscriber is gone.
        fn send(&self, event: &SystemEvent) -> bool {
            match self {
                Self::Queue(queue) => match queue.upgrade() {
                    Some(queue) => {
                        lock(&queue.items).push_back(event.clone());
                        queue.ready.notify_all();
                        true
                    }
                    None => false,
                },
                #[cfg(feature = "tokio")]
                Self::Tokio(sender) => sender.send(event.clone()).is_ok(),
            }
        }
    }

    /// The subscribers and the latest state, under one lock so every
    /// subscriber sees the same order.
    struct Hub {
        sinks: Vec<Sink>,
        state: SystemState,
    }

    static HUB: Mutex<Hub> = Mutex::new(Hub {
        sinks: Vec::new(),
        state: SystemState::EMPTY,
    });

    /// Poisoning is ignored: every critical section is a push or a swap.
    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(super) fn subscribe() -> Events {
        let queue = Arc::new(Queue::default());
        lock(&HUB).sinks.push(Sink::Queue(Arc::downgrade(&queue)));
        Events { queue }
    }

    #[cfg(feature = "tokio")]
    pub(super) fn subscribe_tokio() -> tokio::sync::mpsc::UnboundedReceiver<SystemEvent> {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        lock(&HUB).sinks.push(Sink::Tokio(sender));
        receiver
    }

    pub(super) fn current() -> SystemState {
        lock(&HUB).state.clone()
    }

    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub(super) fn publish(event: SystemEvent) {
        let mut hub = lock(&HUB);
        hub.state.apply(&event);
        hub.sinks.retain(|sink| sink.send(&event));
    }

    #[cfg(test)]
    pub(super) fn sinks() -> usize {
        lock(&HUB).sinks.len()
    }
}

/// libmobile's events, in libmorph's types.
#[cfg(all(target_os = "android", feature = "mobile-system-events"))]
mod imp {
    use super::{NetworkInfo, SystemEvent, SystemState, Transport};
    use crate::service::from_mobile_event;
    use libmobile::system_events as mobile;

    #[derive(Debug)]
    pub(super) struct Events(mobile::Events);

    impl Events {
        pub(super) fn try_next(&self) -> Option<SystemEvent> {
            // Skip any event libmorph does not know.
            while let Some(event) = self.0.try_next() {
                if let Some(event) = from_mobile(event) {
                    return Some(event);
                }
            }
            None
        }

        pub(super) fn next_blocking(&self) -> SystemEvent {
            loop {
                if let Some(event) = from_mobile(self.0.next_blocking()) {
                    return event;
                }
            }
        }
    }

    fn network(info: mobile::NetworkInfo) -> NetworkInfo {
        NetworkInfo {
            validated: info.validated,
            metered: info.metered,
            transport: match info.transport {
                mobile::Transport::Wifi => Transport::Wifi,
                mobile::Transport::Cellular => Transport::Cellular,
                mobile::Transport::Ethernet => Transport::Ethernet,
                mobile::Transport::Vpn => Transport::Vpn,
                _ => Transport::Other,
            },
        }
    }

    pub(super) fn from_mobile(event: mobile::SystemEvent) -> Option<SystemEvent> {
        use mobile::SystemEvent as E;
        Some(match event {
            E::Visible(v) => SystemEvent::Visible(v),
            E::Focused(v) => SystemEvent::Focused(v),
            E::Service(e) => SystemEvent::Service(from_mobile_event(e)?),
            E::Network(n) => SystemEvent::Network(n.map(network)),
            E::DeviceIdle(v) => SystemEvent::DeviceIdle(v),
            #[cfg(feature = "mobile-battery")]
            E::BatteryExemption(v) => SystemEvent::BatteryExemption(v),
            _ => return None,
        })
    }

    pub(super) fn subscribe() -> Events {
        Events(mobile::subscribe())
    }

    /// libmobile's receiver, forwarded in libmorph's types by a thread of
    /// its own, which ends at the first event after the receiver is dropped.
    #[cfg(feature = "tokio")]
    pub(super) fn subscribe_tokio() -> tokio::sync::mpsc::UnboundedReceiver<SystemEvent> {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut events = mobile::subscribe_tokio();
        let spawned = std::thread::Builder::new()
            .name("libmorph-system-events".into())
            .spawn(move || {
                while let Some(event) = events.blocking_recv() {
                    if let Some(event) = from_mobile(event)
                        && sender.send(event).is_err()
                    {
                        break;
                    }
                }
            });
        if let Err(error) = spawned {
            log::error!("libmorph: system events: cannot start the tokio forwarder: {error}");
        }
        receiver
    }

    pub(super) fn current() -> SystemState {
        let state = mobile::current();
        SystemState {
            visible: state.visible,
            focused: state.focused,
            service: state.service.and_then(from_mobile_event),
            network: state.network.map(|n| n.map(network)),
            device_idle: state.device_idle,
            #[cfg(feature = "mobile-battery")]
            battery_exemption: state.battery_exemption,
            #[cfg(not(feature = "mobile-battery"))]
            battery_exemption: None,
        }
    }
}

#[cfg(all(
    test,
    not(all(target_os = "android", feature = "mobile-system-events"))
))]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::Duration;

    /// The hub is process-wide, so these tests take turns.
    static TURN: Mutex<()> = Mutex::new(());

    fn turn() -> std::sync::MutexGuard<'static, ()> {
        TURN.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn drain(events: &Events) -> Vec<SystemEvent> {
        std::iter::from_fn(|| events.try_next()).collect()
    }

    #[test]
    fn every_event_in_order() {
        let _turn = turn();
        let events = subscribe();
        for i in 0..500 {
            publish(SystemEvent::Visible(i % 2 == 0));
        }
        let got = drain(&events);
        assert_eq!(got.len(), 500);
        for (i, event) in got.iter().enumerate() {
            assert_eq!(*event, SystemEvent::Visible(i % 2 == 0));
        }
        assert_eq!(events.try_next(), None);
    }

    #[test]
    fn a_blocking_wait_is_woken_from_another_thread() {
        let _turn = turn();
        let events = subscribe();
        let sender = std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(30));
            publish(SystemEvent::Visible(false));
        });
        assert_eq!(events.next_blocking(), SystemEvent::Visible(false));
        sender.join().unwrap();
    }

    #[test]
    fn dropped_subscribers_leave_and_current_keeps_the_latest() {
        let _turn = turn();
        let a = subscribe();
        let b = subscribe();
        publish(SystemEvent::Visible(true));
        assert_eq!(b.try_next(), Some(SystemEvent::Visible(true)));
        let before = imp::sinks();
        drop(b);
        publish(SystemEvent::Service(ServiceEvent::Started));
        assert_eq!(imp::sinks(), before - 1);
        assert_eq!(
            drain(&a),
            [
                SystemEvent::Visible(true),
                SystemEvent::Service(ServiceEvent::Started)
            ]
        );
        let state = current();
        assert_eq!(state.visible, Some(true));
        assert_eq!(state.service, Some(ServiceEvent::Started));
        assert_eq!(state.network, None);
    }

    #[cfg(feature = "tokio")]
    #[test]
    fn the_tokio_receiver_gets_every_event() {
        let _turn = turn();
        let mut receiver = subscribe_tokio();
        publish(SystemEvent::Visible(true));
        publish(SystemEvent::Visible(false));
        assert_eq!(receiver.try_recv(), Ok(SystemEvent::Visible(true)));
        let blocked = std::thread::spawn(move || receiver.blocking_recv());
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(blocked.join().unwrap(), Some(SystemEvent::Visible(false)));
        let before = imp::sinks();
        publish(SystemEvent::Visible(true));
        assert_eq!(imp::sinks(), before - 1, "the dropped receiver's sink goes");
    }
}
