use std::fs;
use std::path::Path;

use uuid::Uuid;

use crate::ledger::{write_ledger, EntryStatus, Ledger, LedgerError};

#[derive(Debug, Clone)]
pub struct RenameOutcome {
    pub ledger_path: std::path::PathBuf,
    pub ledger: Ledger,
    pub success: usize,
    pub failed: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum RenameError {
    #[error(transparent)]
    Ledger(#[from] LedgerError),
    #[error("没有可执行的改名项")]
    Empty,
}

struct Staged {
    old_name: String,
    new_name: String,
    temp_name: String,
}

fn unique_temp_name(root: &Path) -> String {
    loop {
        let name = format!(".__batch_rename_tmp_{}", Uuid::new_v4().simple());
        if !root.join(&name).exists() {
            return name;
        }
    }
}

fn rename_file(root: &Path, from: &str, to: &str) -> Result<(), String> {
    fs::rename(root.join(from), root.join(to)).map_err(|e| format!("{from} → {to}: {e}"))
}

/// 两阶段改名：先全部改到临时名，再改到最终名，以支持交换/循环。
pub fn apply_renames(
    root: &Path,
    pairs: &[(String, String)],
    backup_dir: &Path,
) -> Result<RenameOutcome, RenameError> {
    if pairs.is_empty() {
        return Err(RenameError::Empty);
    }

    let mut ledger = Ledger::new(root, pairs);
    let ledger_path = write_ledger(backup_dir, &ledger)?;

    let mut staged: Vec<Staged> = Vec::new();
    let mut phase1_ok = true;

    for (old_name, new_name) in pairs {
        if !phase1_ok {
            mark_failed(&mut ledger, old_name, "未执行：前置改名已失败");
            continue;
        }
        if !root.join(old_name).is_file() {
            mark_failed(&mut ledger, old_name, "当前文件不存在");
            continue;
        }
        let temp_name = unique_temp_name(root);
        match rename_file(root, old_name, &temp_name) {
            Ok(()) => staged.push(Staged {
                old_name: old_name.clone(),
                new_name: new_name.clone(),
                temp_name,
            }),
            Err(e) => {
                phase1_ok = false;
                restore_temps(root, &staged);
                mark_failed(&mut ledger, old_name, &e);
                for s in &staged {
                    mark_failed(&mut ledger, &s.old_name, "已回退临时名：第一阶段失败");
                }
                staged.clear();
            }
        }
    }

    for s in &staged {
        match rename_file(root, &s.temp_name, &s.new_name) {
            Ok(()) => mark_success(&mut ledger, &s.old_name),
            Err(e) => {
                if let Err(back) = rename_file(root, &s.temp_name, &s.old_name) {
                    mark_failed(
                        &mut ledger,
                        &s.old_name,
                        &format!("{e}；且无法恢复原名: {back}"),
                    );
                } else {
                    mark_failed(&mut ledger, &s.old_name, &e);
                }
            }
        }
    }

    write_ledger(backup_dir, &ledger)?;
    let success = ledger
        .entries
        .iter()
        .filter(|e| e.status == EntryStatus::Success)
        .count();
    let failed = ledger
        .entries
        .iter()
        .filter(|e| e.status == EntryStatus::Failed)
        .count();
    Ok(RenameOutcome {
        ledger_path,
        ledger,
        success,
        failed,
    })
}

pub fn rollback_ledger(root: &Path, ledger: &Ledger) -> Result<RenameOutcome, RenameError> {
    let pairs: Vec<(String, String)> = ledger
        .entries
        .iter()
        .filter(|e| e.status == EntryStatus::Success)
        .map(|e| (e.new_name.clone(), e.old_name.clone()))
        .collect();
    if pairs.is_empty() {
        return Err(RenameError::Empty);
    }

    let backup_dir = root.join(crate::ledger::BACKUP_DIR_NAME).join(format!(
        "rollback_{}",
        chrono::Local::now().format("%Y%m%d_%H%M%S")
    ));
    std::fs::create_dir_all(&backup_dir).map_err(LedgerError::from)?;
    apply_renames(root, &pairs, &backup_dir)
}

fn mark_success(ledger: &mut Ledger, old_name: &str) {
    if let Some(e) = ledger.entries.iter_mut().find(|e| e.old_name == old_name) {
        e.status = EntryStatus::Success;
        e.error = None;
    }
}

fn mark_failed(ledger: &mut Ledger, old_name: &str, reason: &str) {
    if let Some(e) = ledger.entries.iter_mut().find(|e| e.old_name == old_name) {
        e.status = EntryStatus::Failed;
        e.error = Some(reason.to_string());
    }
}

fn restore_temps(root: &Path, staged: &[Staged]) {
    for s in staged.iter().rev() {
        let _ = rename_file(root, &s.temp_name, &s.old_name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn swaps_two_files() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"AAA").unwrap();
        fs::write(dir.path().join("b.txt"), b"BBB").unwrap();
        let backup = dir.path().join("rename_backup").join("t");
        fs::create_dir_all(&backup).unwrap();
        let out = apply_renames(
            dir.path(),
            &[
                ("a.txt".into(), "b.txt".into()),
                ("b.txt".into(), "a.txt".into()),
            ],
            &backup,
        )
        .unwrap();
        assert_eq!(out.success, 2);
        assert_eq!(fs::read(dir.path().join("a.txt")).unwrap(), b"BBB");
        assert_eq!(fs::read(dir.path().join("b.txt")).unwrap(), b"AAA");
    }

    #[test]
    fn rollback_restores_names() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("old.bam"), b"data").unwrap();
        let backup = dir.path().join("rename_backup").join("t1");
        fs::create_dir_all(&backup).unwrap();
        let out = apply_renames(
            dir.path(),
            &[("old.bam".into(), "new.bam".into())],
            &backup,
        )
        .unwrap();
        assert!(dir.path().join("new.bam").is_file());
        rollback_ledger(dir.path(), &out.ledger).unwrap();
        assert!(dir.path().join("old.bam").is_file());
        assert!(!dir.path().join("new.bam").exists());
    }

    #[test]
    fn chain_rename() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"A").unwrap();
        fs::write(dir.path().join("b.txt"), b"B").unwrap();
        let backup = dir.path().join("rename_backup").join("t");
        fs::create_dir_all(&backup).unwrap();
        apply_renames(
            dir.path(),
            &[
                ("a.txt".into(), "b.txt".into()),
                ("b.txt".into(), "c.txt".into()),
            ],
            &backup,
        )
        .unwrap();
        assert_eq!(fs::read(dir.path().join("b.txt")).unwrap(), b"A");
        assert_eq!(fs::read(dir.path().join("c.txt")).unwrap(), b"B");
        assert!(!dir.path().join("a.txt").exists());
    }
}
