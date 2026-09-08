use std::path::PathBuf;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame,
    Margin, RichText, Sense, Stroke, Vec2,
};
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
        apply_theme(&cc.egui_ctx);
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
        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(56.0)
            .frame(
                Frame::new()
                    .fill(if self.error.is_empty() {
                        Color32::from_rgb(239, 246, 255)
                    } else {
                        Color32::from_rgb(254, 226, 226)
                    })
                    .inner_margin(Margin::symmetric(16, 10))
                    .stroke(Stroke::new(
                        1.0_f32,
                        if self.error.is_empty() {
                            Color32::from_rgb(191, 219, 254)
                        } else {
                            Color32::from_rgb(252, 165, 165)
                        },
                    )),
            )
            .show(ctx, |ui| {
                if !self.error.is_empty() {
                    ui.label(
                        RichText::new(&self.error)
                            .size(17.0)
                            .color(C_DANGER)
                            .strong(),
                    );
                } else if !self.status.is_empty() {
                    ui.label(
                        RichText::new(&self.status)
                            .size(17.0)
                            .color(C_PRIMARY_DARK)
                            .strong(),
                    );
                } else {
                    ui.label(
                        RichText::new("准备就绪：请先选择根目录。")
                            .size(17.0)
                            .color(C_MUTED),
                    );
                }
            });

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(C_BG)
                    .inner_margin(Margin::symmetric(20, 16)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    warning_banner(ui);
                    ui.add_space(16.0);

                    section_card(ui, 1, "选择根目录并导出对照表", "只扫描该目录下一层文件，不进入子文件夹，不改文件夹名。", |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 12.0;
                            if filled_button(ui, "选择根目录…", C_PRIMARY, true).clicked() {
                                self.pick_root();
                            }
                            if filled_button(ui, "导出 Excel 对照表…", C_SUCCESS, self.scan.is_some())
                                .clicked()
                            {
                                self.export_excel();
                            }
                        });
                        ui.add_space(10.0);
                        if let Some(root) = &self.root {
                            info_line(ui, "根目录", &root.display().to_string());
                        }
                        if let Some(scan) = &self.scan {
                            info_line(
                                ui,
                                "扫描结果",
                                &format!(
                                    "{} 个文件 · {}",
                                    scan.files.len(),
                                    format_bytes(scan.total_size)
                                ),
                            );
                        }
                    });

                    ui.add_space(14.0);

                    section_card(ui, 2, "上传已填写的 Excel，批量改名", "请先在 Excel「新文件名称」列填写完整文件名（含扩展名），再上传。空单元格将跳过。", |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 12.0;
                            if filled_button(ui, "上传 Excel…", C_PRIMARY, self.root.is_some())
                                .clicked()
                            {
                                self.load_excel();
                            }
                            let can_run = self.preview.as_ref().is_some_and(|p| {
                                p.blocking_error.is_none() && p.rename_count() > 0 && self.confirmed
                            });
                            if filled_button(ui, "执行批量改名", C_DANGER, can_run).clicked() {
                                self.execute_rename();
                            }
                        });
                        ui.add_space(10.0);
                        ui.checkbox(
                            &mut self.confirmed,
                            RichText::new("我已核对 Excel，确认扩展名与新文件名称无误")
                                .size(17.0)
                                .color(C_TEXT)
                                .strong(),
                        );
                        if self.preview.is_some() && !self.confirmed {
                            ui.label(
                                RichText::new("勾选上方核对项后，「执行批量改名」才会亮起。")
                                    .size(15.0)
                                    .color(C_WARN),
                            );
                        }
                        if let Some(p) = &self.excel_path {
                            ui.add_space(6.0);
                            info_line(ui, "Excel", &p.display().to_string());
                        }
                        if let Some(preview) = &self.preview {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(format!(
                                    "将改名 {}  ·  跳过 {}  ·  拒绝 {}",
                                    preview.rename_count(),
                                    preview.skip_count(),
                                    preview.reject_count()
                                ))
                                .size(16.0)
                                .color(C_PRIMARY_DARK)
                                .strong(),
                            );
                            if let Some(err) = &preview.blocking_error {
                                ui.label(
                                    RichText::new(err).color(C_DANGER).size(17.0).strong(),
                                );
                            }
                            Frame::new()
                                .fill(Color32::from_rgb(248, 250, 252))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(226, 232, 240)))
                                .inner_margin(Margin::same(10))
                                .corner_radius(CornerRadius::same(8))
                                .show(ui, |ui| {
                                    ui.set_min_height(160.0);
                                    egui::ScrollArea::vertical()
                                        .max_height(220.0)
                                        .show(ui, |ui| {
                                            for action in &preview.actions {
                                                match action {
                                                    RowAction::Rename { old_name, new_name } => {
                                                        ui.label(
                                                            RichText::new(format!(
                                                                "{old_name}  →  {new_name}"
                                                            ))
                                                            .size(16.0)
                                                            .color(C_SUCCESS_DARK),
                                                        );
                                                    }
                                                    RowAction::Skip { old_name, reason } => {
                                                        ui.label(
                                                            RichText::new(format!(
                                                                "跳过 {old_name}：{reason}"
                                                            ))
                                                            .size(15.0)
                                                            .color(C_MUTED),
                                                        );
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
                                                            .size(16.0)
                                                            .color(C_DANGER),
                                                        );
                                                    }
                                                }
                                            }
                                        });
                                });
                        }
                    });

                    ui.add_space(14.0);

                    section_card(ui, 3, "按账本回滚文件名", "账本只记录旧名/新名，不复制测序数据。回滚会把已成功改名的文件改回原名。", |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 12.0;
                            if outline_button(
                                ui,
                                "回滚最近一次改名",
                                C_WARN,
                                self.root.is_some(),
                            )
                            .clicked()
                            {
                                self.rollback();
                            }
                            if outline_button(ui, "选择账本 JSON 回滚…", C_MUTED, true).clicked()
                            {
                                self.pick_ledger_rollback();
                            }
                        });
                        if let Some(p) = &self.last_ledger {
                            ui.add_space(10.0);
                            info_line(ui, "当前账本", &p.display().to_string());
                        }
                    });

                    ui.add_space(12.0);
                });
            });
    }
}

const C_BG: Color32 = Color32::from_rgb(241, 245, 249);
const C_TEXT: Color32 = Color32::from_rgb(15, 23, 42);
const C_MUTED: Color32 = Color32::from_rgb(71, 85, 105);
const C_PRIMARY: Color32 = Color32::from_rgb(37, 99, 235);
const C_PRIMARY_DARK: Color32 = Color32::from_rgb(29, 78, 216);
const C_SUCCESS: Color32 = Color32::from_rgb(22, 163, 74);
const C_SUCCESS_DARK: Color32 = Color32::from_rgb(21, 128, 61);
const C_DANGER: Color32 = Color32::from_rgb(220, 38, 38);
const C_WARN: Color32 = Color32::from_rgb(217, 119, 6);

fn apply_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.button_padding = Vec2::new(18.0, 12.0);
    style.spacing.item_spacing = Vec2::new(12.0, 10.0);
    style.spacing.indent = 18.0;
    style.visuals.panel_fill = C_BG;
    style.visuals.window_fill = C_BG;
    style.visuals.override_text_color = Some(C_TEXT);
    style.text_styles.insert(
        egui::TextStyle::Heading,
        FontId::new(22.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Body,
        FontId::new(16.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        FontId::new(18.0, FontFamily::Proportional),
    );
    ctx.set_style(style);
}

fn filled_button(ui: &mut egui::Ui, text: &str, fill: Color32, enabled: bool) -> egui::Response {
    let label = RichText::new(text)
        .size(18.0)
        .color(Color32::WHITE)
        .strong();
    ui.add_enabled(
        enabled,
        egui::Button::new(label)
            .fill(fill)
            .min_size(Vec2::new(200.0, 48.0))
            .corner_radius(CornerRadius::same(8)),
    )
}

fn outline_button(ui: &mut egui::Ui, text: &str, color: Color32, enabled: bool) -> egui::Response {
    let label = RichText::new(text).size(18.0).color(color).strong();
    ui.add_enabled(
        enabled,
        egui::Button::new(label)
            .fill(Color32::WHITE)
            .stroke(Stroke::new(2.0_f32, color))
            .min_size(Vec2::new(200.0, 48.0))
            .corner_radius(CornerRadius::same(8)),
    )
}

fn warning_banner(ui: &mut egui::Ui) {
    Frame::new()
        .fill(Color32::from_rgb(254, 226, 226))
        .stroke(Stroke::new(2.0_f32, Color32::from_rgb(220, 38, 38)))
        .inner_margin(Margin::symmetric(18, 16))
        .corner_radius(CornerRadius::same(10))
        .show(ui, |ui| {
            ui.label(
                RichText::new("修改文件名需谨慎！请提前做好数据备份。")
                    .color(Color32::from_rgb(153, 27, 27))
                    .size(28.0)
                    .strong(),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "本工具只改文件名、不复制文件内容。测序数据体积大，不会做文件拷贝备份。请先核对 Excel（新名称必须含完整扩展名，如 .fastq.gz / .bam），并保留导出的原始对照表。误操作可用改名账本回滚文件名。",
                )
                .color(Color32::from_rgb(127, 29, 29))
                .size(16.0),
            );
        });
}

fn section_card(
    ui: &mut egui::Ui,
    step: u8,
    title: &str,
    hint: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    Frame::new()
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(203, 213, 225)))
        .inner_margin(Margin::symmetric(18, 16))
        .corner_radius(CornerRadius::same(10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(34.0, 34.0), Sense::hover());
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(8), C_PRIMARY);
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    step.to_string(),
                    FontId::proportional(18.0),
                    Color32::WHITE,
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(title)
                        .size(22.0)
                        .strong()
                        .color(C_TEXT),
                );
            });
            ui.add_space(6.0);
            ui.label(RichText::new(hint).size(15.0).color(C_MUTED));
            ui.add_space(12.0);
            add_contents(ui);
        });
}

fn info_line(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(format!("{label}："))
                .size(16.0)
                .color(C_MUTED)
                .strong(),
        );
        ui.label(RichText::new(value).size(16.0).color(C_PRIMARY_DARK));
    });
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
            .with_inner_size(Vec2::new(1080.0, 820.0))
            .with_min_inner_size(Vec2::new(860.0, 640.0))
            .with_title("一层文件批量重命名"),
        ..Default::default()
    };
    eframe::run_native(
        "一层文件批量重命名",
        options,
        Box::new(|cc| Ok(Box::new(RenamerApp::new(cc)))),
    )
}
