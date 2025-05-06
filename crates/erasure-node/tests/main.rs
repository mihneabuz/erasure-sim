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
        collections::{HashMap, HashSet},
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
        senders: HashMap<usize, Sender<(usize, Command)>>,
        receivers: HashMap<usize, Receiver<(usize, Command)>>,
        disabled: HashSet<usize>,
    }

    impl TestNetworkBuilder {
        fn new() -> Self {
            Self {
                inner: Arc::new(Mutex::new(TestNetworkBuilderInner {
                    id: 0,
                    senders: HashMap::new(),
                    receivers: HashMap::new(),
                    disabled: HashSet::new(),
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

        fn disable(&self, id: usize) {
            self.inner.lock().unwrap().disabled.insert(id);
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
            let inner = self.builder.lock().unwrap();
            if inner.disabled.contains(&id) {
                return;
            }

            // println!("{} > SENDING to {}: {:?}", self.id, peer, cmd);
            inner.senders[&id].send((self.id, cmd)).unwrap();
        }

        async fn recv(&self) -> Option<(String, Command)> {
            loop {
                if let Some(res) = self.builder.lock().unwrap().receivers[&self.id]
                    .try_recv()
                    .map(|(id, cmd)| (format!("{id}"), cmd))
                    .ok()
                {
                    // println!("{} > RECEIVED from {}: {:?}", self.id, &res.0, &res.1);
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

        assert_eq!(aw(net.discover()).len(), 2);

        aw(n1.upload("test".to_string(), "content".to_string()));
        assert!(aw(n1.download("test".to_string())).is_some());

        aw(n2.download("test".to_string()));
        std::thread::sleep(std::time::Duration::from_millis(10));

        assert!(aw(n2.download("test".to_string())).is_some());
    }

    #[test]
    fn big() {
        let builder = TestNetworkBuilder::new();
        let n1 = TestNode::new(builder.spawn());
        let n2 = TestNode::new(builder.spawn());

        let content = "hello world!".repeat(100);
        let name = "hello".to_string();

        aw(n1.upload(name.clone(), content.clone()));
        assert!(aw(n1.download(name.clone())).is_some());

        aw(n2.download(name.clone()));
        std::thread::sleep(std::time::Duration::from_millis(20));

        let res = aw(n2.download(name.clone()));
        assert!(res.is_some());
        assert_eq!(res.unwrap(), content);
    }

    #[test]
    fn many() {
        let builder = TestNetworkBuilder::new();
        let nodes = (0..32)
            .map(|_| TestNode::new(builder.spawn()))
            .collect::<Vec<_>>();

        let content = "hello world!".repeat(100);
        let name1 = "hello".to_string();
        let name2 = "hello".to_string();
        let name3 = "hello".to_string();

        aw(nodes[0].upload(name1.clone(), content.clone()));
        aw(nodes[10].upload(name2.clone(), content.clone()));
        aw(nodes[20].upload(name3.clone(), content.clone()));

        builder.disable(nodes[0].network().id);
        builder.disable(nodes[10].network().id);

        aw(nodes[7].download(name1.clone()));
        aw(nodes[13].download(name2.clone()));
        aw(nodes[17].download(name3.clone()));
        std::thread::sleep(std::time::Duration::from_millis(20));

        let res = aw(nodes[7].download(name1.clone()));
        assert!(res.is_some());
        assert_eq!(res.unwrap(), content);

        let res = aw(nodes[13].download(name2.clone()));
        assert!(res.is_some());
        assert_eq!(res.unwrap(), content);

        let res = aw(nodes[17].download(name3.clone()));
        assert!(res.is_some());
        assert_eq!(res.unwrap(), content);
    }

    #[test]
    fn lost() {
        let builder = TestNetworkBuilder::new();
        let nodes = (0..8)
            .map(|_| TestNode::new(builder.spawn()))
            .collect::<Vec<_>>();

        let content = "hello world!".repeat(30);
        let name = "hello".to_string();

        aw(nodes[0].upload(name.clone(), content.clone()));
        for i in 0..6 {
            builder.disable(nodes[i].network().id);
        }

        aw(nodes[7].download(name.clone()));
        std::thread::sleep(std::time::Duration::from_millis(40));

        aw(nodes[7].download(name.clone()));
        std::thread::sleep(std::time::Duration::from_millis(40));

        aw(nodes[7].download(name.clone()));
        std::thread::sleep(std::time::Duration::from_millis(40));

        let res = aw(nodes[7].download(name.clone()));
        assert!(res.is_none());
    }
}
