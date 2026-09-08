use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::scan::FileEntry;

pub const COL_OLD: &str = "当前文件名称";
pub const COL_NEW: &str = "新文件名称";

const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowAction {
    Skip {
        old_name: String,
        reason: String,
    },
    Rename {
        old_name: String,
        new_name: String,
    },
    Reject {
        old_name: String,
        new_name: String,
        reason: String,
    },
}

#[derive(Debug, Clone)]
pub struct Preview {
    pub actions: Vec<RowAction>,
    /// 若存在，整批不得执行（例如新文件名重复）。
    pub blocking_error: Option<String>,
}

impl Preview {
    pub fn rename_count(&self) -> usize {
        self.actions
            .iter()
            .filter(|a| matches!(a, RowAction::Rename { .. }))
            .count()
    }

    pub fn skip_count(&self) -> usize {
        self.actions
            .iter()
            .filter(|a| matches!(a, RowAction::Skip { .. }))
            .count()
    }

    pub fn reject_count(&self) -> usize {
        self.actions
            .iter()
            .filter(|a| matches!(a, RowAction::Reject { .. }))
            .count()
    }

    pub fn planned_renames(&self) -> Vec<(String, String)> {
        self.actions
            .iter()
            .filter_map(|a| match a {
                RowAction::Rename { old_name, new_name } => {
                    Some((old_name.clone(), new_name.clone()))
                }
                _ => None,
            })
            .collect()
    }
}

pub fn validate_filename(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("文件名为空".into());
    }
    if name == "." || name == ".." {
        return Err("文件名不能是 . 或 ..".into());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("新文件名称不能包含路径分隔符，只能是文件名".into());
    }
    for ch in name.chars() {
        if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
            return Err(format!("文件名包含非法字符: {ch:?}"));
        }
    }
    if name.ends_with(' ') || name.ends_with('.') {
        return Err("Windows 不允许文件名以空格或点结尾".into());
    }
    if name.len() > 255 {
        return Err("文件名过长（超过 255 字节/字符）".into());
    }
    let stem = name.split('.').next().unwrap_or(name);
    if WINDOWS_RESERVED
        .iter()
        .any(|r| stem.eq_ignore_ascii_case(r))
    {
        return Err(format!("Windows 保留设备名不可用作文件名: {stem}"));
    }
    Ok(())
}

pub fn build_preview(root: &Path, rows: &[(String, String)], disk_files: &[FileEntry]) -> Preview {
    let on_disk: HashSet<String> = disk_files.iter().map(|f| f.name.clone()).collect();
    let mut actions = Vec::new();
    let mut planned_new: HashMap<String, String> = HashMap::new();
    let mut duplicate_news: HashSet<String> = HashSet::new();

    for (old_raw, new_raw) in rows {
        let old_name = old_raw.trim().to_string();
        let new_name = new_raw.trim().to_string();
        if old_name.is_empty() {
            continue;
        }
        if new_name.is_empty() || new_name == old_name {
            actions.push(RowAction::Skip {
                old_name,
                reason: if new_raw.trim().is_empty() {
                    "新文件名称为空，跳过".into()
                } else {
                    "新旧名称相同，跳过".into()
                },
            });
            continue;
        }
        if let Err(reason) = validate_filename(&new_name) {
            actions.push(RowAction::Reject {
                old_name,
                new_name,
                reason,
            });
            continue;
        }
        if planned_new.insert(new_name.clone(), old_name.clone()).is_some() {
            duplicate_news.insert(new_name.clone());
        }
        actions.push(RowAction::Rename { old_name, new_name });
    }

    if !duplicate_news.is_empty() {
        let list: Vec<_> = duplicate_news.iter().cloned().collect();
        return Preview {
            actions,
            blocking_error: Some(format!(
                "存在重复的新文件名称，整批拒绝执行: {}",
                list.join("、")
            )),
        };
    }

    let moving: HashSet<String> = actions
        .iter()
        .filter_map(|a| match a {
            RowAction::Rename { old_name, .. } => Some(old_name.clone()),
            _ => None,
        })
        .collect();

    for action in &mut actions {
        if let RowAction::Rename { old_name, new_name } = action {
            if !on_disk.contains(old_name) {
                *action = RowAction::Reject {
                    old_name: old_name.clone(),
                    new_name: new_name.clone(),
                    reason: "当前文件在目录中不存在".into(),
                };
                continue;
            }
            let target_exists = root.join(&*new_name).exists();
            let target_will_move_away = moving.contains(new_name.as_str());
            if target_exists && !target_will_move_away {
                *action = RowAction::Reject {
                    old_name: old_name.clone(),
                    new_name: new_name.clone(),
                    reason: "目标文件名已存在，拒绝覆盖".into(),
                };
            }
        }
    }

    Preview {
        actions,
        blocking_error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn skips_empty_and_same() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"a").unwrap();
        let files = vec![FileEntry {
            name: "a.txt".into(),
            size: 1,
        }];
        let preview = build_preview(
            dir.path(),
            &[
                ("a.txt".into(), "".into()),
                ("a.txt".into(), "a.txt".into()),
            ],
            &files,
        );
        assert_eq!(preview.skip_count(), 2);
        assert_eq!(preview.rename_count(), 0);
    }

    #[test]
    fn rejects_duplicate_new_names() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"a").unwrap();
        std::fs::write(dir.path().join("b.txt"), b"b").unwrap();
        let files = vec![
            FileEntry {
                name: "a.txt".into(),
                size: 1,
            },
            FileEntry {
                name: "b.txt".into(),
                size: 1,
            },
        ];
        let preview = build_preview(
            dir.path(),
            &[
                ("a.txt".into(), "c.txt".into()),
                ("b.txt".into(), "c.txt".into()),
            ],
            &files,
        );
        assert!(preview.blocking_error.is_some());
    }

    #[test]
    fn rejects_overwrite() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"a").unwrap();
        std::fs::write(dir.path().join("keep.txt"), b"k").unwrap();
        let files = vec![
            FileEntry {
                name: "a.txt".into(),
                size: 1,
            },
            FileEntry {
                name: "keep.txt".into(),
                size: 1,
            },
        ];
        let preview = build_preview(
            dir.path(),
            &[("a.txt".into(), "keep.txt".into())],
            &files,
        );
        assert_eq!(preview.reject_count(), 1);
        assert_eq!(preview.rename_count(), 0);
    }

    #[test]
    fn allows_swap() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"a").unwrap();
        std::fs::write(dir.path().join("b.txt"), b"b").unwrap();
        let files = vec![
            FileEntry {
                name: "a.txt".into(),
                size: 1,
            },
            FileEntry {
                name: "b.txt".into(),
                size: 1,
            },
        ];
        let preview = build_preview(
            dir.path(),
            &[
                ("a.txt".into(), "b.txt".into()),
                ("b.txt".into(), "a.txt".into()),
            ],
            &files,
        );
        assert!(preview.blocking_error.is_none());
        assert_eq!(preview.rename_count(), 2);
    }

    #[test]
    fn rejects_illegal_chars() {
        assert!(validate_filename("a:b.txt").is_err());
        assert!(validate_filename("CON.txt").is_err());
        assert!(validate_filename("ok.fastq.gz").is_ok());
    }
}
