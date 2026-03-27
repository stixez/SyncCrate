use crate::state::{FileManifest, SyncAction, SyncPlan};
use sha2::{Digest, Sha256};

pub fn compute_diff(local: &FileManifest, remote: &FileManifest) -> SyncPlan {
    let mut actions = Vec::new();
    let mut total_bytes = 0u64;

    // Files in remote but not in local → receive
    for (path, remote_info) in &remote.files {
        match local.files.get(path) {
            None => {
                total_bytes += remote_info.size;
                actions.push(SyncAction::ReceiveFromRemote(remote_info.clone()));
            }
            Some(local_info) => {
                if local_info.hash != remote_info.hash {
                    total_bytes += remote_info.size.max(local_info.size);
                    actions.push(SyncAction::Conflict {
                        local: local_info.clone(),
                        remote: remote_info.clone(),
                    });
                }
                // Same hash = in sync, no action needed
            }
        }
    }

    // Files in local but not in remote → send
    for (path, local_info) in &local.files {
        if !remote.files.contains_key(path) {
            total_bytes += local_info.size;
            actions.push(SyncAction::SendToRemote(local_info.clone()));
        }
    }

    SyncPlan {
        actions,
        total_bytes,
        excluded: Vec::new(),
        resumed_files: 0,
    }
}

/// Compute a deterministic hash of a sync plan's actions.
/// Used to detect if the plan has changed between sessions.
pub fn compute_plan_hash(plan: &SyncPlan) -> String {
    let mut entries: Vec<String> = plan.actions.iter().map(|action| {
        match action {
            SyncAction::SendToRemote(f) => format!("send:{}:{}", f.relative_path, f.hash),
            SyncAction::ReceiveFromRemote(f) => format!("recv:{}:{}", f.relative_path, f.hash),
            SyncAction::Conflict { local, remote } => {
                format!("conflict:{}:{}:{}", local.relative_path, local.hash, remote.hash)
            }
            SyncAction::Delete(p) => format!("delete:{}", p),
        }
    }).collect();
    entries.sort();

    let mut hasher = Sha256::new();
    for entry in &entries {
        hasher.update(entry.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}
