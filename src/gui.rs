use std::path::PathBuf;

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, RichText, Vec2};
use rfd::FileDialog;

use batch_file_renamer::ledger::create_backup_dir;
use batch_file_renamer::scan::{format_bytes, FileEntry, ScanResult};
use batch_file_renamer::{
    apply_renames, build_preview, export_mapping, find_latest_ledger, import_mapping, read_ledger,
    rollback_ledger, scan_one_level, Preview, RowAction,
};

pub struct RenamerApp {
    root: Option<PathBuf>,
    scan: Option<ScanResult>,
    excel_path: Option<PathBuf>,
    preview: Option<Preview>,
    confirmed: bool,
    status: String,
    error: String,
    last_ledger: Option<PathBuf>,
}

impl Default for RenamerApp {
    fn default() -> Self {
        Self {
            root: None,
            scan: None,
            excel_path: None,
            preview: None,
            confirmed: false,
            status: String::new(),
            error: String::new(),
            last_ledger: None,
        }
    }
}

impl RenamerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_cjk_fonts(&cc.egui_ctx);
        let mut style = (*cc.egui_ctx.style()).clone();
        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::new(28.0, FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::new(16.0, FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::new(16.0, FontFamily::Proportional),
        );
        cc.egui_ctx.set_style(style);
        Self::default()
    }

    fn pick_root(&mut self) {
        self.error.clear();
        if let Some(path) = FileDialog::new().set_title("选择根目录").pick_folder() {
            match scan_one_level(&path) {
                Ok(scan) => {
                    self.status = format!(
                        "已扫描 {} 个文件，合计 {}",
                        scan.files.len(),
                        format_bytes(scan.total_size)
                    );
                    self.root = Some(path);
                    self.scan = Some(scan);
                    self.preview = None;
                    self.excel_path = None;
                    self.confirmed = false;
                    if let Ok(ledger) = find_latest_ledger(self.root.as_ref().unwrap()) {
                        self.last_ledger = Some(ledger);
                    }
                }
                Err(e) => self.error = e.to_string(),
            }
        }
    }

    fn export_excel(&mut self) {
        self.error.clear();
        let Some(scan) = self.scan.as_ref() else {
            self.error = "请先选择根目录".into();
            return;
        };
        let Some(path) = FileDialog::new()
            .set_title("保存文件对照表")
            .add_filter("Excel", &["xlsx"])
            .set_file_name("文件对照表.xlsx")
            .save_file()
        else {
            return;
        };
        match export_mapping(&path, &scan.files) {
            Ok(()) => self.status = format!("已导出: {}", path.display()),
            Err(e) => self.error = e.to_string(),
        }
    }

    fn load_excel(&mut self) {
        self.error.clear();
        let Some(root) = self.root.clone() else {
            self.error = "请先选择根目录".into();
            return;
        };
        let Some(path) = FileDialog::new()
            .set_title("选择已填写新文件名称的 Excel")
            .add_filter("Excel", &["xlsx", "xls"])
            .pick_file()
        else {
            return;
        };
        match import_mapping(&path) {
            Ok(rows) => {
                let files: Vec<FileEntry> = self
                    .scan
                    .as_ref()
                    .map(|s| s.files.clone())
                    .unwrap_or_default();
                let preview = build_preview(&root, &rows, &files);
                self.status = format!(
                    "预览：将改名 {}，跳过 {}，拒绝 {}。新文件名称必须含完整扩展名（如 .fastq.gz）",
                    preview.rename_count(),
                    preview.skip_count(),
                    preview.reject_count()
                );
                self.excel_path = Some(path);
                self.preview = Some(preview);
                self.confirmed = false;
            }
            Err(e) => self.error = e.to_string(),
        }
    }

    fn execute_rename(&mut self) {
        self.error.clear();
        let Some(root) = self.root.clone() else {
            self.error = "请先选择根目录".into();
            return;
        };
        let Some(preview) = self.preview.as_ref() else {
            self.error = "请先上传 Excel 并预览".into();
            return;
        };
        if let Some(err) = &preview.blocking_error {
            self.error = err.clone();
            return;
        }
        if !self.confirmed {
            self.error = "请先勾选：我已核对 Excel，确认扩展名与新文件名称无误".into();
            return;
        }
        let pairs = preview.planned_renames();
        if pairs.is_empty() {
            self.error = "没有可执行的改名项".into();
            return;
        }
        let backup_dir = match create_backup_dir(&root) {
            Ok(d) => d,
            Err(e) => {
                self.error = e.to_string();
                return;
            }
        };
        match apply_renames(&root, &pairs, &backup_dir) {
            Ok(out) => {
                self.last_ledger = Some(out.ledger_path.clone());
                self.status = format!(
                    "完成：成功 {}，失败 {}。账本（可回滚）: {}",
                    out.success,
                    out.failed,
                    out.ledger_path.display()
                );
                if let Ok(scan) = scan_one_level(&root) {
                    self.scan = Some(scan);
                }
                self.preview = None;
                self.confirmed = false;
            }
            Err(e) => self.error = e.to_string(),
        }
    }

    fn rollback(&mut self) {
        self.error.clear();
        let Some(root) = self.root.clone() else {
            self.error = "请先选择根目录".into();
            return;
        };
        let ledger_path = if let Some(p) = self.last_ledger.clone() {
            p
        } else {
            match find_latest_ledger(&root) {
                Ok(p) => p,
                Err(e) => {
                    self.error = e.to_string();
                    return;
                }
            }
        };
        match read_ledger(&ledger_path) {
            Ok(ledger) => match rollback_ledger(&root, &ledger) {
                Ok(out) => {
                    self.status = format!(
                        "回滚完成：成功 {}，失败 {}。回滚账本: {}",
                        out.success,
                        out.failed,
                        out.ledger_path.display()
                    );
                    if let Ok(scan) = scan_one_level(&root) {
                        self.scan = Some(scan);
                    }
                }
                Err(e) => self.error = e.to_string(),
            },
            Err(e) => self.error = e.to_string(),
        }
    }

    fn pick_ledger_rollback(&mut self) {
        if let Some(path) = FileDialog::new()
            .set_title("选择 rename_map.json 账本")
            .add_filter("JSON", &["json"])
            .pick_file()
        {
            self.last_ledger = Some(path);
            self.rollback();
        }
    }
}

impl eframe::App for RenamerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(8.0);
                ui.label(
                    RichText::new("修改文件名需谨慎！请提前做好数据备份。")
                        .color(Color32::from_rgb(180, 0, 0))
                        .size(32.0)
                        .strong(),
                );
                ui.label(
                    RichText::new(
                        "本工具只改文件名、不复制文件内容。测序数据体积大，不会做文件拷贝备份。请先核对 Excel（新名称必须含完整扩展名，如 .fastq.gz / .bam），并保留导出的原始对照表。误操作可用改名账本回滚文件名。",
                    )
                    .color(Color32::from_rgb(140, 20, 20))
                    .size(18.0)
                    .strong(),
                );
                ui.add_space(12.0);
                ui.separator();

                ui.heading("1. 选择根目录并导出对照表");
                ui.label("只扫描该目录下一层文件，不进入子文件夹，不改文件夹名。");
                ui.horizontal(|ui| {
                    if ui.button("选择根目录…").clicked() {
                        self.pick_root();
                    }
                    if ui
                        .add_enabled(self.scan.is_some(), egui::Button::new("导出 Excel 对照表…"))
                        .clicked()
                    {
                        self.export_excel();
                    }
                });
                if let Some(root) = &self.root {
                    ui.label(format!("根目录: {}", root.display()));
                }
                if let Some(scan) = &self.scan {
                    ui.label(format!(
                        "文件数: {}    合计大小: {}",
                        scan.files.len(),
                        format_bytes(scan.total_size)
                    ));
                }

                ui.add_space(10.0);
                ui.separator();
                ui.heading("2. 上传已填写的 Excel，批量改名");
                ui.label("请先在 Excel「新文件名称」列填完整文件名，再上传。空单元格将跳过。");
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(self.root.is_some(), egui::Button::new("上传 Excel…"))
                        .clicked()
                    {
                        self.load_excel();
                    }
                    ui.checkbox(
                        &mut self.confirmed,
                        "我已核对 Excel，确认扩展名与新文件名称无误",
                    );
                    let can_run = self.preview.as_ref().is_some_and(|p| {
                        p.blocking_error.is_none() && p.rename_count() > 0 && self.confirmed
                    });
                    if ui
                        .add_enabled(can_run, egui::Button::new("执行批量改名"))
                        .clicked()
                    {
                        self.execute_rename();
                    }
                });
                if let Some(p) = &self.excel_path {
                    ui.label(format!("Excel: {}", p.display()));
                }
                if let Some(preview) = &self.preview {
                    if let Some(err) = &preview.blocking_error {
                        ui.label(RichText::new(err).color(Color32::RED).size(18.0).strong());
                    }
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.set_min_height(180.0);
                        egui::ScrollArea::vertical()
                            .max_height(240.0)
                            .show(ui, |ui| {
                                for action in &preview.actions {
                                    match action {
                                        RowAction::Rename { old_name, new_name } => {
                                            ui.label(
                                                RichText::new(format!("{old_name}  →  {new_name}"))
                                                    .color(Color32::from_rgb(0, 90, 40)),
                                            );
                                        }
                                        RowAction::Skip { old_name, reason } => {
                                            ui.label(format!("跳过 {old_name}：{reason}"));
                                        }
                                        RowAction::Reject {
                                            old_name,
                                            new_name,
                                            reason,
                                        } => {
                                            ui.label(
                                                RichText::new(format!(
                                                    "拒绝 {old_name} → {new_name}：{reason}"
                                                ))
                                                .color(Color32::from_rgb(160, 40, 0)),
                                            );
                                        }
                                    }
                                }
                            });
                    });
                }

                ui.add_space(10.0);
                ui.separator();
                ui.heading("3. 按账本回滚文件名");
                ui.label("账本只记录旧名/新名，不复制测序数据。回滚会把已成功改名的文件改回原名。");
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(self.root.is_some(), egui::Button::new("回滚最近一次改名"))
                        .clicked()
                    {
                        self.rollback();
                    }
                    if ui.button("选择账本 JSON 回滚…").clicked() {
                        self.pick_ledger_rollback();
                    }
                });
                if let Some(p) = &self.last_ledger {
                    ui.label(format!("当前账本: {}", p.display()));
                }

                ui.add_space(12.0);
                if !self.status.is_empty() {
                    ui.label(RichText::new(&self.status).color(Color32::from_rgb(20, 70, 130)));
                }
                if !self.error.is_empty() {
                    ui.label(RichText::new(&self.error).color(Color32::RED).strong());
                }
            });
        });
    }
}

fn setup_cjk_fonts(ctx: &egui::Context) {
    let candidates = [
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyh.ttf",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
        r"C:\Windows\Fonts\msyhbd.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    ];
    let mut found: Option<(String, Vec<u8>)> = None;
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            found = Some((path.to_string(), bytes));
            break;
        }
    }
    let Some((path, bytes)) = found else {
        return;
    };
    let mut fonts = FontDefinitions::default();
    let mut data = FontData::from_owned(bytes);
    if path.ends_with(".ttc") {
        data.index = 0;
    }
    fonts.font_data.insert("cjk".to_owned(), data.into());
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "cjk".to_owned());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .push("cjk".to_owned());
    ctx.set_fonts(fonts);
}

pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(Vec2::new(980.0, 780.0))
            .with_min_inner_size(Vec2::new(720.0, 560.0))
            .with_title("一层文件批量重命名"),
        ..Default::default()
    };
    eframe::run_native(
        "一层文件批量重命名",
        options,
        Box::new(|cc| Ok(Box::new(RenamerApp::new(cc)))),
    )
}
