<div align="center">

<img src="src-tauri/icons/icon.png" width="128" height="128" alt="Mini Wallpaper Logo" />

# Mini Wallpaper

🖼️ 轻量级 Windows 桌面壁纸管理工具

[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Tauri](https://img.shields.io/badge/Tauri-2.0-FFC131?logo=tauri&logoColor=white)](https://v2.tauri.app/)
[![React](https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=white)](https://react.dev/)
[![TypeScript](https://img.shields.io/badge/TypeScript-5.8-3178C6?logo=typescript&logoColor=white)](https://www.typescriptlang.org/)
[![Rust](https://img.shields.io/badge/Rust-2021-DEA584?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Vite](https://img.shields.io/badge/Vite-8-646CFF?logo=vite&logoColor=white)](https://vite.dev/)
[![TailwindCSS](https://img.shields.io/badge/Tailwind_CSS-4-06B6D4?logo=tailwindcss&logoColor=white)](https://tailwindcss.com/)
[![SQLite](https://img.shields.io/badge/SQLite-SeaORM-003B57?logo=sqlite&logoColor=white)](https://www.sea-ql.org/SeaORM/)

**简体中文 | [English](README.md)**

</div>

---

## ✨ 功能特性

- 🖼️ **本地壁纸管理** — 导入、浏览和管理本地壁纸文件夹
- ⭐ **收藏夹** — 将喜爱的壁纸加入收藏，快速访问
- 🔄 **一键切换壁纸** — 通过全局快捷键快速切换桌面壁纸
- 🔍 **智能排序与筛选** — 按时间、名称等多维度排序管理
- 🖱️ **拖拽排序** — 自定义壁纸展示顺序
- 🌐 **国际化** — 支持中英文双语界面
- 🎨 **自定义主题** — 个性化界面配色
- ⌨️ **全局快捷键** — 自定义快捷键操作
- 🚀 **开机自启** — 支持系统启动时自动运行（最小化到托盘）
- 🪟 **Win11 风格 UI** — 现代化毛玻璃效果界面

---

## 📸 截图

<!-- 在此处添加应用截图 -->

| 主界面 | 壁纸详情 |
|:---:|:---:|
| ![主界面](screenshots/main.png) | ![详情](screenshots/detail.png) |

| 收藏夹 | 设置 |
|:---:|:---:|
| ![收藏夹](screenshots/favorites.png) | ![设置](screenshots/settings.png) |

---

## 🛠️ 技术栈

| 层级 | 技术 |
|------|------|
| **框架** | Tauri 2.0 |
| **前端** | React 19 + TypeScript 5.8 |
| **构建工具** | Vite 8 |
| **样式** | Tailwind CSS 4 + Radix UI |
| **状态管理** | Zustand |
| **虚拟滚动** | @tanstack/react-virtual |
| **后端** | Rust (Edition 2021) |
| **数据库** | SQLite + SeaORM |
| **安装包** | NSIS Installer |

---

## 🚀 快速开始

### 环境要求

- [Node.js](https://nodejs.org/) >= 22
- [pnpm](https://pnpm.io/) (latest)
- [Rust](https://www.rust-lang.org/tools/install) (stable)
- Windows 10/11

### 安装与运行

```bash
# 克隆仓库
git clone https://github.com/mi-saka10032/mini-wallpaper.git
cd mini-wallpaper

# 安装前端依赖
pnpm install

# 开发模式运行
pnpm tauri dev

# 构建生产版本
pnpm tauri build
```

---

## 📦 下载

前往 [Releases](https://github.com/mi-saka10032/mini-wallpaper/releases) 页面下载最新版本的安装包。

---

## 📄 开源协议

本项目基于 [MIT License](LICENSE) 开源。

---

<div align="center">

Made with ❤️ by [misaka10032](https://github.com/mi-saka10032)

</div>
