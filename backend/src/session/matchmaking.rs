// Matchmaking queue trait with in-memory and Redis implementations
use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[async_trait]
pub trait MatchmakingQueue: Send + Sync {
    async fn enqueue(&self, player_id: Uuid) -> Result<(), String>;
    async fn dequeue(&self) -> Result<Option<Uuid>, String>;
    async fn queue_size(&self) -> Result<usize, String>;
}

/// In-memory FIFO matchmaking queue
pub struct InMemoryQueue {
    queue: Arc<Mutex<VecDeque<Uuid>>>,
}

impl InMemoryQueue {
    pub fn new() -> Self {
        Self {
            queue: Arc::new(Mutex::new(VecDeque::new())),
        }
    }
}

#[async_trait]
impl MatchmakingQueue for InMemoryQueue {
    async fn enqueue(&self, player_id: Uuid) -> Result<(), String> {
        self.queue.lock().unwrap().push_back(player_id);
        Ok(())
    }

    async fn dequeue(&self) -> Result<Option<Uuid>, String> {
        Ok(self.queue.lock().unwrap().pop_front())
    }

    async fn queue_size(&self) -> Result<usize, String> {
        Ok(self.queue.lock().unwrap().len())
    }
}

/// Redis-backed FIFO matchmaking queue
pub struct RedisQueue {
    _client: Arc<Mutex<()>>, // Placeholder for Redis client
}

impl RedisQueue {
    pub fn new() -> Self {
        Self {
            _client: Arc::new(Mutex::new(())),
        }
    }
}

#[async_trait]
impl MatchmakingQueue for RedisQueue {
    async fn enqueue(&self, player_id: Uuid) -> Result<(), String> {
        // In real impl: RPUSH queue player_id
        Ok(())
    }

    async fn dequeue(&self) -> Result<Option<Uuid>, String> {
        // In real impl: LPOP queue
        Ok(None)
    }

    async fn queue_size(&self) -> Result<usize, String> {
        // In real impl: LLEN queue
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let queue = Arc::new(InMemoryQueue::new());
        let mut handles = vec![];

        for i in 0..10 {
            let q = Arc::clone(&queue);
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
}
