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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{FileInfo, FileManifest};
    use std::collections::HashMap;

    fn make_file(path: &str, hash: &str, size: u64) -> FileInfo {
        FileInfo {
            relative_path: path.to_string(),
            size,
            hash: hash.to_string(),
            modified: 1000,
            file_type: "Mod".to_string(),
        }
    }

    fn make_manifest(files: Vec<FileInfo>) -> FileManifest {
        let mut map = HashMap::new();
        for f in files {
            map.insert(f.relative_path.clone(), f);
        }
        FileManifest {
            files: map,
            generated_at: 1000,
        }
    }

    #[test]
    fn test_diff_remote_only_receive() {
        let local = make_manifest(vec![]);
        let remote = make_manifest(vec![make_file("Mods/a.package", "abc123", 1000)]);
        let plan = compute_diff(&local, &remote);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(&plan.actions[0], SyncAction::ReceiveFromRemote(f) if f.relative_path == "Mods/a.package"));
        assert_eq!(plan.total_bytes, 1000);
    }

    #[test]
    fn test_diff_local_only_send() {
        let local = make_manifest(vec![make_file("Mods/b.package", "def456", 2000)]);
        let remote = make_manifest(vec![]);
        let plan = compute_diff(&local, &remote);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(&plan.actions[0], SyncAction::SendToRemote(f) if f.relative_path == "Mods/b.package"));
        assert_eq!(plan.total_bytes, 2000);
    }

    #[test]
    fn test_diff_same_hash_no_action() {
        let file = make_file("Mods/c.package", "same_hash", 500);
        let local = make_manifest(vec![file.clone()]);
        let remote = make_manifest(vec![file]);
        let plan = compute_diff(&local, &remote);
        assert_eq!(plan.actions.len(), 0);
        assert_eq!(plan.total_bytes, 0);
    }

    #[test]
    fn test_diff_different_hash_conflict() {
        let local = make_manifest(vec![make_file("Mods/d.package", "hash_a", 1000)]);
        let remote = make_manifest(vec![make_file("Mods/d.package", "hash_b", 2000)]);
        let plan = compute_diff(&local, &remote);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(&plan.actions[0], SyncAction::Conflict { .. }));
        assert_eq!(plan.total_bytes, 2000); // max(1000, 2000)
    }

    #[test]
    fn test_plan_hash_deterministic() {
        let local = make_manifest(vec![make_file("a.txt", "h1", 100)]);
        let remote = make_manifest(vec![make_file("b.txt", "h2", 200)]);
        let plan = compute_diff(&local, &remote);
        let hash1 = compute_plan_hash(&plan);
        let hash2 = compute_plan_hash(&plan);
        assert_eq!(hash1, hash2, "same plan should produce same hash");
    }

    #[test]
    fn test_plan_hash_different_plans() {
        let local1 = make_manifest(vec![]);
        let remote1 = make_manifest(vec![make_file("a.txt", "h1", 100)]);
        let plan1 = compute_diff(&local1, &remote1);

        let local2 = make_manifest(vec![]);
        let remote2 = make_manifest(vec![make_file("b.txt", "h2", 200)]);
        let plan2 = compute_diff(&local2, &remote2);

        assert_ne!(compute_plan_hash(&plan1), compute_plan_hash(&plan2));
    }
}
