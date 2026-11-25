# device 层：平台基础设施

## 本层职责

提供与壁纸业务无关的 Windows 平台能力，是依赖图的最底层——不依赖 decode / render。

## 模块

| 文件 | 内容 |
|---|---|
| `com_scope.rs` | renderer 线程的 COM 生命周期守卫（RAII） |
| `window.rs` | `NativeWindow`：壁纸 HWND 的创建、样式归一、几何更新、销毁 |
| `device_lost.rs` | `DeviceLostHResult` trait + `is_device_lost()`：统一判定 D3D11/DXGI 设备丢失错误 |

## 关键实现

### COM 守卫（为什么是 RAII）

`CoInitializeEx` / `CoUninitialize` 是**线程级**且必须成对。renderer 线程有多个退出路径，手写反初始化容易漏；`ComScope` 把生命周期绑到栈变量 `Drop` 上。

- 以 STA（`COINIT_APARTMENTTHREADED`）初始化，因为该线程跑消息泵与窗口
- `RPC_E_CHANGED_MODE` 或其它 HRESULT 失败 → `should_uninitialize = false`，Drop 不调 `CoUninitialize`，避免误释放别人的 COM 状态
- 成功含 `S_FALSE`（同模型重复初始化、引用计数 +1）→ 必须由 Drop 平衡

### HWND 创建（`window.rs`）

`WS_EX_NOREDIRECTIONBITMAP` **必须在创建期声明**：DirectComposition 窗口若事后补这个样式，GDI 重定向位图仍会参与桌面合成，导致采样错误。

`Drop for NativeWindow` → `DestroyWindow(hwnd)`。unembed（恢复父窗口）由上层 `RenderSurface::destroy` 负责，因为需要幂等控制。

### 设备丢失判定（`device_lost.rs`）

两类错误来源：

- `anyhow::Error`：经多层 Context 包装，需沿 `chain()` 找 `windows::core::Error`
- `windows::core::Error`：直接 `code()`

`DeviceLostHResult` trait 统一提取 HRESULT，`is_device_lost()` 判定 `DEVICE_REMOVED` / `DEVICE_RESET` / `DRIVER_INTERNAL_ERROR`。

## 对外契约（只写输入输出）

| 输出 | 消费者 | 契约 |
|---|---|---|
| `ComScope` | renderer 入口 | `init()` 后绑定到 `run()` 的栈上；函数返回即自动反初始化 |
| `NativeWindow` | render 层 `RenderSurface` | 提供 `hwnd()` / `set_bounds()` / `show()`；Drop 自动销毁窗口。**unembed 由调用方显式负责** |
| `is_device_lost(&err)` | render 层 `RenderSurface::render` | 输入任意错误引用，输出 bool；返回 true 时调用方必须清理整组资源 |

本层**不**关心：为什么要嵌入桌面、何时切换壁纸、视频怎么解码。
