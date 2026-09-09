use std::path::PathBuf;

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId,
    Frame, Layout, Margin, RichText, Stroke, Vec2,
};
use rfd::FileDialog;

use batch_file_renamer::ledger::create_backup_dir;
use batch_file_renamer::scan::{format_bytes, FileEntry, ScanResult};
use batch_file_renamer::{
    apply_renames, build_preview, build_restore_preview, export_mapping, import_mapping,
    scan_one_level, Preview, RowAction,
};

pub struct RenamerApp {
    root: Option<PathBuf>,
    scan: Option<ScanResult>,
    excel_path: Option<PathBuf>,
    preview: Option<Preview>,
    confirmed: bool,
    status: String,
    error: String,
    last_export: Option<PathBuf>,
    show_preview: bool,
    restore_preview: Option<Preview>,
    restore_confirmed: bool,
    show_restore_preview: bool,
    completion: Option<String>,
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
            last_export: None,
            show_preview: false,
            restore_preview: None,
            restore_confirmed: false,
            show_restore_preview: false,
            completion: None,
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
                    self.status = "已选择根目录。".into();
                    self.root = Some(path);
                    self.scan = Some(scan);
                    self.preview = None;
                    self.excel_path = None;
                    self.confirmed = false;
                    self.show_preview = false;
                    self.last_export = None;
                    self.restore_preview = None;
                    self.restore_confirmed = false;
                    self.show_restore_preview = false;
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
            Ok(()) => {
                self.last_export = Some(path);
                self.status = "Excel 对照表已导出。".into();
            }
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
                self.status = "已选择 Excel 文件。".into();
                self.excel_path = Some(path);
                self.preview = Some(preview);
                self.confirmed = false;
                self.show_preview = true;
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
            self.error = "请先选择 Excel 文件".into();
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
                self.status = format!(
                    "完成：成功 {}，失败 {}。可用同一份 Excel 把新名还原为旧名。",
                    out.success, out.failed
                );
                self.completion = Some(format!(
                    "修改完成\n成功 {} 个，失败 {} 个。",
                    out.success, out.failed
                ));
                if let Ok(scan) = scan_one_level(&root) {
                    self.scan = Some(scan);
                }
                self.preview = None;
                self.confirmed = false;
                self.show_preview = false;
            }
            Err(e) => self.error = e.to_string(),
        }
    }

    fn load_restore_preview(&mut self, path: PathBuf) {
        self.error.clear();
        let Some(root) = self.root.clone() else {
            self.error = "请先选择根目录".into();
            return;
        };
        let files = match scan_one_level(&root) {
            Ok(scan) => {
                self.scan = Some(scan.clone());
                scan.files
            }
            Err(e) => {
                self.error = e.to_string();
                return;
            }
        };
        let rows = match import_mapping(&path) {
            Ok(rows) => rows,
            Err(e) => {
                self.error = e.to_string();
                return;
            }
        };
        let preview = build_restore_preview(&root, &rows, &files);
        self.excel_path = Some(path);
        self.restore_preview = Some(preview);
        self.restore_confirmed = false;
        self.show_restore_preview = true;
        self.status = "已载入还原预览，请核对后再执行还原。".into();
    }

    fn execute_restore(&mut self) {
        self.error.clear();
        let Some(root) = self.root.clone() else {
            self.error = "请先选择根目录".into();
            return;
        };
        let Some(preview) = self.restore_preview.as_ref() else {
            self.error = "请先选择用于还原的 Excel 对照表".into();
            return;
        };
        if let Some(err) = &preview.blocking_error {
            self.error = err.clone();
            return;
        }
        if !self.restore_confirmed {
            self.error = "请先勾选：我已核对，确认把新文件名改回原来的名字".into();
            return;
        }
        let pairs = preview.planned_renames();
        if pairs.is_empty() {
            self.error = "对照表中没有可还原的改名项（请确认文件夹里现在是新文件名）".into();
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
                self.status = format!(
                    "已还原：成功 {}，失败 {}。文件名已改回原来的名字。",
                    out.success, out.failed
                );
                self.completion = Some(format!(
                    "还原完成\n成功 {} 个，失败 {} 个。",
                    out.success, out.failed
                ));
                if let Ok(scan) = scan_one_level(&root) {
                    self.scan = Some(scan);
                }
                self.restore_preview = None;
                self.restore_confirmed = false;
                self.show_restore_preview = false;
            }
            Err(e) => self.error = e.to_string(),
        }
    }

    fn restore_from_current_excel(&mut self) {
        let Some(path) = self.excel_path.clone() else {
            self.error = "请先在上面「执行改名」里选择那份 Excel".into();
            return;
        };
        self.load_restore_preview(path);
    }

    fn pick_excel_restore(&mut self) {
        self.error.clear();
        if self.root.is_none() {
            self.error = "请先选择根目录".into();
            return;
        }
        let Some(path) = FileDialog::new()
            .set_title("选择改名用的 Excel 对照表")
            .add_filter("Excel", &["xlsx", "xls"])
            .pick_file()
        else {
            return;
        };
        self.load_restore_preview(path);
    }
}

impl eframe::App for RenamerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(32.0)
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(250, 250, 250))
                    .inner_margin(Margin::symmetric(12, 6))
                    .stroke(Stroke::new(1.0_f32, C_LINE)),
            )
            .show(ctx, |ui| {
                if !self.error.is_empty() {
                    ui.label(RichText::new(&self.error).size(13.0).color(C_DANGER));
                } else if !self.status.is_empty() {
                    ui.label(RichText::new(&self.status).size(13.0).color(C_MUTED));
                } else {
                    ui.label(RichText::new("就绪").size(13.0).color(C_MUTED));
                }
            });

        if let Some(message) = self.completion.clone() {
            let mut open = true;
            egui::Window::new(" ")
                .title_bar(false)
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .frame(
                    Frame::new()
                        .fill(Color32::WHITE)
                        .stroke(Stroke::new(2.0_f32, C_PRIMARY))
                        .inner_margin(Margin::symmetric(28, 22))
                        .corner_radius(CornerRadius::same(8)),
                )
                .show(ctx, |ui| {
                    ui.set_min_width(360.0);
                    ui.vertical_centered(|ui| {
                        let title = message.lines().next().unwrap_or("完成");
                        ui.label(
                            RichText::new(title)
                                .size(28.0)
                                .color(C_PRIMARY)
                                .strong(),
                        );
                        ui.add_space(8.0);
                        for line in message.lines().skip(1) {
                            ui.label(RichText::new(line).size(16.0).color(C_TEXT));
                        }
                        ui.add_space(16.0);
                        if primary_button(ui, "确定", true).clicked() {
                            open = false;
                        }
                    });
                });
            if !open {
                self.completion = None;
            }
        }

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(C_BG)
                    .inner_margin(Margin::symmetric(16, 12)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    centered_content(ui, |ui| {
                        ui.label(
                            RichText::new("文件批量重命名")
                                .size(20.0)
                                .strong()
                                .color(C_TEXT),
                        );
                        ui.add_space(8.0);
                        warning_bar(ui);
                        ui.add_space(10.0);

                        panel(ui, |ui| {
                            section_title(ui, "①  准备文件");
                            ui.label(
                                RichText::new("选择根目录后导出对照表。只扫描下一层文件，不进入子文件夹。")
                                    .size(12.5)
                                    .color(C_MUTED),
                            );
                            ui.add_space(8.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                if primary_button(ui, "选择根目录…", true).clicked() {
                                    self.pick_root();
                                }
                                if secondary_button(ui, "导出 Excel", self.scan.is_some()).clicked()
                                {
                                    self.export_excel();
                                }
                            });
                            if let Some(root) = &self.root {
                                ui.add_space(8.0);
                                field_row(ui, "根目录", &root.display().to_string());
                            }
                            if let Some(scan) = &self.scan {
                                field_row(
                                    ui,
                                    "扫描结果",
                                    &format!(
                                        "{} 个文件 · {}",
                                        scan.files.len(),
                                        format_bytes(scan.total_size)
                                    ),
                                );
                            }
                            if let Some(path) = &self.last_export {
                                field_row(ui, "已导出", &path.display().to_string());
                            }
                        });

                        ui.add_space(10.0);

                        panel(ui, |ui| {
                            section_title(ui, "②  执行改名");
                            ui.label(
                                RichText::new("选择已填写「新文件名称」的 Excel（须含完整扩展名）。空单元格将跳过。")
                                    .size(12.5)
                                    .color(C_MUTED),
                            );
                            ui.add_space(8.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                if primary_button(ui, "选择 Excel 文件…", self.root.is_some())
                                    .clicked()
                                {
                                    self.load_excel();
                                }
                                let can_run = self.preview.as_ref().is_some_and(|p| {
                                    p.blocking_error.is_none()
                                        && p.rename_count() > 0
                                        && self.confirmed
                                });
                                if danger_button(ui, "执行批量改名", can_run).clicked() {
                                    self.execute_rename();
                                }
                            });
                            ui.add_space(6.0);
                            ui.checkbox(
                                &mut self.confirmed,
                                RichText::new("我已核对 Excel，确认扩展名与新文件名称无误")
                                    .size(13.0)
                                    .color(C_TEXT),
                            );
                            if let Some(p) = &self.excel_path {
                                ui.add_space(6.0);
                                field_row(
                                    ui,
                                    "Excel",
                                    &p.file_name()
                                        .map(|n| n.to_string_lossy().into_owned())
                                        .unwrap_or_else(|| p.display().to_string()),
                                );
                                field_row(ui, "路径", &p.display().to_string());
                            }
                            if let Some(preview) = &self.preview {
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new("校验结果")
                                        .size(13.0)
                                        .strong()
                                        .color(C_TEXT),
                                );
                                ui.add_space(4.0);
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing.x = 16.0;
                                    stat_chip(ui, "总记录", preview.actions.len());
                                    stat_chip(ui, "将改名", preview.rename_count());
                                    stat_chip(ui, "跳过", preview.skip_count());
                                    stat_chip(ui, "错误", preview.reject_count());
                                });
                                if let Some(err) = &preview.blocking_error {
                                    ui.add_space(4.0);
                                    ui.label(RichText::new(err).size(13.0).color(C_DANGER));
                                }
                                ui.add_space(6.0);
                                let preview_label = if self.show_preview {
                                    "收起改名预览"
                                } else {
                                    "查看改名预览"
                                };
                                if secondary_button(ui, preview_label, true).clicked() {
                                    self.show_preview = !self.show_preview;
                                }
                                if self.show_preview {
                                    ui.add_space(6.0);
                                    draw_preview_list(ui, preview, 360.0);
                                }
                            }
                        });

                        ui.add_space(36.0);
                        ui.separator();
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("还原文件名")
                                .size(16.0)
                                .strong()
                                .color(C_TEXT),
                        );
                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(12.0);

                        panel(ui, |ui| {
                            section_title(ui, "把改过的名字改回去");
                            ui.label(
                                RichText::new("改名之后如果想改回去，请再用刚才那份 Excel。")
                                    .size(14.0)
                                    .color(C_TEXT)
                                    .strong(),
                            );
                            ui.label(
                                RichText::new("程序会把「新文件名称」改回「当前文件名称」（也就是原来的名字）。请先预览、勾选核对，再点执行还原。")
                                    .size(13.0)
                                    .color(C_MUTED),
                            );
                            ui.add_space(8.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                if secondary_button(
                                    ui,
                                    "用当前 Excel 预览还原",
                                    self.root.is_some() && self.excel_path.is_some(),
                                )
                                .clicked()
                                {
                                    self.restore_from_current_excel();
                                }
                                if secondary_button(ui, "选择 Excel 预览还原…", self.root.is_some())
                                    .clicked()
                                {
                                    self.pick_excel_restore();
                                }
                                let can_restore = self.restore_preview.as_ref().is_some_and(|p| {
                                    p.blocking_error.is_none()
                                        && p.rename_count() > 0
                                        && self.restore_confirmed
                                });
                                if danger_button(ui, "执行还原", can_restore).clicked() {
                                    self.execute_restore();
                                }
                            });
                            ui.add_space(6.0);
                            ui.checkbox(
                                &mut self.restore_confirmed,
                                RichText::new("我已核对预览，确认把新文件名改回原来的名字")
                                    .size(13.0)
                                    .color(C_TEXT),
                            );
                            if let Some(p) = &self.excel_path {
                                ui.add_space(8.0);
                                field_row(ui, "对照表", &p.display().to_string());
                            }
                            if let Some(preview) = &self.restore_preview {
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new("还原校验")
                                        .size(13.0)
                                        .strong()
                                        .color(C_TEXT),
                                );
                                ui.add_space(4.0);
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing.x = 16.0;
                                    stat_chip(ui, "总记录", preview.actions.len());
                                    stat_chip(ui, "将还原", preview.rename_count());
                                    stat_chip(ui, "跳过", preview.skip_count());
                                    stat_chip(ui, "错误", preview.reject_count());
                                });
                                if let Some(err) = &preview.blocking_error {
                                    ui.add_space(4.0);
                                    ui.label(RichText::new(err).size(13.0).color(C_DANGER));
                                }
                                ui.add_space(6.0);
                                let preview_label = if self.show_restore_preview {
                                    "收起还原预览"
                                } else {
                                    "查看还原预览"
                                };
                                if secondary_button(ui, preview_label, true).clicked() {
                                    self.show_restore_preview = !self.show_restore_preview;
                                }
                                if self.show_restore_preview {
                                    ui.add_space(6.0);
                                    draw_preview_list(ui, preview, 360.0);
                                }
                            }
                        });
                    });
                });
            });
    }
}

const CONTENT_W: f32 = 760.0;
const C_BG: Color32 = Color32::from_rgb(245, 245, 245);
const C_TEXT: Color32 = Color32::from_rgb(32, 32, 32);
const C_MUTED: Color32 = Color32::from_rgb(96, 96, 96);
const C_LINE: Color32 = Color32::from_rgb(220, 220, 220);
const C_PRIMARY: Color32 = Color32::from_rgb(37, 99, 235);
const C_DANGER: Color32 = Color32::from_rgb(185, 28, 28);

fn apply_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.button_padding = Vec2::new(10.0, 5.0);
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.visuals.panel_fill = C_BG;
    style.visuals.window_fill = C_BG;
    style.visuals.override_text_color = Some(C_TEXT);
    style.text_styles.insert(
        egui::TextStyle::Heading,
        FontId::new(15.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Body,
        FontId::new(13.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        FontId::new(13.0, FontFamily::Proportional),
    );
    ctx.set_style(style);
}

fn centered_content(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let width = ui.available_width();
    let content = width.min(CONTENT_W);
    let pad = ((width - content) / 2.0).max(0.0);
    ui.horizontal(|ui| {
        ui.add_space(pad);
        ui.allocate_ui_with_layout(
            Vec2::new(content, ui.available_height()),
            Layout::top_down(Align::Min).with_cross_justify(true),
            add,
        );
    });
}

fn panel(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    Frame::new()
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0_f32, C_LINE))
        .inner_margin(Margin::symmetric(12, 10))
        .corner_radius(CornerRadius::same(4))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add(ui);
        });
}

fn section_title(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).size(15.0).strong().color(C_TEXT));
    ui.add_space(2.0);
}

fn primary_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).size(13.0).color(Color32::WHITE))
            .fill(C_PRIMARY)
            .min_size(Vec2::new(108.0, 30.0))
            .corner_radius(CornerRadius::same(4)),
    )
}

fn secondary_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).size(13.0).color(C_TEXT))
            .fill(Color32::WHITE)
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(180, 180, 180)))
            .min_size(Vec2::new(108.0, 30.0))
            .corner_radius(CornerRadius::same(4)),
    )
}

fn danger_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).size(13.0).color(Color32::WHITE))
            .fill(C_DANGER)
            .min_size(Vec2::new(108.0, 30.0))
            .corner_radius(CornerRadius::same(4)),
    )
}

fn warning_bar(ui: &mut egui::Ui) {
    Frame::new()
        .fill(Color32::from_rgb(185, 28, 28))
        .stroke(Stroke::new(2.0_f32, Color32::from_rgb(127, 29, 29)))
        .inner_margin(Margin::symmetric(12, 10))
        .corner_radius(CornerRadius::same(4))
        .show(ui, |ui| {
            ui.label(
                RichText::new("修改文件名需谨慎，请提前做好数据备份！")
                    .size(18.0)
                    .color(Color32::WHITE)
                    .strong(),
            );
            ui.label(
                RichText::new("本工具只改文件名，不复制文件内容。测序大文件请先核对 Excel 再执行。")
                    .size(14.0)
                    .color(Color32::from_rgb(254, 226, 226)),
            );
        });
}

fn draw_preview_list(ui: &mut egui::Ui, preview: &Preview, max_height: f32) {
    Frame::new()
        .fill(Color32::from_rgb(250, 250, 250))
        .stroke(Stroke::new(1.0_f32, C_LINE))
        .inner_margin(Margin::same(8))
        .corner_radius(CornerRadius::same(4))
        .show(ui, |ui| {
            ui.set_min_height((max_height * 0.7).min(280.0));
            egui::ScrollArea::vertical()
                .max_height(max_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for action in &preview.actions {
                        match action {
                            RowAction::Rename { old_name, new_name } => {
                                ui.label(
                                    RichText::new(format!("{old_name}  →  {new_name}"))
                                        .size(13.0)
                                        .color(C_TEXT),
                                );
                            }
                            RowAction::Skip { old_name, reason } => {
                                ui.label(
                                    RichText::new(format!("跳过 {old_name}：{reason}"))
                                        .size(12.5)
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
                                        "错误 {old_name} → {new_name}：{reason}"
                                    ))
                                    .size(13.0)
                                    .color(C_DANGER),
                                );
                            }
                        }
                    }
                });
        });
}

fn field_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("{label}：")).size(12.5).color(C_MUTED));
        ui.label(RichText::new(value).size(12.5).color(C_TEXT));
    });
}

fn stat_chip(ui: &mut egui::Ui, label: &str, value: usize) {
    ui.label(
        RichText::new(format!("{label} {value}"))
            .size(13.0)
            .color(C_TEXT),
    );
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
            .with_inner_size(Vec2::new(880.0, 780.0))
            .with_min_inner_size(Vec2::new(680.0, 560.0))
            .with_title("文件批量重命名"),
        ..Default::default()
    };
    eframe::run_native(
        "文件批量重命名",
        options,
        Box::new(|cc| Ok(Box::new(RenamerApp::new(cc)))),
    )
}
