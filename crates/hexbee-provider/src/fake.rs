use std::collections::VecDeque;
use std::sync::Mutex;

use crate::traits::{BoxFuture, LlmClient, LlmError, LlmResult};

/// Scripted LLM for deterministic tests.
#[derive(Debug, Default)]
pub struct FakeLlm {
    replies: Mutex<VecDeque<String>>,
}

impl FakeLlm {
    pub fn new(replies: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            replies: Mutex::new(replies.into_iter().map(Into::into).collect()),
        }
    }

    pub fn push(&self, reply: impl Into<String>) {
        self.replies.lock().unwrap().push_back(reply.into());
    }
}

impl LlmClient for FakeLlm {
    fn complete<'a>(
        &'a self,
        _system: &'a str,
        _user: &'a str,
    ) -> BoxFuture<'a, LlmResult<String>> {
        Box::pin(async move {
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| LlmError::Unavailable("FakeLlm queue empty".into()))
        })
    }

    fn complete_json<'a>(
        &'a self,
        system: &'a str,
        user: &'a str,
    ) -> BoxFuture<'a, LlmResult<String>> {
        self.complete(system, user)
    }
}
