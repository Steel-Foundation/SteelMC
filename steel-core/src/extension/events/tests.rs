use super::*;
use std::sync::Arc;
use steel_utils::locks::SyncMutex;

#[derive(Default)]
struct TestEvent {
    log: Vec<&'static str>,
    cancelled: bool,
}

impl Event for TestEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

struct Recorder {
    name: &'static str,
    cancel: bool,
    seen: Arc<SyncMutex<Vec<EventPriority>>>,
}

impl EventHandler<TestEvent> for Recorder {
    fn handle(&self, event: &mut TestEvent, priority: EventPriority) {
        event.log.push(self.name);
        self.seen.lock().push(priority);
        if self.cancel {
            event.cancelled = true;
        }
    }
}

fn recorder(name: &'static str, cancel: bool) -> Box<dyn EventHandler<TestEvent>> {
    Box::new(Recorder {
        name,
        cancel,
        seen: Arc::default(),
    })
}

#[test]
fn handlers_run_by_priority_then_subscription_order() {
    let mut bus = EventBus::default();
    bus.subscribe(EventPriority::Monitor, false, recorder("monitor", false));
    bus.subscribe(EventPriority::High, false, recorder("high", false));
    bus.subscribe(EventPriority::Normal, false, recorder("normal-1", false));
    bus.subscribe(EventPriority::Lowest, false, recorder("lowest", false));
    bus.subscribe(EventPriority::Normal, false, recorder("normal-2", false));
    let mut event = TestEvent::default();
    bus.fire(&mut event);
    assert_eq!(
        event.log,
        ["lowest", "normal-1", "normal-2", "high", "monitor"]
    );
}

#[test]
fn cancellation_is_visible_and_skips_ignore_cancelled_handlers() {
    let mut bus = EventBus::default();
    bus.subscribe(EventPriority::Low, false, recorder("canceller", true));
    bus.subscribe(EventPriority::Normal, true, recorder("skipped", false));
    bus.subscribe(EventPriority::High, false, recorder("observer", false));
    let mut event = TestEvent::default();
    bus.fire(&mut event);
    assert!(event.cancelled);
    assert_eq!(event.log, ["canceller", "observer"]);
}

#[test]
fn handlers_receive_their_subscribed_priority() {
    let seen = Arc::new(SyncMutex::new(Vec::new()));
    let mut bus = EventBus::default();
    for priority in [EventPriority::Monitor, EventPriority::Lowest] {
        bus.subscribe(
            priority,
            false,
            Box::new(Recorder {
                name: "r",
                cancel: false,
                seen: Arc::clone(&seen),
            }),
        );
    }
    bus.fire(&mut TestEvent::default());
    assert_eq!(
        *seen.lock(),
        [EventPriority::Lowest, EventPriority::Monitor]
    );
}
