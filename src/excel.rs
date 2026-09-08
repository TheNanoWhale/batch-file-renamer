use std::path::Path;

use calamine::{open_workbook_auto, Data, Reader};
use rust_xlsxwriter::{Format, Workbook};

use crate::scan::FileEntry;
use crate::validate::{COL_NEW, COL_OLD};

#[derive(Debug, thiserror::Error)]
pub enum ExcelError {
    #[error("写入 Excel 失败: {0}")]
    Write(String),
    #[error("读取 Excel 失败: {0}")]
    Read(String),
    #[error("找不到表头列「{COL_OLD}」和「{COL_NEW}」，请不要修改列名")]
    MissingHeaders,
}

pub fn export_mapping(path: &Path, files: &[FileEntry]) -> Result<(), ExcelError> {
    let mut workbook = Workbook::new();

    let header_fmt = Format::new().set_bold();
    let note_fmt = Format::new().set_text_wrap().set_font_color(0x9B1B30);

    {
        let sheet = workbook
            .add_worksheet()
            .set_name("使用说明")
            .map_err(|e| ExcelError::Write(e.to_string()))?;
        sheet
            .set_column_width(0, 80)
            .map_err(|e| ExcelError::Write(e.to_string()))?;
        let notes = [
            "修改文件名需谨慎！请提前做好数据备份。",
            "本工具只改文件名、不复制文件内容。测序大文件请务必先核对再执行。",
            "请在「文件列表」工作表填写「新文件名称」。必须包含完整扩展名，例如 .fastq.gz / .bam。",
            "新文件名称为空或与当前名称相同的行会被跳过。",
            "不要修改表头列名，不要填写路径，只能填写文件名。",
            "请勿使用非法字符: \\ / : * ? \" < > |",
        ];
        for (i, line) in notes.iter().enumerate() {
            sheet
                .write_string_with_format(i as u32, 0, *line, &note_fmt)
                .map_err(|e| ExcelError::Write(e.to_string()))?;
        }
    }

    {
        let sheet = workbook
            .add_worksheet()
            .set_name("文件列表")
            .map_err(|e| ExcelError::Write(e.to_string()))?;
        sheet
            .set_column_width(0, 48)
            .map_err(|e| ExcelError::Write(e.to_string()))?;
        sheet
            .set_column_width(1, 48)
            .map_err(|e| ExcelError::Write(e.to_string()))?;
        sheet
            .write_string_with_format(0, 0, COL_OLD, &header_fmt)
            .map_err(|e| ExcelError::Write(e.to_string()))?;
        sheet
            .write_string_with_format(0, 1, COL_NEW, &header_fmt)
            .map_err(|e| ExcelError::Write(e.to_string()))?;
        for (i, file) in files.iter().enumerate() {
            let row = (i + 1) as u32;
            sheet
                .write_string(row, 0, &file.name)
                .map_err(|e| ExcelError::Write(e.to_string()))?;
            sheet
                .write_string(row, 1, "")
                .map_err(|e| ExcelError::Write(e.to_string()))?;
        }
    }

    workbook
        .save(path)
        .map_err(|e| ExcelError::Write(e.to_string()))?;
    Ok(())
}

pub fn import_mapping(path: &Path) -> Result<Vec<(String, String)>, ExcelError> {
    let mut workbook = open_workbook_auto(path).map_err(|e| ExcelError::Read(e.to_string()))?;
    let sheet_names = workbook.sheet_names();
    let preferred = if sheet_names.iter().any(|n| n == "文件列表") {
        "文件列表".to_string()
    } else {
        sheet_names
            .first()
            .cloned()
            .ok_or_else(|| ExcelError::Read("工作簿没有工作表".into()))?
    };

    let range = workbook
        .worksheet_range(&preferred)
        .map_err(|e| ExcelError::Read(e.to_string()))?;

    let mut old_col: Option<usize> = None;
    let mut new_col: Option<usize> = None;
    let mut header_row: Option<u32> = None;

    for (r, row) in range.rows().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            let text = cell_text(cell);
            if text == COL_OLD {
                old_col = Some(c);
                header_row = Some(r as u32);
            }
            if text == COL_NEW {
                new_col = Some(c);
                header_row = Some(r as u32);
            }
        }
        if old_col.is_some() && new_col.is_some() {
            break;
        }
    }

    let (old_col, new_col, header_row) = match (old_col, new_col, header_row) {
        (Some(o), Some(n), Some(h)) => (o, n, h),
        _ => return Err(ExcelError::MissingHeaders),
    };

    let mut rows = Vec::new();
    for (r, row) in range.rows().enumerate() {
        if (r as u32) <= header_row {
            continue;
        }
        let old = row.get(old_col).map(cell_text).unwrap_or_default();
        let new = row.get(new_col).map(cell_text).unwrap_or_default();
        if old.trim().is_empty() && new.trim().is_empty() {
            continue;
        }
        rows.push((old, new));
    }
    Ok(rows)
}

fn cell_text(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Float(f) => {
            if f.fract() == 0.0 {
                format!("{}", *f as i64)
            } else {
                f.to_string()
            }
        }
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(dt) => dt.to_string(),
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Error(e) => e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn export_then_import_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("map.xlsx");
        let files = vec![
            FileEntry {
                name: "sample_R1.fastq.gz".into(),
                size: 10,
            },
            FileEntry {
                name: "sample_R2.fastq.gz".into(),
                size: 10,
            },
        ];
        export_mapping(&path, &files).unwrap();
        let rows = import_mapping(&path).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "sample_R1.fastq.gz");
        assert_eq!(rows[0].1, "");
    }
}
