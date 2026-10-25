use rollback_netcode_server::session::matchmaking::{InMemoryQueue, MatchmakingQueue};
use uuid::Uuid;

#[tokio::test]
async fn test_inmemory_fifo() {
    let queue = InMemoryQueue::new();
    let p1 = Uuid::new_v4();
    let p2 = Uuid::new_v4();

    queue.enqueue(p1).await.unwrap();
    queue.enqueue(p2).await.unwrap();

    assert_eq!(queue.dequeue().await.unwrap(), Some(p1), "FIFO order");
    assert_eq!(queue.dequeue().await.unwrap(), Some(p2), "FIFO order");
    assert_eq!(queue.dequeue().await.unwrap(), None, "Empty after all dequeued");
}

#[tokio::test]
async fn test_queue_size() {
    let queue = InMemoryQueue::new();
    let p1 = Uuid::new_v4();

    assert_eq!(queue.queue_size().await.unwrap(), 0);
    queue.enqueue(p1).await.unwrap();
    assert_eq!(queue.queue_size().await.unwrap(), 1);
    queue.dequeue().await.unwrap();
    assert_eq!(queue.queue_size().await.unwrap(), 0);
}

#[tokio::test]
async fn test_concurrent_enqueue() {
    let queue = std::sync::Arc::new(InMemoryQueue::new());
    let mut handles = vec![];

    for _ in 0..10 {
        let q = std::sync::Arc::clone(&queue);
        let handle = tokio::spawn(async move {
            q.enqueue(Uuid::nil()).await
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    assert_eq!(queue.queue_size().await.unwrap(), 10);
}

#[tokio::test]
async fn test_dequeue_maintains_order() {
    let queue = InMemoryQueue::new();
    let ids: Vec<_> = (0..5).map(|_| Uuid::new_v4()).collect();

    for id in &ids {
        queue.enqueue(*id).await.unwrap();
    }

    for expected_id in &ids {
        let got = queue.dequeue().await.unwrap();
        assert_eq!(got, Some(*expected_id));
    }
}
