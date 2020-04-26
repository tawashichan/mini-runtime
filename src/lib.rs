use mio;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
mod tcp_listener;
mod tcp_stream;

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
            Poll::Pending => Poll::Pending,
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
    poller: RefCell<mio::Poll>,
    events: RefCell<mio::Events>,
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

pub fn run<F: Future<Output = ()> + Send + 'static>(f: F) {
    REACTOR.with(|reactor| reactor.run(f));
}

pub fn spawn<F: Future<Output = ()> + Send + 'static>(f: F) {
    REACTOR.with(|reactor| reactor.spawn(f));
}

impl EventLoop {
    fn new() -> Self {
        let poll = mio::Poll::new().unwrap();
        let events = mio::Events::with_capacity(1024);

        EventLoop {
            entry: RefCell::new(BTreeMap::new()),
            poller: RefCell::new(poll),
            wait_queue: RefCell::new(BTreeMap::new()),
            events: RefCell::new(events),
            run_queue: RefCell::new(VecDeque::new()),
            task_counter: Cell::new(0),
        }
    }

    fn register_entry(&self, token: mio::Token, waker: Waker) {
        println!("{:?} {:?}", token, waker);
        self.entry.borrow_mut().insert(
            token,
            Entry {
                token: token.clone(),
                reader: waker.clone(),
                writer: waker,
            },
        );
    }

    fn register_source<S: mio::event::Source + std::fmt::Debug>(
        &self,
        source: &mut S,
        token: mio::Token,
    ) {
        println!("{:?} {:?}", source, token);
        self.poller.borrow_mut().registry().register(
            source,
            token,
            mio::Interest::READABLE | mio::Interest::WRITABLE | mio::Interest::AIO,
        );
    }

    fn wake(&self, wakeup: Wakeup) {
        self.run_queue.borrow_mut().push_back(wakeup)
    }

    fn spawn<F: Future<Output = ()> + Send + 'static>(&self, f: F) {
        let task_id = self.task_counter.get();
        self.task_counter.set(task_id + 1);
        let mut task = Task(task_id, RawTask(Box::new(f)));
        let waker = CustomWaker::waker(task_id);

        if let Poll::Ready(_) = task.poll(waker) {
            return;
        }
        self.wait_queue.borrow_mut().insert(task_id, task);
    }

    fn run<F: Future<Output = ()> + Send + 'static>(&self, f: F) -> Result<(), Box<dyn Error>> {
        self.spawn(f);
        loop {
            println!("start polling");

            let mut events = self.events.borrow_mut();
            self.poller.borrow_mut().poll(&mut events, None)?;

            for event in events.iter() {
                let token = event.token();

                if let Some(entry) = self.entry.borrow_mut().get(&token) {
                    if event.is_readable() {
                        println!("read event");
                        entry.reader.wake_by_ref()
                    }
                    if event.is_writable() {
                        println!("write event");
                        entry.writer.wake_by_ref()
                    }
                }
            }

            loop {
                let wakeup = self.run_queue.borrow_mut().pop_front();
                match wakeup {
                    Some(wakeup) => {
                        let mut wait_queue = self.wait_queue.borrow_mut();
                        if let Some(mut task) = wait_queue.remove(&wakeup.task_id) {
                            if let Poll::Pending = task.poll(wakeup.waker) {
                                wait_queue.insert(wakeup.task_id, task);
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
fn server() {
    use futures::io::{AsyncReadExt, AsyncWriteExt};

    async fn listen(addr: &str) {
        use futures::stream::StreamExt;

        let addr = addr.parse().unwrap();
        let listener = tcp_listener::AsyncTcpListener::bind(addr).unwrap();
        let mut incoming = listener.incoming();

        while let Some(stream) = incoming.next().await {
            spawn(process(stream));
        }
    }

    async fn process(mut stream: tcp_stream::AsyncTcpStream) {
        let mut buf = vec![0; 10];
        let _ = stream.read_exact(&mut buf).await;
        println!("{}", String::from_utf8_lossy(&buf));
        let _ = stream.write_all(b"GET / HTTP/1.0\r\n\r\n").await;
    }

    run(listen("127.0.0.1:8888"))
}

/*#[test]
fn client() {
    use futures::io::{AsyncReadExt, AsyncWriteExt};
    async fn http_get(addr: &str) -> Result<String, std::io::Error> {
        let addr = addr.parse().unwrap();
        let mut conn = tcp_stream::AsyncTcpStream::connect(addr)?;

        let _ = conn.write_all(b"GET / HTTP/1.0\r\n\r\n").await?;

        let mut page = Vec::new();
        loop {
            let mut buf = vec![0; 128];
            let len = conn.read(&mut buf).await?;
            if len == 0 {
                break;
            }
            page.extend_from_slice(&buf[..len]);
        }
        let page = String::from_utf8_lossy(&page).into();
        Ok(page)
    }
    async fn local() {
        let res = http_get("127.0.0.1:8888").await.unwrap();
        println!("{}", res);
    }
    run(local())
}
*/