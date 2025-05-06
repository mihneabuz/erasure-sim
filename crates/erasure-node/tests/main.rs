mod file {
    use erasure_node::file::File;

    #[test]
    fn simple() {
        let s1 = "hello world!";
        let mut file = File::encode(s1).unwrap();
        file.shards_mut().delete(0);
        assert!(file.can_decode());
        let s2 = file.decode().unwrap();
        assert_eq!(s1, s2);
    }

    #[test]
    fn big() {
        let s1 = "hello world!".repeat(100);
        let mut file = File::encode(&s1).unwrap();
        file.shards_mut().delete(0);
        file.shards_mut().delete(1);
        file.shards_mut().delete(3);
        file.shards_mut().delete(10);
        file.shards_mut().delete(20);
        assert!(file.can_decode());
        let s2 = file.decode().unwrap();
        assert_eq!(s1, s2);
    }

    #[test]
    fn fail() {
        let s1 = "hello world!".repeat(3);
        let mut file = File::encode(&s1).unwrap();
        file.shards_mut().delete(0);
        file.shards_mut().delete(1);
        assert!(!file.can_decode());
        assert!(file.decode().is_none());
    }
}

mod node {
    use std::{
        collections::HashMap,
        ops::Deref,
        pin::pin,
        sync::{
            Arc, Mutex,
            mpsc::{Receiver, Sender, channel},
        },
        task::{Context, Poll, Waker},
    };

    use erasure_node::{
        network::{Command, Network},
        node::Node,
    };

    struct TestNetworkBuilder {
        inner: Arc<Mutex<TestNetworkBuilderInner>>,
    }

    struct TestNetworkBuilderInner {
        id: usize,
        senders: HashMap<usize, Sender<Command>>,
        receivers: HashMap<usize, Receiver<Command>>,
    }

    impl TestNetworkBuilder {
        fn new() -> Self {
            Self {
                inner: Arc::new(Mutex::new(TestNetworkBuilderInner {
                    id: 0,
                    senders: HashMap::new(),
                    receivers: HashMap::new(),
                })),
            }
        }

        fn spawn(&self) -> TestNetwork {
            let mut inner = self.inner.lock().unwrap();
            let id = inner.id;
            inner.id += 1;

            let (sender, receiver) = channel();
            inner.senders.insert(id, sender);
            inner.receivers.insert(id, receiver);

            TestNetwork {
                id,
                builder: self.inner.clone(),
            }
        }
    }

    struct TestNetwork {
        id: usize,
        builder: Arc<Mutex<TestNetworkBuilderInner>>,
    }

    impl Network for TestNetwork {
        async fn discover(&self) -> Vec<String> {
            self.builder
                .lock()
                .unwrap()
                .senders
                .keys()
                .filter(|id| **id != self.id)
                .map(|id| format!("{id}"))
                .collect()
        }

        async fn send(&self, peer: String, cmd: Command) {
            let id = peer.parse().unwrap();
            self.builder.lock().unwrap().senders[&id].send(cmd).unwrap();
        }

        async fn recv(&self) -> Option<(String, Command)> {
            loop {
                if let Some(res) = self.builder.lock().unwrap().receivers[&self.id]
                    .try_recv()
                    .map(|cmd| (format!("{}", self.id), cmd))
                    .ok()
                {
                    return Some(res);
                }
            }
        }
    }

    struct TestNode {
        inner: Arc<Node<TestNetwork>>,
    }

    impl TestNode {
        fn new(network: TestNetwork) -> Self {
            let inner = Arc::new(Node::new(network));
            let inner_clone = Arc::clone(&inner);
            std::thread::spawn(move || aw(inner_clone.run()));
            Self { inner }
        }
    }

    impl Deref for TestNode {
        type Target = Node<TestNetwork>;

        fn deref(&self) -> &Self::Target {
            self.inner.deref()
        }
    }

    fn aw<F, T>(fut: F) -> T
    where
        F: Future<Output = T>,
    {
        let mut fut = pin!(fut);
        loop {
            if let Poll::Ready(res) = fut.as_mut().poll(&mut Context::from_waker(&Waker::noop())) {
                return res;
            }
        }
    }

    #[test]
    fn simple() {
        let builder = TestNetworkBuilder::new();

        let net = builder.spawn();
        let n1 = TestNode::new(builder.spawn());
        let n2 = TestNode::new(builder.spawn());

        aw(n1.upload("test".to_string(), "content".to_string()));
        println!("{:?}", aw(net.discover()));
        println!("{:?}", aw(n1.download("test".to_string())));
        println!("{:?}", aw(n2.download("test".to_string())));
        std::thread::sleep(std::time::Duration::from_millis(100));
        println!("{:?}", aw(n2.download("test".to_string())));
    }
}
