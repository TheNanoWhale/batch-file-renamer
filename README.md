# 一层文件批量重命名

面向测序数据等**超大文件**目录：只改文件名，**不复制文件内容**。用 Excel 对照表批量重命名，并用改名账本回滚。

## 请先阅读

**修改文件名需谨慎！请提前做好数据备份。**

本工具不会拷贝测序数据（单文件可达数十 GB，目录可达数 TB）。真正的保险是：

1. 改名前核对 Excel，**新文件名称必须包含完整扩展名**（如 `.fastq.gz`、`.bam`）。
2. 保留导出的原始对照表。
3. 每次执行都会在数据目录下写入 `rename_backup/时间戳/rename_map.json`，可用它把文件名改回去。

## 功能范围

- 只处理所选根目录的**下一层文件**，不进入子目录，不改文件夹名。
- Excel 两列：`当前文件名称`、`新文件名称`。
- 新文件名为空或与旧名相同 → 跳过。
- 目标名已存在且该文件不在本次改名集合中 → 拒绝覆盖。
- 新文件名重复 → 整批拒绝。
- 交换文件名（A↔B）使用两阶段临时名，避免互相覆盖。

## 使用步骤

1. 打开软件，顶部红色警告请务必阅读。
2. 选择根目录，导出 Excel 对照表。
3. 在「新文件名称」列填写完整新文件名。
4. 回到软件上传 Excel，核对预览。
5. 勾选「我已核对 Excel」后执行批量改名。
6. 若改错，使用「回滚最近一次改名」或选择对应 `rename_map.json`。

## 免安装 Windows 版

由 GitHub Actions 在 `windows-latest` 上编译，产物为单个 `batch-file-renamer.exe`，拷贝后双击即可，无需安装。

- 正式包：[Releases](https://github.com/TheNanoWhale/batch-file-renamer/releases)
- 每次推送 `main` 也会留下 [Actions Artifact](https://github.com/TheNanoWhale/batch-file-renamer/actions)
- 打标签 `v*`（例如 `v0.1.0`）会自动发布 Release 并附带 exe
- 也可在 Actions 里手动运行工作流 `Windows exe`

中文界面依赖 Windows 系统字体（微软雅黑 / 黑体 / 宋体）。

## 从源码编译

```bash
cargo test
cargo build --release
```

## 许可

MIT
