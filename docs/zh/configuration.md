---
title: 配置
section: guide
order: 3
locale: zh
---


所有配置文件位于 `~/.config/pikpaktui/` 目录下。

## 账号凭据——`login.yaml`

存储 PikPak 账号信息，首次登录（TUI 或 `pikpaktui login`）时自动创建。

```yaml
username: "you@example.com"
password: "your-password"
```

也可通过环境变量传入（用于 `login` 命令）：

```bash
PIKPAK_USER=you@example.com PIKPAK_PASS=yourpassword pikpaktui login
```

:::callout[warning]{kind="warn"}
凭据以明文存储，请确保 `~/.config/pikpaktui/` 目录权限为 `chmod 700`。
:::

## TUI 与 CLI 设置——`config.toml`

主配置文件，可手动编辑，也可在 TUI 设置面板（`,`）中修改后按 `s` 保存。

```toml
[tui]
# 界面
nerd_font = false           # TUI 中使用 Nerd Font 图标（需要 Nerd Font 终端字体）
border_style = "thick"      # "rounded" | "thick" | "thick-rounded" | "double"
color_scheme = "vibrant"    # "vibrant" | "classic" | "custom"
show_help_bar = true        # 底部快捷键提示栏
quota_bar_style = "bar"     # "bar"（可视化进度条）| "percent"（百分比数字）

# 预览
show_preview = true         # 三列布局；false = 两列布局
lazy_preview = false        # 仅在光标停止移动后加载预览
preview_max_size = 65536    # 文本预览最大加载字节数（默认 64 KB）
thumbnail_mode = "auto"     # "auto" | "off" | "force-color" | "force-grayscale"
thumbnail_size = "medium"   # "small" | "medium" | "large"

# 排序（在 TUI 中用 S / R 修改时自动保存）
sort_field = "name"         # "name" | "size" | "created" | "type" | "extension" | "none"
sort_reverse = false

# 交互
move_mode = "picker"        # "picker"（双面板图形选择器）| "input"（文本输入 + Tab 补全）
cli_nerd_font = false       # CLI 输出中使用 Nerd Font 图标

# 播放
player = "mpv"              # 外部视频播放器命令；首次播放视频时在 TUI 设置

# 下载
download_jobs = 1           # 并发下载线程数（1–16）
update_check = "notify"     # "notify" | "quiet" | "off"
```

### 图片协议配置

按终端模拟器配置图片渲染协议，键名为 `$TERM_PROGRAM` 环境变量的值。首次使用该终端时自动添加条目。

```toml
[tui.image_protocols]
ghostty = "kitty"
"iTerm.app" = "iterm2"
WezTerm = "auto"
```

可选值：`"auto"`（自动检测）、`"kitty"`、`"iterm2"`、`"sixel"`。

### 自定义颜色

当 `color_scheme = "custom"` 时生效，每个值为 `[R, G, B]` 数组（0–255）。

```toml
[tui.custom_colors]
folder   = [92, 176, 255]   # 浅蓝
archive  = [255, 102, 102]  # 浅红
image    = [255, 102, 255]  # 浅品红
video    = [102, 255, 255]  # 浅青
audio    = [0, 255, 255]    # 青色
document = [102, 255, 102]  # 浅绿
code     = [255, 255, 102]  # 浅黄
default  = [255, 255, 255]  # 白色
```

在 TUI 中编辑自定义颜色：打开设置（`,`）→ 选中 **Color Scheme** → 按 `Enter` 进入颜色编辑器 → 用 `r` / `g` / `b` 分别编辑 RGB 分量。

## 自动管理的文件

以下文件由 pikpaktui 自动维护，无需手动编辑。

| 文件 | 说明 |
|------|------|
| `session.json` | 访问令牌和刷新令牌（自动刷新） |
| `downloads.json` | 未完成的下载状态（重启后可恢复） |

## 环境变量

环境变量优先级高于配置文件，适用于 CI 或临时覆盖。

| 变量 | 说明 |
|------|------|
| `PIKPAK_USER` | 账号邮箱（`pikpaktui login` 使用） |
| `PIKPAK_PASS` | 账号密码（`pikpaktui login` 使用） |
| `PIKPAK_DRIVE_BASE_URL` | 覆盖 PikPak Drive API 地址 |
| `PIKPAK_AUTH_BASE_URL` | 覆盖 PikPak 认证 API 地址 |
| `PIKPAK_CLIENT_ID` | 覆盖 OAuth Client ID |
| `PIKPAK_CLIENT_SECRET` | 覆盖 OAuth Client Secret |
| `PIKPAK_CAPTCHA_TOKEN` | 登录遭遇验证码时提供 token |

### update_check

控制更新检查行为。

- `"notify"`（默认）— 启动时检查更新，在 TUI 状态栏和 CLI stderr 中持续显示
- `"quiet"` — 静默检查，仅在 TUI 日志中显示
- `"off"` — 完全禁用更新检查

```toml
update_check = "notify"
```

:::callout[并发下载]{kind="info"}
将 `download_jobs` 设为 2–4 通常可以显著提升大批量下载速度，最大值为 16。
:::
