//! Per-project assistant turn state: cancel + confirm channels.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;

static CONFIRM_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmDecision {
    Approved,
    Denied,
    Cancelled,
}

struct PendingConfirm {
    id: String,
    tx: oneshot::Sender<bool>,
}

struct TurnSlot {
    cancel: Arc<AtomicBool>,
    confirm: Option<PendingConfirm>,
}

/// Managed runtime for in-flight assistant turns (one per project).
pub struct AssistantRuntime {
    turns: Mutex<HashMap<String, TurnSlot>>,
}

impl AssistantRuntime {
    pub fn new() -> Self {
        Self {
            turns: Mutex::new(HashMap::new()),
        }
    }

    pub fn begin_turn(&self, project_id: &str) -> Result<Arc<AtomicBool>, String> {
        let mut map = self.turns.lock().unwrap_or_else(|p| p.into_inner());
        if map.contains_key(project_id) {
            return Err("assistant_busy".into());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        map.insert(
            project_id.to_string(),
            TurnSlot {
                cancel: cancel.clone(),
                confirm: None,
            },
        );
        Ok(cancel)
    }

    pub fn finish_turn(&self, project_id: &str) {
        let mut map = self.turns.lock().unwrap_or_else(|p| p.into_inner());
        map.remove(project_id);
    }

    pub fn cancel(&self, project_id: &str) {
        let mut map = self.turns.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(slot) = map.get_mut(project_id) {
            slot.cancel.store(true, Ordering::Relaxed);
            if let Some(pending) = slot.confirm.take() {
                let _ = pending.tx.send(false);
            }
        }
    }

    pub fn resolve_confirm(&self, project_id: &str, confirm_id: &str, approved: bool) -> Result<(), String> {
        let mut map = self.turns.lock().unwrap_or_else(|p| p.into_inner());
        let slot = map.get_mut(project_id).ok_or("no_assistant_turn")?;
        let pending = slot.confirm.take().ok_or("no_pending_confirm")?;
        if pending.id != confirm_id {
            // Put it back if id mismatch.
            slot.confirm = Some(pending);
            return Err("confirm_id_mismatch".into());
        }
        let _ = pending.tx.send(approved);
        Ok(())
    }

    /// Emit need_confirm and wait for approve/deny (or cancel).
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

        match rx.await {
            Ok(true) => ConfirmDecision::Approved,
            Ok(false) => {
                // Distinguish cancel vs deny: if cancel flag set, treat as cancelled.
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
            Err(_) => ConfirmDecision::Cancelled,
        }
    }
}
