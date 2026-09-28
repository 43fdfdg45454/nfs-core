//! nfsd's administration, where the tests run on its machine as root (the CI): its view of the
//! test clients (/proc/fs/nfsd/clients), expiring one, restarting it.

use std::path::PathBuf;

/// The directories nfsd keeps for clients whose owner contains `owner`, or `None` where they
/// are not readable (another server, not root).
fn clients(owner: &str) -> Option<Vec<PathBuf>> {
    let all = std::fs::read_dir("/proc/fs/nfsd/clients").ok()?.flatten();
    let has = |path: &PathBuf| {
        std::fs::read_to_string(path.join("info")).is_ok_and(|info| info.contains(owner))
    };
    Some(all.map(|c| c.path()).filter(has).collect())
}

/// Open states nfsd holds for the client with this owner.
pub fn server_opens(owner: &str) -> Option<usize> {
    let owner = format!("\"{owner}\"");
    let states = |path: &PathBuf| std::fs::read_to_string(path.join("states")).unwrap_or_default();
    Some(clients(&owner)?.iter().map(|c| states(c).matches("type: open").count()).sum())
}

/// Makes nfsd forget the client with exactly this owner, as when its lease expires; false where
/// nfsd cannot be reached this way.
pub fn expire(owner: &str) -> bool {
    let Some(found) = clients(&format!("\"{owner}\"")) else { return false };
    assert_eq!(found.len(), 1, "clients with owner {owner}");
    std::fs::write(found[0].join("ctl"), "expire").is_ok()
}

/// Restarts nfsd: sessions, open state and unstable writes are lost (grace time: ci/nfsd.sh).
pub fn restart_nfsd() {
    super::run("systemctl restart nfs-kernel-server");
}
