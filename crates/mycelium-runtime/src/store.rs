//! 会话持久化：可插拔存储后端。
//!
//! 语义（FC-POST-181 / FC-POST-182 / FC-ERR-070）：
//! - [`SessionStore::load`] 会话不存在时返回 `Ok(None)`；数据损坏返回 `Corrupted`（显式失败）；
//! - [`SessionStore::save`] 覆盖写；[`SessionStore::delete`] 幂等（不存在视为成功）；
//! - 内置实现：[`InMemorySessionStore`]（进程内）与 [`FileSessionStore`]（单文件 JSON，原子写）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use mycelium_core::SessionId;

use crate::session::Session;

/// 会话存储错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SessionStoreError {
    /// IO 失败（含路径上下文）。
    #[error("会话存储 IO 失败：{message}")]
    Io {
        /// 失败描述（含路径）。
        message: String,
    },
    /// 会话数据损坏（无法反序列化）。
    #[error("会话数据损坏：{message}")]
    Corrupted {
        /// 违约细节（含路径）。
        message: String,
    },
}

/// 会话存储（SPI 扩展点）。
#[async_trait]
pub trait SessionStore: Send + Sync + 'static {
    /// 读取会话；不存在时返回 `Ok(None)`。
    ///
    /// # Errors
    /// IO 失败 → [`SessionStoreError::Io`]；数据损坏 → [`SessionStoreError::Corrupted`]。
    async fn load(&self, session_id: &SessionId) -> Result<Option<Session>, SessionStoreError>;

    /// 写入（覆盖）会话。
    ///
    /// # Errors
    /// IO 失败 → [`SessionStoreError::Io`]。
    async fn save(&self, session: &Session) -> Result<(), SessionStoreError>;

    /// 删除会话（不存在视为成功，幂等）。
    ///
    /// # Errors
    /// IO 失败 → [`SessionStoreError::Io`]。
    async fn delete(&self, session_id: &SessionId) -> Result<(), SessionStoreError>;
}

/// 进程内存储（测试与短生命周期场景）。
#[derive(Debug, Clone, Default)]
pub struct InMemorySessionStore {
    sessions: Arc<Mutex<HashMap<SessionId, Session>>>,
}

impl InMemorySessionStore {
    /// 创建空存储。
    pub fn new() -> Self {
        Self::default()
    }

    /// 已存会话数量。
    pub async fn len(&self) -> usize {
        self.sessions.lock().await.len()
    }

    /// 是否为空。
    pub async fn is_empty(&self) -> bool {
        self.sessions.lock().await.is_empty()
    }
}

#[async_trait]
impl SessionStore for InMemorySessionStore {
    async fn load(&self, session_id: &SessionId) -> Result<Option<Session>, SessionStoreError> {
        Ok(self.sessions.lock().await.get(session_id).cloned())
    }

    async fn save(&self, session: &Session) -> Result<(), SessionStoreError> {
        self.sessions
            .lock()
            .await
            .insert(session.id(), session.clone());
        Ok(())
    }

    async fn delete(&self, session_id: &SessionId) -> Result<(), SessionStoreError> {
        self.sessions.lock().await.remove(session_id);
        Ok(())
    }
}

/// 单文件 JSON 存储：`<directory>/<session_id>.json`。
///
/// 写入使用「临时文件 + 重命名」保证原子性（读者不会观察到半写文件）。
#[derive(Debug, Clone)]
pub struct FileSessionStore {
    directory: PathBuf,
}

impl FileSessionStore {
    /// 创建存储（目录在首次写入时自动建立）。
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    /// 存储目录。
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// 会话文件路径。
    fn path_for(&self, session_id: &SessionId) -> PathBuf {
        self.directory.join(format!("{session_id}.json"))
    }
}

#[async_trait]
impl SessionStore for FileSessionStore {
    async fn load(&self, session_id: &SessionId) -> Result<Option<Session>, SessionStoreError> {
        let path = self.path_for(session_id);
        match tokio::fs::read(&path).await {
            Ok(bytes) => {
                let session = serde_json::from_slice::<Session>(&bytes).map_err(|error| {
                    SessionStoreError::Corrupted {
                        message: format!("{}：{error}", path.display()),
                    }
                })?;
                Ok(Some(session))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(SessionStoreError::Io {
                message: format!("{}：{error}", path.display()),
            }),
        }
    }

    async fn save(&self, session: &Session) -> Result<(), SessionStoreError> {
        let path = self.path_for(&session.id());
        let bytes = serde_json::to_vec_pretty(session).map_err(|error| SessionStoreError::Io {
            message: format!("{}：序列化失败：{error}", path.display()),
        })?;
        tokio::fs::create_dir_all(&self.directory)
            .await
            .map_err(|error| SessionStoreError::Io {
                message: format!("{}：创建目录失败：{error}", self.directory.display()),
            })?;
        let temp_path = path.with_extension("json.tmp");
        tokio::fs::write(&temp_path, &bytes)
            .await
            .map_err(|error| SessionStoreError::Io {
                message: format!("{}：写入失败：{error}", temp_path.display()),
            })?;
        tokio::fs::rename(&temp_path, &path)
            .await
            .map_err(|error| SessionStoreError::Io {
                message: format!("{}：重命名失败：{error}", path.display()),
            })
    }

    async fn delete(&self, session_id: &SessionId) -> Result<(), SessionStoreError> {
        let path = self.path_for(session_id);
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(SessionStoreError::Io {
                message: format!("{}：{error}", path.display()),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use mycelium_core::message::ToolResultBlock;
    use mycelium_core::{ContentBlock, Message};

    use super::*;

    fn sample_session() -> Session {
        let mut session = Session::new(1_700_000_000_000);
        session.push(Message::user_text("你好"));
        session.push(Message::assistant_text("你好！"));
        session
    }

    #[tokio::test]
    async fn in_memory_round_trip() {
        let store = InMemorySessionStore::new();
        assert!(store.is_empty().await);
        let session = sample_session();

        store.save(&session).await.unwrap();
        assert_eq!(store.len().await, 1);
        let loaded = store.load(&session.id()).await.unwrap();
        assert_eq!(loaded, Some(session.clone()));

        store.delete(&session.id()).await.unwrap();
        assert_eq!(store.load(&session.id()).await.unwrap(), None);
        assert!(store.is_empty().await);
    }

    #[tokio::test]
    async fn load_missing_is_none() {
        let session_id = SessionId::generate();
        let memory = InMemorySessionStore::new();
        assert_eq!(memory.load(&session_id).await.unwrap(), None);

        let dir = tempfile::tempdir().unwrap();
        let file = FileSessionStore::new(dir.path());
        assert_eq!(file.load(&session_id).await.unwrap(), None);
    }

    #[tokio::test]
    async fn delete_is_idempotent() {
        let session_id = SessionId::generate();
        let memory = InMemorySessionStore::new();
        memory.delete(&session_id).await.unwrap();
        memory.delete(&session_id).await.unwrap();

        let dir = tempfile::tempdir().unwrap();
        let file = FileSessionStore::new(dir.path());
        file.delete(&session_id).await.unwrap();
        file.delete(&session_id).await.unwrap();
    }

    #[tokio::test]
    async fn file_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileSessionStore::new(dir.path());
        let session = sample_session();

        store.save(&session).await.unwrap();
        assert!(
            dir.path()
                .join(format!("{}.json", session.id().as_uuid()))
                .exists()
        );

        let loaded = store.load(&session.id()).await.unwrap();
        assert_eq!(loaded, Some(session.clone()));

        store.delete(&session.id()).await.unwrap();
        assert_eq!(store.load(&session.id()).await.unwrap(), None);
    }

    #[tokio::test]
    async fn file_round_trip_preserves_structured_content() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileSessionStore::new(dir.path());
        let mut session = Session::new(42);
        session.push(Message::assistant_text("看截图"));
        session.push(Message::tool_result_blocks(
            mycelium_core::ToolCallId::new("call_1").unwrap(),
            vec![
                ToolResultBlock::Text {
                    text: "截图如下".to_owned(),
                },
                ToolResultBlock::Image {
                    media_type: "image/png".to_owned(),
                    data: "aGVsbG8=".to_owned(),
                },
            ],
            false,
        ));
        session.push(
            Message::new(
                mycelium_core::Role::Assistant,
                vec![ContentBlock::Thinking {
                    text: "思考中".to_owned(),
                }],
            )
            .unwrap(),
        );

        store.save(&session).await.unwrap();
        let loaded = store.load(&session.id()).await.unwrap().unwrap();
        assert_eq!(loaded, session);
    }

    #[tokio::test]
    async fn corrupted_file_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileSessionStore::new(dir.path());
        let session_id = SessionId::generate();
        tokio::fs::write(store.path_for(&session_id), b"{ not valid json")
            .await
            .unwrap();

        let error = store.load(&session_id).await.unwrap_err();
        assert!(
            matches!(error, SessionStoreError::Corrupted { .. }),
            "期望 Corrupted，实际：{error:?}"
        );
    }
}
