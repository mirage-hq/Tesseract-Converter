//! Keep console effects synchronous and ordered while independent tracks fit.

use std::{
    cell::RefCell,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use boa_engine::{Context, JsNativeError, JsResult, property::Attribute};
use boa_gc::{Finalize, Trace};
use boa_runtime::Console;
use boa_runtime::console::{ConsoleState, DefaultLogger, Logger};
use fx_keyframe_bake::script::{ScriptError, ScriptRuntime};

#[derive(Default)]
pub(super) struct Order {
    turn: Mutex<usize>,
    changed: Condvar,
    aborted: AtomicBool,
}

impl Order {
    fn wait(&self, position: usize) -> JsResult<()> {
        let mut turn = self.turn.lock().unwrap_or_else(|error| error.into_inner());
        while *turn != position && !self.aborted.load(Ordering::Acquire) {
            turn = self
                .changed
                .wait(turn)
                .unwrap_or_else(|error| error.into_inner());
        }
        if self.aborted.load(Ordering::Acquire) {
            return Err(JsNativeError::error()
                .with_message("script preparation aborted")
                .into());
        }
        Ok(())
    }

    pub(super) fn advance(&self) {
        let mut turn = self.turn.lock().unwrap_or_else(|error| error.into_inner());
        *turn += 1;
        self.changed.notify_all();
    }

    pub(super) fn abort_on_drop(self: &Arc<Self>) -> AbortOnDrop {
        AbortOnDrop(Arc::clone(self))
    }
}

pub(super) struct AbortOnDrop(Arc<Order>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        // Pair the flag and notification with wait's mutex to avoid lost wakes.
        let _turn = self
            .0
            .turn
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.0.aborted.store(true, Ordering::Release);
        self.0.changed.notify_all();
    }
}

#[derive(Clone)]
struct Slot {
    order: Arc<Order>,
    position: usize,
}

thread_local! {
    static CURRENT: RefCell<Option<Slot>> = const { RefCell::new(None) };
}

pub(super) fn with_order<T>(order: Arc<Order>, position: usize, fit: impl FnOnce() -> T) -> T {
    struct Restore(Option<Slot>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CURRENT.with(|slot| slot.replace(self.0.take()));
        }
    }
    let _restore = Restore(CURRENT.with(|slot| slot.replace(Some(Slot { order, position }))));
    fit()
}

pub(super) fn aborted() -> bool {
    CURRENT.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|slot| slot.order.aborted.load(Ordering::Acquire))
    })
}

pub(super) fn runtime() -> Result<ScriptRuntime, ScriptError> {
    let mut runtime = ScriptRuntime::new()?;
    let slot = CURRENT.with(|slot| slot.borrow().clone());
    if let Some(slot) = slot {
        let console = Console::init_with_logger(
            OrderedLogger {
                order: slot.order,
                position: slot.position,
                logger: DefaultLogger,
            },
            runtime.context_mut(),
        );
        // Match ScriptRuntime's console property, including enumerability.
        runtime
            .context_mut()
            .register_global_property(Console::NAME, console, Attribute::all())?;
    }
    Ok(runtime)
}

#[derive(Trace, Finalize)]
struct OrderedLogger<L: Logger> {
    #[unsafe_ignore_trace]
    order: Arc<Order>,
    #[unsafe_ignore_trace]
    position: usize,
    logger: L,
}

impl<L: Logger> Logger for OrderedLogger<L> {
    fn log(&self, msg: String, state: &ConsoleState, context: &mut Context) -> JsResult<()> {
        self.order.wait(self.position)?;
        self.logger.log(msg, state, context)
    }
    fn info(&self, msg: String, state: &ConsoleState, context: &mut Context) -> JsResult<()> {
        self.order.wait(self.position)?;
        self.logger.info(msg, state, context)
    }
    fn warn(&self, msg: String, state: &ConsoleState, context: &mut Context) -> JsResult<()> {
        self.order.wait(self.position)?;
        self.logger.warn(msg, state, context)
    }
    fn error(&self, msg: String, state: &ConsoleState, context: &mut Context) -> JsResult<()> {
        self.order.wait(self.position)?;
        self.logger.error(msg, state, context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boa_engine::{JsValue, Source};
    use std::{sync::mpsc, time::Duration};

    #[derive(Trace, Finalize)]
    struct FailingLogger;
    impl Logger for FailingLogger {
        fn log(&self, _: String, _: &ConsoleState, _: &mut Context) -> JsResult<()> {
            Err(JsNativeError::error()
                .with_message("writer failed synchronously")
                .into())
        }
        fn info(&self, msg: String, state: &ConsoleState, context: &mut Context) -> JsResult<()> {
            self.log(msg, state, context)
        }
        fn warn(&self, msg: String, state: &ConsoleState, context: &mut Context) -> JsResult<()> {
            self.log(msg, state, context)
        }
        fn error(&self, msg: String, state: &ConsoleState, context: &mut Context) -> JsResult<()> {
            self.log(msg, state, context)
        }
    }

    #[test]
    fn console_writer_errors_are_visible_to_javascript_immediately() {
        let mut context = Context::default();
        let console = Console::init_with_logger(
            OrderedLogger {
                order: Arc::new(Order::default()),
                position: 0,
                logger: FailingLogger,
            },
            &mut context,
        );
        context
            .register_global_property(Console::NAME, console, Attribute::all())
            .unwrap();
        for method in ["log", "info", "warn", "error", "debug", "trace"] {
            let code = format!(
                "let caught = false; try {{ console.{method}('test'); }} catch (e) {{ caught = String(e).includes('writer failed synchronously'); }} caught;"
            );
            // Separate lexical scope for each method.
            let code = format!("{{ {code} }}");
            assert_eq!(
                context.eval(Source::from_bytes(&code)).unwrap(),
                JsValue::from(true)
            );
        }
    }

    #[test]
    fn abort_releases_a_later_console_waiter() {
        let order = Arc::new(Order::default());
        let (started_tx, started_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        std::thread::scope(|scope| {
            let guard = order.abort_on_drop();
            scope.spawn(|| {
                started_tx.send(()).unwrap();
                finished_tx.send(order.wait(1).is_err()).unwrap();
            });
            started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            drop(guard);
            assert!(finished_rx.recv_timeout(Duration::from_secs(5)).unwrap());
        });
    }

    #[test]
    fn later_console_waits_for_an_already_running_predecessor() {
        let order = Arc::new(Order::default());
        let (tx, rx) = mpsc::channel();
        std::thread::scope(|scope| {
            let _guard = order.abort_on_drop();
            scope.spawn(|| {
                order.wait(1).unwrap();
                tx.send(1).unwrap();
            });
            scope.spawn(|| {
                order.wait(0).unwrap();
                tx.send(0).unwrap();
            });
            assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), 0);
            order.advance();
            assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), 1);
        });
    }
}
