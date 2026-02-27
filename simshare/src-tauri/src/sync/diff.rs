use crate::state::{FileManifest, SyncAction, SyncPlan};

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
                    total_bytes += remote_info.size + local_info.size;
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
    }
}
