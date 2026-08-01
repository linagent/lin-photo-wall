# 照片墙 (PhotoWall)

Apple TV 风格照片幻灯墙,Tauri 2 打包,支持 Windows x64 / ARM64。

## 目录结构

- `src/index.html` — 全部前端(单文件,无依赖,也可直接用浏览器打开调试)
- `src-tauri/` — Rust 后端:文件夹扫描、多线程缩略图、缓存管理
- `.github/workflows/build.yml` — 自动编译 x64 + ARM64

## 在 GitHub 上编译

1. 新建仓库(Private 即可),把本项目所有文件上传(保持目录结构,`.github` 目录不能丢)
2. 提交后 Actions 自动开始编译;首次约 15-25 分钟,之后有缓存约 5-10 分钟
3. 产物下载:
   - 日常提交 → Actions 页面对应运行记录底部 Artifacts
   - 发版 → Releases 页面 Draft a new release → 填版本号(如 v1.0.0) → Publish,安装包自动挂到 Release
4. 四个产物:
   - `PhotoWall-x64-setup.exe` / `PhotoWall-x64-portable.exe`
   - `PhotoWall-arm64-setup.exe` / `PhotoWall-arm64-portable.exe`

## 本地开发(可选)

需要 Rust(msvc) + VS Build Tools:

```
cargo install tauri-cli --version "^2"
cargo tauri dev          # 开发运行
cargo tauri build        # 本机打包
```

## 说明

- **绿色版** portable.exe 单文件直接运行;首次启动若系统缺 WebView2 会自动引导安装(Win11 自带)
- **缩略图缓存** 在 `%LocalAppData%/com.linxiao.photowall/thumbs`,设置面板可一键清理
- **SmartScreen**:未签名,首次运行点"更多信息 → 仍要运行"
- 图标在 `src-tauri/icons/`,替换同名文件即可换图标
