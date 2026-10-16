//! 长期记忆（SPI）：跨会话的知识沉淀与召回。
//!
//! 语义（FC-POST-200 ~ FC-POST-202）：
//! - [`Memory::remember`] 追加记忆并返回含生成 `id` 的记录；
//! - [`Memory::recall`] 按「相关度降序 → 时间倒序 → id」返回至多 `limit` 条；
//!   相关度由实现定义；无匹配时即「最近优先」（便于上下文注入）；
//! - [`Memory::forget`] 幂等（不存在视为成功）。
//!
//! 内置 [`InMemoryMemory`] 使用朴素关键字相关度；向量检索等由生态实现（见 roadmap）。

use std::fmt;
use std::num::NonZeroUsize;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

/// 记忆标识（UUID v7，时间有序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MemoryId(Uuid);

impl MemoryId {
    /// 生成新的记忆标识。
    pub fn generate() -> Self {
        Self(Uuid::now_v7())
    }

    /// 从已有 UUID 构造（持久化恢复场景）。
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// 返回内部 UUID。
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl fmt::Display for MemoryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 一条记忆。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryRecord {
    /// 标识。
    pub id: MemoryId,
    /// 内容文本。
    pub content: String,
    /// 写入时间（Unix epoch 毫秒）。
    pub at_ms: u64,
}

impl MemoryRecord {
    /// 构造记录（生成 `id`）。
    pub fn new(content: impl Into<String>, at_ms: u64) -> Self {
        Self {
            id: MemoryId::generate(),
            content: content.into(),
            at_ms,
        }
    }
}

/// 记忆后端错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MemoryError {
    /// 后端失败（含上下文）。
    #[error("记忆后端失败：{message}")]
    Backend {
        /// 失败描述。
        message: String,
    },
}

/// 长期记忆（SPI 扩展点）。
#[async_trait]
pub trait Memory: Send + Sync + 'static {
    /// 写入一条记忆。
    ///
    /// # Errors
    /// 后端失败 → [`MemoryError`]。
    async fn remember(&self, content: String, at_ms: u64) -> Result<MemoryRecord, MemoryError>;

    /// 按相关度召回至多 `limit` 条记忆（相关度降序，同分按时间倒序）。
    ///
    /// # Errors
    /// 后端失败 → [`MemoryError`]。
    async fn recall(
        &self,
        query: &str,
        limit: NonZeroUsize,
    ) -> Result<Vec<MemoryRecord>, MemoryError>;

    /// 删除一条记忆（幂等：不存在视为成功）。
    ///
    /// # Errors
    /// 后端失败 → [`MemoryError`]。
    async fn forget(&self, id: &MemoryId) -> Result<(), MemoryError>;
}

/// 进程内记忆（朴素关键字相关度；向量检索由生态实现）。
///
/// 相关度定义：查询按非字母数字字符切分、小写化并去重后，统计**出现在记录内容中**
/// 的关键字个数；同分按 `at_ms` 倒序、再按 `id` 排序（保证确定性）。
#[derive(Debug, Clone, Default)]
pub struct InMemoryMemory {
    records: Arc<Mutex<Vec<MemoryRecord>>>,
}

impl InMemoryMemory {
    /// 创建空记忆。
    pub fn new() -> Self {
        Self::default()
    }

    /// 已存记忆数量。
    pub async fn len(&self) -> usize {
        self.records.lock().await.len()
    }

    /// 是否为空。
    pub async fn is_empty(&self) -> bool {
        self.records.lock().await.is_empty()
    }
}

/// 关键字切分：按非字母数字字符拆分、小写化、去重（保序）。
fn keywords(query: &str) -> Vec<String> {
    let mut keywords: Vec<String> = Vec::new();
    for token in query.split(|c: char| !c.is_alphanumeric()) {
        if token.is_empty() {
            continue;
        }
        let token = token.to_lowercase();
        if !keywords.iter().any(|existing| existing == &token) {
            keywords.push(token);
        }
    }
    keywords
}

/// 相关度：内容命中的关键字个数。
fn relevance(keywords: &[String], content: &str) -> usize {
    let content = content.to_lowercase();
    keywords
        .iter()
        .filter(|keyword| content.contains(keyword.as_str()))
        .count()
}

#[async_trait]
impl Memory for InMemoryMemory {
    async fn remember(&self, content: String, at_ms: u64) -> Result<MemoryRecord, MemoryError> {
        let record = MemoryRecord::new(content, at_ms);
        self.records.lock().await.push(record.clone());
        Ok(record)
    }

    async fn recall(
        &self,
        query: &str,
        limit: NonZeroUsize,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        let keywords = keywords(query);
        let records = self.records.lock().await;
        let mut scored: Vec<(usize, &MemoryRecord)> = records
            .iter()
            .map(|record| (relevance(&keywords, &record.content), record))
            .collect();
        scored.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| right.1.at_ms.cmp(&left.1.at_ms))
                .then_with(|| right.1.id.cmp(&left.1.id))
        });
        Ok(scored
            .into_iter()
            .take(limit.get())
            .map(|(_, record)| record.clone())
            .collect())
    }

    async fn forget(&self, id: &MemoryId) -> Result<(), MemoryError> {
        self.records.lock().await.retain(|record| record.id != *id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limit(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).expect("非零")
    }

    async fn seed(memory: &InMemoryMemory) {
        memory
            .remember("Rust 的所有权系统保证内存安全".to_owned(), 1_000)
            .await
            .unwrap();
        memory
            .remember("Python 的 GIL 限制并行执行".to_owned(), 2_000)
            .await
            .unwrap();
        memory
            .remember("Rust 的借用检查器在编译期发现错误".to_owned(), 3_000)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn recall_ranks_by_keyword_then_recency() {
        let memory = InMemoryMemory::new();
        seed(&memory).await;

        let results = memory.recall("Rust 借用", limit(10)).await.unwrap();
        assert_eq!(results.len(), 3);
        // 命中 2 个关键字（rust/借用）优先；命中 1 个（rust）次之；未命中按时间倒序
        assert_eq!(results[0].content, "Rust 的借用检查器在编译期发现错误");
        assert_eq!(results[1].content, "Rust 的所有权系统保证内存安全");
        assert_eq!(results[2].content, "Python 的 GIL 限制并行执行");
    }

    #[tokio::test]
    async fn recall_respects_limit() {
        let memory = InMemoryMemory::new();
        seed(&memory).await;
        assert_eq!(memory.recall("Rust", limit(2)).await.unwrap().len(), 2);
        assert_eq!(memory.recall("Rust", limit(1)).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn empty_query_returns_recent_first() {
        let memory = InMemoryMemory::new();
        seed(&memory).await;
        let results = memory.recall("  ！！！  ", limit(10)).await.unwrap();
        assert_eq!(results[0].at_ms, 3_000);
        assert_eq!(results[1].at_ms, 2_000);
        assert_eq!(results[2].at_ms, 1_000);
    }

    #[tokio::test]
    async fn recall_is_deterministic() {
        let memory = InMemoryMemory::new();
        seed(&memory).await;
        let first = memory.recall("rust", limit(10)).await.unwrap();
        let second = memory.recall("rust", limit(10)).await.unwrap();
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn forget_is_idempotent() {
        let memory = InMemoryMemory::new();
        let record = memory.remember("临时记忆".to_owned(), 0).await.unwrap();
        assert_eq!(memory.len().await, 1);

        memory.forget(&record.id).await.unwrap();
        memory.forget(&record.id).await.unwrap();
        assert!(memory.is_empty().await);
    }

    #[test]
    fn record_json_roundtrip() {
        let record = MemoryRecord::new("内容", 123);
        let json = serde_json::to_string(&record).unwrap();
        let decoded: MemoryRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, record);
        // UUID v7（时间有序）：版本号为 7
        assert_eq!(record.id.as_uuid().get_version_num(), 7);
    }
}
