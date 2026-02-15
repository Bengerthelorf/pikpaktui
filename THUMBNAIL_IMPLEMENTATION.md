# 缩略图和图片协议实现总结

## 🎯 实现的功能

### 1. 自动 iTerm2 检测修复
- **问题**: iTerm2 被错误检测为 Kitty 协议，导致显示空白
- **原因**: iTerm2 响应了 Kitty 协议查询，但实际不完全支持
- **解决方案**: 在 `src/tui/draw.rs:533-541` 添加自动检测修正
  ```rust
  if picker.protocol_type() == ProtocolType::Kitty {
      if let Ok(term_program) = std::env::var("TERM_PROGRAM") {
          if term_program.contains("iTerm") {
              picker.set_protocol_type(ProtocolType::Iterm2);
          }
      }
  }
  ```

### 2. 图片协议配置项
- **配置项**: `image_protocol` in `config.toml`
- **选项**:
  - `Auto` (默认) - 自动检测，包含 iTerm2 修复
  - `Kitty` - 强制使用 Kitty 协议
  - `iTerm2` - 强制使用 iTerm2 协议
  - `Sixel` - 强制使用 Sixel 协议

- **位置**: Settings -> Preview Settings -> Image Protocol (第 8 项)
- **快捷键**: `,` 打开设置 -> j/k 导航 -> Space 编辑 -> Left/Right 切换

### 3. 渲染模式
根据 `thumbnail_mode` 配置：
- **Auto**: ratatui-image 自动检测 (图片协议 > 彩色 halfblocks)
- **ForceColor**: 手动彩色半块字符 (不使用图片协议)
- **ForceGrayscale**: 手动灰度 ASCII art
- **Off**: 不显示缩略图

## 📁 修改的文件

### 1. `src/config.rs`
- 添加 `ImageProtocol` 枚举 (L114-156)
- 在 `TuiConfig` 中添加 `image_protocol` 字段 (L367)
- 实现 `next()`/`prev()`/`display_name()` 方法

### 2. `src/tui/draw.rs`
- 导入 `ProtocolType` (L504)
- 添加 iTerm2 自动检测修复逻辑 (L533-541)
- 根据配置选择协议 (L542-556)
- 在设置界面添加 "Image Protocol" 选项 (L2047-2051)

### 3. `src/tui/handler.rs`
- 添加 case 8 处理 `image_protocol` 编辑 (L1932-1947)
- 原 case 8/9 重新编号为 9/10 (L1948-1992)
- 更新导航限制 `.min(9)` -> `.min(10)` (L1981)

## 🧪 测试脚本

### `examples/test_thumbnail.rs`
显示协议检测信息和图片，用于诊断问题。
```bash
cargo run --example test_thumbnail
```

### `examples/test_iterm2_fix.rs`
验证 iTerm2 自动修复功能。
```bash
cargo run --example test_iterm2_fix
```

### `examples/test_protocol_selection.rs`
测试所有协议选项 (Auto/Kitty/iTerm2/Sixel/Halfblocks)。
```bash
cargo run --example test_protocol_selection
# 按 'n' 切换协议，'q' 退出
```

## ✅ 验证结果

### Ghostty
- ✅ 自动检测 Kitty 协议
- ✅ 图片正常显示并占满空间
- ✅ 环境变量: `TERM_PROGRAM=ghostty`, `COLORTERM=truecolor`

### iTerm2
- ✅ 自动检测 Kitty 协议 -> 自动修正为 iTerm2
- ✅ 图片正常显示
- ✅ 环境变量: `TERM_PROGRAM=iTerm.app`, `COLORTERM=truecolor`
- ⚠️  原始检测: Kitty (错误) -> 修正后: iTerm2 (正确)

### 配置测试
- ✅ 设置界面正确显示 11 个选项 (0-10)
- ✅ Image Protocol 可以在 Auto/Kitty/iTerm2/Sixel 间切换
- ✅ 配置保存到 `~/.config/pikpaktui/config.toml`
- ✅ 重启后配置持久化

## 🔧 配置文件示例

`~/.config/pikpaktui/config.toml`:
```toml
thumbnail_mode = "auto"      # auto | off | force-color | force-grayscale
image_protocol = "auto"      # auto | kitty | iterm2 | sixel
```

## 📝 设置项索引

完整的 11 个设置项：

### UI Settings (0-3)
0. Nerd Font Icons
1. Border Style
2. Color Scheme
3. Show Help Bar

### Preview Settings (4-8)
4. Show Preview Pane
5. Lazy Preview
6. Preview Max Size
7. Thumbnail Mode
8. **Image Protocol** (新增)

### Interface Settings (9-10)
9. Move Mode
10. CLI Nerd Font

## 🐛 已知问题

1. **ratatui-image 对 iTerm2 的检测不准确**
   - 原因: iTerm2 响应 Kitty 查询但不完全支持
   - 解决: 添加了自动检测修正逻辑

2. **图片大小限制**
   - 图片不会超过原始尺寸 (如 400x300)
   - 终端缩小时会自动适应
   - 这是 ratatui-image 的设计行为

## 🔮 未来改进

1. 添加更多图片协议支持 (如 Unicode Blocks)
2. 图片缓存优化
3. 支持本地图片预览
4. 支持更多图片格式
