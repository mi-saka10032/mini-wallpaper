<div align="center">

<img src="src-tauri/icons/icon.png" width="128" height="128" alt="Mini Wallpaper Logo" />

# Mini Wallpaper

🖼️ A lightweight Windows desktop wallpaper manager

[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Tauri](https://img.shields.io/badge/Tauri-2.0-FFC131?logo=tauri&logoColor=white)](https://v2.tauri.app/)
[![React](https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=white)](https://react.dev/)
[![TypeScript](https://img.shields.io/badge/TypeScript-5.8-3178C6?logo=typescript&logoColor=white)](https://www.typescriptlang.org/)
[![Rust](https://img.shields.io/badge/Rust-2021-DEA584?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Vite](https://img.shields.io/badge/Vite-8-646CFF?logo=vite&logoColor=white)](https://vite.dev/)
[![TailwindCSS](https://img.shields.io/badge/Tailwind_CSS-4-06B6D4?logo=tailwindcss&logoColor=white)](https://tailwindcss.com/)
[![SQLite](https://img.shields.io/badge/SQLite-SeaORM-003B57?logo=sqlite&logoColor=white)](https://www.sea-ql.org/SeaORM/)

**[简体中文](README-zh.md) | English**

</div>

---

## ✨ Features

- 🖼️ **Local Wallpaper Management** — Import, browse and manage local wallpaper folders
- ⭐ **Favorites** — Add your favorite wallpapers to a collection for quick access
- 🔄 **One-click Wallpaper Switch** — Quickly change desktop wallpaper via global shortcuts
- 🔍 **Smart Sorting & Filtering** — Sort and manage by time, name, and more
- 🖱️ **Drag & Drop Sorting** — Customize wallpaper display order
- 🌐 **Internationalization** — Supports both Chinese and English interfaces
- 🎨 **Custom Themes** — Personalize interface colors
- ⌨️ **Global Shortcuts** — Customizable keyboard shortcuts
- 🚀 **Auto Start** — Launch on system startup (minimized to tray)
- 🪟 **Win11 Style UI** — Modern frosted glass effect interface

---

## 📸 Screenshots

<!-- Add your screenshots here -->

| Main View | Manage View |
|:---:|:---:|
| ![Main](screenshots/main.png) | ![Detail](screenshots/manage.png) |

| Favorites | Settings |
|:---:|:---:|
| ![Favorites](screenshots/favorites.png) | ![Settings](screenshots/settings.png) |

---

## 🛠️ Tech Stack

| Layer | Technology |
|-------|-----------|
| **Framework** | Tauri 2.0 |
| **Frontend** | React 19 + TypeScript 5.8 |
| **Bundler** | Vite 8 |
| **Styling** | Tailwind CSS 4 + Radix UI |
| **State Management** | Zustand |
| **Virtual Scroll** | @tanstack/react-virtual |
| **Backend** | Rust (Edition 2021) |
| **Database** | SQLite + SeaORM |
| **Installer** | NSIS |

---

## 🚀 Getting Started

### Prerequisites

- [Node.js](https://nodejs.org/) >= 22
- [pnpm](https://pnpm.io/) (latest)
- [Rust](https://www.rust-lang.org/tools/install) (stable)
- Windows 10/11

### Install & Run

```bash
# Clone the repository
git clone https://github.com/mi-saka10032/mini-wallpaper.git
cd mini-wallpaper

# Install frontend dependencies
pnpm install

# Run in development mode
pnpm tauri dev

# Build for production
pnpm tauri build
```

---

## 📦 Download

Visit the [Releases](https://github.com/mi-saka10032/mini-wallpaper/releases) page to download the latest installer.

---

## 📄 License

This project is licensed under the [MIT License](LICENSE).

---

<div align="center">

Made with ❤️ by [misaka10032](https://github.com/mi-saka10032)

</div>
