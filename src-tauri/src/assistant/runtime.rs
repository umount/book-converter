//! Per-project assistant turn state: cancel + confirm channels.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Emitter};
use tokio::sync::{oneshot, Notify};

static CONFIRM_SEQ: AtomicU64 = AtomicU64::new(1);

const CONFIRM_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmDecision {
    Approved,
    Denied,
    Cancelled,
    TimedOut,
}

#[derive(Debug)]
pub(crate) struct TurnHandle {
    pub(crate) cancel: Arc<AtomicBool>,
    pub(crate) notify: Arc<Notify>,
}

pub(crate) struct PendingConfirmView {
    pub(crate) id: String,
    pub(crate) tool: String,
    pub(crate) args: String,
    pub(crate) heavy: bool,
}

struct PendingConfirm {
    id: String,
    tool: String,
    args: String,
    heavy: bool,
    tx: oneshot::Sender<bool>,
}

struct TurnSlot {
    cancel: Arc<AtomicBool>,
    notify: Arc<Notify>,
    confirm: Option<PendingConfirm>,
}

/// Managed runtime for in-flight assistant turns (one per project).
pub struct AssistantRuntime {
    turns: Mutex<HashMap<String, TurnSlot>>,
}

pub(crate) struct TurnGuard {
    runtime: Arc<AssistantRuntime>,
    project_id: String,
}

impl TurnGuard {
    pub(crate) fn new(runtime: Arc<AssistantRuntime>, project_id: String) -> Self {
        Self {
            runtime,
            project_id,
        }
    }
}

impl Drop for TurnGuard {
    fn drop(&mut self) {
        self.runtime.finish_turn(&self.project_id);
    }
}

impl AssistantRuntime {
    pub fn new() -> Self {
        Self {
            turns: Mutex::new(HashMap::new()),
        }
    }

    pub fn begin_turn(&self, project_id: &str) -> Result<TurnHandle, String> {
        let mut map = self.turns.lock().unwrap_or_else(|p| p.into_inner());
        if map.contains_key(project_id) {
            return Err("assistant_busy".into());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let notify = Arc::new(Notify::new());
        map.insert(
            project_id.to_string(),
            TurnSlot {
                cancel: cancel.clone(),
                notify: notify.clone(),
                confirm: None,
            },
        );
        Ok(TurnHandle { cancel, notify })
    }

    pub fn finish_turn(&self, project_id: &str) {
        let mut map = self.turns.lock().unwrap_or_else(|p| p.into_inner());
        map.remove(project_id);
    }

    pub fn cancel(&self, project_id: &str) {
        let mut map = self.turns.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(slot) = map.get_mut(project_id) {
            slot.cancel.store(true, Ordering::Relaxed);
            slot.notify.notify_waiters();
            if let Some(pending) = slot.confirm.take() {
                let _ = pending.tx.send(false);
            }
        }
    }

    pub fn is_running(&self, project_id: &str) -> bool {
        self.turns
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains_key(project_id)
    }

    pub fn pending(&self, project_id: &str) -> Option<PendingConfirmView> {
        self.turns
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(project_id)
            .and_then(|s| s.confirm.as_ref())
            .map(|p| PendingConfirmView {
                id: p.id.clone(),
                tool: p.tool.clone(),
                args: p.args.clone(),
                heavy: p.heavy,
            })
    }

    pub fn resolve_confirm(
        &self,
        project_id: &str,
        confirm_id: &str,
        approved: bool,
    ) -> Result<(), String> {
        let mut map = self.turns.lock().unwrap_or_else(|p| p.into_inner());
        let slot = map.get_mut(project_id).ok_or("no_assistant_turn")?;
        let pending = slot.confirm.take().ok_or("no_pending_confirm")?;
        if pending.id != confirm_id {
            slot.confirm = Some(pending);
            return Err("confirm_id_mismatch".into());
        }
        let _ = pending.tx.send(approved);
        Ok(())
    }

    /// Emit need_confirm and wait for approve/deny (or cancel / timeout).
    pub async fn request_confirm(
        &self,
        app: &AppHandle,
        project_id: &str,
        tool: &str,
        args: &str,
        heavy: bool,
    ) -> ConfirmDecision {
        let (tx, rx) = oneshot::channel();
        let id = format!("c{}", CONFIRM_SEQ.fetch_add(1, Ordering::Relaxed));
        {
            let mut map = self.turns.lock().unwrap_or_else(|p| p.into_inner());
            let Some(slot) = map.get_mut(project_id) else {
                return ConfirmDecision::Cancelled;
            };
            if slot.cancel.load(Ordering::Relaxed) {
                return ConfirmDecision::Cancelled;
            }
            slot.confirm = Some(PendingConfirm {
                id: id.clone(),
                tool: tool.to_string(),
                args: args.to_string(),
                heavy,
                tx,
            });
        }

        let _ = app.emit(
            "assistant_need_confirm",
            serde_json::json!({
                "project": project_id,
                "id": id,
                "tool": tool,
                "args": args,
                "heavy": heavy,
            }),
        );

        match tokio::time::timeout(CONFIRM_TIMEOUT, rx).await {
            Ok(Ok(true)) => ConfirmDecision::Approved,
            Ok(Ok(false)) => {
                let cancelled = self
                    .turns
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .get(project_id)
                    .map(|s| s.cancel.load(Ordering::Relaxed))
                    .unwrap_or(true);
                if cancelled {
                    ConfirmDecision::Cancelled
                } else {
                    ConfirmDecision::Denied
                }
            }
            Ok(Err(_)) => ConfirmDecision::Cancelled,
            Err(_) => {
                let mut map = self.turns.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(slot) = map.get_mut(project_id) {
                    slot.confirm = None;
                }
                let _ = app.emit(
                    "assistant_confirm_expired",
                    serde_json::json!({ "project": project_id, "id": id }),
                );
                ConfirmDecision::TimedOut
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_turn_twice_is_busy() {
        let rt = AssistantRuntime::new();
        rt.begin_turn("p").unwrap();
        assert_eq!(rt.begin_turn("p").unwrap_err(), "assistant_busy");
        assert!(rt.is_running("p"));
        rt.finish_turn("p");
        assert!(!rt.is_running("p"));
    }

    #[tokio::test]
    async fn cancel_resolves_confirm_as_cancelled() {
        let rt = Arc::new(AssistantRuntime::new());
        let _h = rt.begin_turn("p").unwrap();
        let rt2 = rt.clone();
        let waiter = tokio::spawn(async move {
            // AppHandle is required to emit; skip emit by resolving via cancel.
            let (tx, rx) = oneshot::channel::<bool>();
            {
                let mut map = rt2.turns.lock().unwrap();
                map.get_mut("p").unwrap().confirm = Some(PendingConfirm {
                    id: "c1".into(),
                    tool: "x".into(),
                    args: "{}".into(),
                    heavy: false,
                    tx,
                });
            }
            match rx.await {
                Ok(false) => ConfirmDecision::Cancelled,
                _ => ConfirmDecision::Denied,
            }
        });
        // Let the task park on rx.
        tokio::task::yield_now().await;
        rt.cancel("p");
        assert_eq!(waiter.await.unwrap(), ConfirmDecision::Cancelled);
    }

    #[test]
    fn pending_is_visible() {
        let rt = AssistantRuntime::new();
        rt.begin_turn("p").unwrap();
        let (tx, _rx) = oneshot::channel();
        {
            let mut map = rt.turns.lock().unwrap();
            map.get_mut("p").unwrap().confirm = Some(PendingConfirm {
                id: "c9".into(),
                tool: "reset_translation".into(),
                args: "{}".into(),
                heavy: true,
                tx,
            });
        }
        let view = rt.pending("p").unwrap();
        assert_eq!(view.id, "c9");
        assert!(view.heavy);
    }

    #[test]
    fn turn_guard_releases_on_drop() {
        let rt = Arc::new(AssistantRuntime::new());
        rt.begin_turn("p").unwrap();
        {
            let _g = TurnGuard::new(rt.clone(), "p".into());
            assert!(rt.is_running("p"));
        }
        assert!(!rt.is_running("p"));
    }
}
