use mio;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

type TaskId = usize;

struct Task(TaskId, RawTask<()>);

struct RawTask<T>(Box<(dyn Future<Output = T> + 'static)>);

impl<T> Unpin for RawTask<T> {}

impl<T> Future for RawTask<T> {
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context) -> Poll<T> {
        unsafe { Pin::new_unchecked(&mut *self.0).poll(context) }
    }
}

impl Task {
    fn poll(&mut self, waker: Waker) -> Poll<()> {
        let future = Pin::new(&mut self.1);
        let mut context = Context::from_waker(&waker);

        match future.poll(&mut context) {
            Poll::Ready(_) => Poll::Ready(()),
            _ => unimplemented!(),
        }
    }
}

thread_local! {
    static REACTOR: Rc<EventLoop> = Rc::new(EventLoop::new());
}

static CUSTOM_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    CustomWaker::unsafe_clone,
    CustomWaker::unsafe_wake,
    CustomWaker::unsafe_wake_by_ref,
    CustomWaker::unsafe_drop,
);

struct Wakeup {
    task_id: TaskId,
    waker: Waker,
}

#[derive(Clone)]
struct CustomWaker {
    task_id: TaskId,
}

impl CustomWaker {
    fn waker(task_id: TaskId) -> Waker {
        unsafe { Waker::from_raw(Self::new(task_id).into_raw_waker()) }
    }

    fn new(task_id: TaskId) -> Self {
        Self { task_id }
    }

    unsafe fn into_raw_waker(self) -> RawWaker {
        let prt = Box::into_raw(Box::new(self)) as *const ();
        RawWaker::new(prt, &CUSTOM_WAKER_VTABLE)
    }

    unsafe fn unsafe_clone(this: *const ()) -> RawWaker {
        let ptr = this as *const Self;
        Box::new(ptr.as_ref().unwrap().clone()).into_raw_waker()
    }

    fn wake(self: Self) {
        REACTOR.with(|reactor| {
            reactor.wake(Wakeup {
                task_id: self.task_id,
                waker: CustomWaker::waker(self.task_id),
            })
        });
    }

    unsafe fn unsafe_wake(this: *const ()) {
        let ptr = this as *mut Self;
        Box::from_raw(ptr).wake()
    }

    fn wake_by_ref(&self) {
        Box::new(self.clone()).wake()
    }

    unsafe fn unsafe_wake_by_ref(this: *const ()) {
        let ptr = this as *const Self;
        ptr.as_ref().unwrap().wake_by_ref()
    }

    unsafe fn unsafe_drop(this: *const ()) {
        let ptr = this as *mut Self;
        Box::from_raw(ptr);
    }
}

struct EventLoop {
    poller: mio::Poll,
    events: mio::Events,
    entry: RefCell<BTreeMap<mio::Token, Entry>>,
    wait_queue: RefCell<BTreeMap<TaskId, Task>>,
    run_queue: RefCell<VecDeque<Wakeup>>,
    task_counter: Cell<usize>,
}

#[derive(Debug)]
struct Entry {
    token: mio::Token,
    reader: Waker,
    writer: Waker,
}

impl EventLoop {
    fn new() -> Self {
        let poll = mio::Poll::new().unwrap();
        let events = mio::Events::with_capacity(1024);

        EventLoop {
            entry: RefCell::new(BTreeMap::new()),
            poller: poll,
            wait_queue: RefCell::new(BTreeMap::new()),
            events: events,
            run_queue: RefCell::new(VecDeque::new()),
            task_counter: Cell::new(0),
        }
    }

    fn register<S: mio::event::Source>(&mut self, source: &mut S, token: mio::Token, waker: Waker) {
        self.entry.borrow_mut().insert(
            token,
            Entry {
                token: token.clone(),
                reader: waker.clone(),
                writer: waker,
            },
        );
        self.poller
            .registry()
            .register(source, token, mio::Interest::AIO);
    }

    fn wake(&self, wakeup: Wakeup) {
        self.run_queue.borrow_mut().push_back(wakeup)
    }

    fn spawn<F: Future<Output = ()> + Send + 'static>(&self, f: F) {
        let task_id = self.task_counter.get();
        let mut task = Task(task_id, RawTask(Box::new(f)));
        let waker = CustomWaker::waker(task_id);

        if let Poll::Ready(_) = task.poll(waker) {
            return;
        }
        self.wait_queue.borrow_mut().insert(task_id,task);
    }

    pub fn run<F: Future<Output = ()> + Send + 'static>(
        &mut self,
        f: F,
    ) -> Result<(), Box<dyn Error>> {
        self.spawn(f);
        loop {
            self.poller.poll(&mut self.events, None)?;

            for event in self.events.iter() {
                let token = event.token();

                if let Some(entry) = self.entry.borrow_mut().get(&token) {
                    if event.is_readable() {
                        entry.reader.wake_by_ref()
                    }
                    if event.is_writable() {
                        entry.writer.wake_by_ref()
                    }
                }
            }

            loop {
                let wakeup = self.run_queue.borrow_mut().pop_front();
                match wakeup {
                    Some(wakeup) => {
                        if let Some(mut task) = self.wait_queue.borrow_mut().remove(&wakeup.task_id) {
                            if let Poll::Pending = task.poll(wakeup.waker) {
                                self.wait_queue.borrow_mut().insert(wakeup.task_id, task);
                            }
                        }
                    }
                    None => break,
                }
            }
        }
    }
}

#[test]
fn run_test() {
    let mut reactor = EventLoop::new();

    let future = async {
        println!("aaaa");
    };

    reactor.run(future);
}
