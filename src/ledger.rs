use std::fs;
use std::path::{Path, PathBuf};

use chrono::Local;
use serde::{Deserialize, Serialize};

use crate::excel::ExcelError;
use crate::validate::{COL_NEW, COL_OLD};

pub const BACKUP_DIR_NAME: &str = "rename_backup";
pub const LEDGER_JSON: &str = "rename_map.json";
pub const LEDGER_XLSX: &str = "rename_map.xlsx";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntryStatus {
    Planned,
    Success,
    Failed,
    RolledBack,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub old_name: String,
    pub new_name: String,
    pub status: EntryStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ledger {
    pub version: u32,
    pub root: PathBuf,
    pub created_at: String,
    pub entries: Vec<LedgerEntry>,
}

#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    #[error("账本 IO 失败: {0}")]
    Io(#[from] std::io::Error),
    #[error("账本 JSON 失败: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Excel(#[from] ExcelError),
    #[error("找不到改名账本")]
    NotFound,
}

impl Ledger {
    pub fn new(root: &Path, pairs: &[(String, String)]) -> Self {
        Self {
            version: 1,
            root: root.to_path_buf(),
            created_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            entries: pairs
                .iter()
                .map(|(old, new)| LedgerEntry {
                    old_name: old.clone(),
                    new_name: new.clone(),
                    status: EntryStatus::Planned,
                    error: None,
                })
                .collect(),
        }
    }

    pub fn json_path(dir: &Path) -> PathBuf {
        dir.join(LEDGER_JSON)
    }
}

pub fn backup_root(data_root: &Path) -> PathBuf {
    data_root.join(BACKUP_DIR_NAME)
}

pub fn create_backup_dir(data_root: &Path) -> Result<PathBuf, LedgerError> {
    let stamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
    let dir = backup_root(data_root).join(stamp);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn write_ledger(dir: &Path, ledger: &Ledger) -> Result<PathBuf, LedgerError> {
    let json_path = dir.join(LEDGER_JSON);
    fs::write(&json_path, serde_json::to_vec_pretty(ledger)?)?;

    let xlsx_path = dir.join(LEDGER_XLSX);
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name("改名账本")
        .map_err(|e| ExcelError::Write(e.to_string()))?;
    sheet
        .write_string(0, 0, COL_OLD)
        .map_err(|e| ExcelError::Write(e.to_string()))?;
    sheet
        .write_string(0, 1, COL_NEW)
        .map_err(|e| ExcelError::Write(e.to_string()))?;
    sheet
        .write_string(0, 2, "状态")
        .map_err(|e| ExcelError::Write(e.to_string()))?;
    sheet
        .write_string(0, 3, "错误")
        .map_err(|e| ExcelError::Write(e.to_string()))?;
    for (i, entry) in ledger.entries.iter().enumerate() {
        let row = (i + 1) as u32;
        sheet
            .write_string(row, 0, &entry.old_name)
            .map_err(|e| ExcelError::Write(e.to_string()))?;
        sheet
            .write_string(row, 1, &entry.new_name)
            .map_err(|e| ExcelError::Write(e.to_string()))?;
        let status = match entry.status {
            EntryStatus::Planned => "planned",
            EntryStatus::Success => "success",
            EntryStatus::Failed => "failed",
            EntryStatus::RolledBack => "rolled_back",
        };
        sheet
            .write_string(row, 2, status)
            .map_err(|e| ExcelError::Write(e.to_string()))?;
        if let Some(err) = &entry.error {
            sheet
                .write_string(row, 3, err)
                .map_err(|e| ExcelError::Write(e.to_string()))?;
        }
    }
    workbook
        .save(&xlsx_path)
        .map_err(|e| ExcelError::Write(e.to_string()))?;
    Ok(json_path)
}

pub fn read_ledger(path: &Path) -> Result<Ledger, LedgerError> {
    let data = fs::read(path)?;
    Ok(serde_json::from_slice(&data)?)
}

pub fn find_latest_ledger(data_root: &Path) -> Result<PathBuf, LedgerError> {
    let backup = backup_root(data_root);
    if !backup.is_dir() {
        return Err(LedgerError::NotFound);
    }
    let mut dirs: Vec<PathBuf> = fs::read_dir(&backup)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join(LEDGER_JSON).is_file())
        .collect();
    dirs.sort();
    dirs.pop()
        .map(|d| d.join(LEDGER_JSON))
        .ok_or(LedgerError::NotFound)
}
